use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use tokio::io;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::gdb::Gdb;

const TMP_WORKSPACE_DIR: &str = "tmp_bin_chunks";

/// Returns the absolute path to the temporary workspace directory.
fn get_abs_tmp_workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TMP_WORKSPACE_DIR)
}

/// Prepares the temporary workspace directory by removing any existing directory
/// and then re-creating it.
///
/// # Returns
/// An `io::Result<()>` indicating whether the directory was successfully prepared.
///
/// # Notes
/// This function uses synchronous calls (e.g., `exists()`) to check for the directory.
/// For fully asynchronous behavior, consider using `tokio::fs::metadata`.
async fn prepare_tmp_workspace_dir() -> io::Result<()> {
    let absolute_path = get_abs_tmp_workspace_dir();
    log::debug!("Preparing tmp workspace at absolute path: {:?}", absolute_path);

    // Good enough method in this case
    if absolute_path.exists() {
        log::debug!("Preparing tmp workspace - erasing existing file...");
        fs::remove_dir_all(&absolute_path).await?;
    }

    fs::create_dir_all(&absolute_path).await?;
    log::debug!("Preparing tmp workspace done!");

    Ok(())
}

/// Saves a data chunk to a temporary file within the workspace directory.
///
/// # Parameters
/// - `chunk_idx`: The index of the chunk (used in the file name).
/// - `data_slice`: The data slice to be saved.
///
/// # Returns
/// An `io::Result<PathBuf>` containing the absolute path of the created file.
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

/// Uploads a binary file to external flash memory in chunks.
///
/// The upload process includes:
/// 1. Reading the entire binary file into memory.
/// 2. Splitting the file into chunks of size `chunk_size`.
/// 3. For each chunk:
///    - Saving the chunk to a temporary file.
///    - Uploading the file to a RAM buffer using the GDB interface.
///    - Triggering a copying function on the target to transfer data from RAM to flash.
///    - Calculating a checksum for the chunk and comparing it with the target's checksum.
///
/// # Parameters
/// - `gdb`: A mutable reference to an active GDB connection.
/// - `binary_filepath`: The path to the binary file to be uploaded.
/// - `ram_buffer_name`: The name of the RAM buffer on the target device.
/// - `chunk_size`: The maximum size (in bytes) of each chunk.
/// - `flash_start_offset`: The starting offset in external flash memory for data writing.
/// - `coping_function_name`: The name of the function that triggers copying from RAM to flash.
///    *Note: Consider renaming this parameter (e.g., to `copying_function_name`).
///
/// # Returns
/// - `Ok(())` if the upload is successful and all checksums match.
/// - `Err(io::Error)` if an I/O error occurs or if a checksum mismatch is detected.
pub async fn upload_binary_file_to_external_flash<P, F>(
    gdb: &mut Gdb,
    binary_filepath: P,
    ram_buffer_name: &str,
    chunk_size: usize,
    flash_start_offset: usize,
    coping_function_name: &str,
    per_chunk_handler: Option<F>
) -> io::Result<()> 
where
    P: AsRef<Path> + Debug,
    F: Fn(usize, usize, usize, usize, u128) + 'static
{
    let file_data = fs::read(&binary_filepath).await?;
    let total_data_size = file_data.len();
    let chunks_count = (total_data_size / chunk_size) + if total_data_size % chunk_size != 0 { 1 } else { 0 };
    log::info!("Loaded file {:?}, got {} B. Packets to upload: {} up to {} B each.", 
        binary_filepath, total_data_size, chunks_count, chunk_size
    );

    // Create or recreate temp files directory
    // It will be used to store files to be transfered 
    // via GDB to target MCU RAM buffer.
    prepare_tmp_workspace_dir().await?;

    let mut remaining_bytes = total_data_size;
    let mut data_offset = 0;
    let mut chunk_idx: usize = 0;
    let mut flash_offset: usize = flash_start_offset;
    let mut bytes_trasfered = 0;

    let system_time_start = SystemTime::now();

    while remaining_bytes > 0 {
        // Determine the number of bytes for the current chunk.
        let chunk_bytes = if remaining_bytes > chunk_size { chunk_size } else { remaining_bytes };
        log::info!("Preparing chunk_idx={chunk_idx}/{chunks_count}, chunk_size={chunk_bytes} B, remaining={remaining_bytes} B.");

        let data_slice_start = data_offset;
        let data_slice_end = data_slice_start + chunk_bytes;
        let data_slice = &file_data[data_slice_start..data_slice_end];

        // Calculate the checksum for the current chunk.
        let data_slice_checksum = data_slice.iter().fold(0u32, |acc, &v| acc.wrapping_add(v as u32));

        // Save the chunk to a temporary file.
        let chunk_abs_file_path = save_chunk_tmp_file(
            chunk_idx,
            data_slice
        ).await?;

        // Upload the temporary file to the target's RAM.
        let result = gdb.write_binary_file_to_mem(ram_buffer_name, &chunk_abs_file_path).await?;
        log::info!("Got RAM writing results: {result:?}");

        // Trigger the copying function to move the data from RAM to external flash.
        let target_checksum = gdb.call_with_u32_u32_resulting_u32(
            coping_function_name, 
            flash_offset as u32, 
            chunk_bytes as u32,
            true        
        ).await?;

        log::info!("Got target_checksum={target_checksum}, host_checksum={data_slice_checksum}, matches={}", 
            target_checksum == data_slice_checksum
        );

        // Compare the computed checksum with the target's checksum.
        if data_slice_checksum != target_checksum {
            log::error!("Compare with host checksum={data_slice_checksum}...");
            return Err(io::Error::new(
                io::ErrorKind::InvalidData, 
                format!("Checksum not match host={data_slice_checksum} target={target_checksum}")
            ));
        }

        bytes_trasfered += chunk_bytes;
        if let Some(chunk_handle) = per_chunk_handler.as_ref() {
            let time_since_start = system_time_start.duration_since(UNIX_EPOCH).expect("Time went backwards");
            chunk_handle(
                chunk_idx, 
                chunks_count, 
                bytes_trasfered,
                total_data_size,
                time_since_start.as_millis()
            )
        }

        // Update indices and offsets for the next iteration.
        chunk_idx += 1;
        flash_offset += chunk_bytes;
        data_offset += chunk_bytes;
        remaining_bytes -= chunk_bytes;
    }

    Ok(())
}