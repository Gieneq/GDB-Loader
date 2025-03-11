use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;

use tokio::io;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::gdb::Gdb;

const TMP_WORKSPACE_DIR: &str = "tmp_bin_chunks";

fn get_abs_tmp_workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TMP_WORKSPACE_DIR)
}

async fn prepare_tmp_workspace_dir() -> io::Result<()> {
    let absolute_path = get_abs_tmp_workspace_dir();
    log::debug!("Preparing tmp workspace at absolute path: {:?}", absolute_path);

    if absolute_path.exists() {
        log::debug!("Preparing tmp workspace - erasing existing file...");
        fs::remove_dir_all(&absolute_path).await?;
    }
    assert!(!absolute_path.exists());

    fs::create_dir_all(&absolute_path).await?;
    assert!(absolute_path.exists());
    log::debug!("Preparing tmp workspace done!");

    Ok(())
}

async fn save_chunk_tmp_file(
    chunk_idx: usize,
    data_slice: &[u8]
) -> io::Result<PathBuf> {
    let tmp_file_name = format!("chunk_{}_.bin", chunk_idx);

    let tmp_dir_abs_path = get_abs_tmp_workspace_dir();
    let tmp_file_abs_path = tmp_dir_abs_path.clone().join(tmp_file_name);
    log::debug!("Saving tmp chunk: {:?} with {} B...",
        &tmp_file_abs_path, data_slice.len()
    );

    let mut chunk_file = fs::File::create_new(&tmp_file_abs_path).await?;
    chunk_file.write_all(data_slice).await?;
    chunk_file.flush().await?;

    log::debug!("Saving tmp chunk done!");
    Ok(tmp_file_abs_path)
}

/// Copy file in chunks first to RAM, then trigger
/// function to copy from RAM to external FLASH.
/// 
/// Consider erasing from GDB or using coping function. Probably best
/// option: erase 4k blocks right before coping data.
pub async fn upload_binary_file_to_external_flash<P>(
    gdb: &mut Gdb,
    binary_filepath: P,
    ram_buffer_name: &str,
    chunk_size: usize,
    flash_start_offset: usize,
    _checksum_variable_name: &str,
    coping_function_namy: &str,
) -> io::Result<()> 
where
    P: AsRef<Path> + Debug
{
    let file_data = fs::read(&binary_filepath).await?;
    let chunks_count = (file_data.len() / chunk_size) + if file_data.len() % chunk_size != 0 { 1 } else { 0 };
    log::info!("Loaded file {:?}, got {} B. Packets to upload: {} up to {} B each.", 
        binary_filepath, file_data.len(), chunks_count, chunk_size
    );

    // Create or recreate temp files directory
    // It will be used to store files to be transfered 
    // via GDB to target MCU RAM buffer.
    prepare_tmp_workspace_dir().await?;

    let mut remaining_bytes = file_data.len();
    let mut data_offset = 0;
    let mut chunk_idx: usize = 0;
    let mut flash_offset: usize = flash_start_offset;

    while remaining_bytes > 0 {
        // Prepare chunk
        let chunk_bytes = if remaining_bytes > chunk_size { chunk_size } else { remaining_bytes };
        log::info!("Preparing chunk_idx={chunk_idx}/{chunks_count}, chunk_size={chunk_bytes} B, remaining={remaining_bytes} B.");

        let data_slice_start = data_offset;
        let data_slice_end = data_slice_start + chunk_bytes;
        let data_slice = &file_data[data_slice_start..data_slice_end];

        // Calculate checksum
        let data_slice_checksum = data_slice.iter().fold(0u32, |acc, &v| acc.wrapping_add(v as u32));

        // Save temp file
        let chunk_abs_file_path = save_chunk_tmp_file(
            chunk_idx,
            data_slice
        ).await?;

        // Upload to RAM
        let result = gdb.write_binary_file_to_mem(ram_buffer_name, &chunk_abs_file_path).await?;
        log::info!("Got RAM writing results: {result:?}");

        // Copy to Ext FLASH
        let target_checksum = gdb.call_with_u32_u32_resulting_u32(
            coping_function_namy, 
            flash_offset as u32, 
            chunk_bytes as u32,
            true        
        ).await?;

        // let target_checksum = gdb.read_variable_u32(checksum_variable_name).await?;
        log::info!("Got target_checksum={target_checksum}, host_checksum={data_slice_checksum}, matches={}", 
            target_checksum == data_slice_checksum
        );

        // Compare checksums
        if data_slice_checksum != target_checksum {
            log::error!("Compare with host checksum={data_slice_checksum}...");
            return Err(io::Error::new(
                io::ErrorKind::InvalidData, 
                format!("Checksum not match host={data_slice_checksum} target={target_checksum}")
            ));
        }

        chunk_idx += 1;
        flash_offset += chunk_bytes;
        data_offset += chunk_bytes;
        remaining_bytes -= chunk_bytes;
    }

    Ok(())
}