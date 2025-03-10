mod gdb;

use std::path::PathBuf;

use gdb::Gdb;

const ELF_ABS_PATH: &str = "C:/WS/STM32U5_CMake_DevContainer_TouchGFX_Template/target/build/tmplatemkfileu5dk.elf";
const GDB_EXEC: &str = "arm-none-eabi-gdb";
const GDB_SERVER: &str = "localhost:61234"; //"host.docker.internal:61234"

/// Copy file in chunks first to RAM, then trigger
/// function to copy from RAM to external FLASH.
/// 
/// Consider erasing from GDB or using coping function. Probably best
/// option: erase 4k blocks right before coping data.
async fn upload_binary_file_to_external_flash<P>(
    binary_filepath: P,
    chunk_size: u32,
    ram_start_address: u32,
    flash_start_address: u32,
    flash_block_size: u32,
    coping_function_namy: &str,
) {
    unimplemented!()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut gdb = Gdb::try_new(
        PathBuf::from(GDB_EXEC), 
        PathBuf::from(ELF_ABS_PATH), 
        GDB_SERVER.to_string()
    ).await?;

    gdb.help().await?;

    gdb.monitor_reset().await?;

    gdb.break_at("MX_ThreadX_Init").await?;

    gdb.continue_execution().await?;

    gdb.monitor_halt().await?;

    for _ in 0..5 {
        gdb.call("green_togl").await?;

        gdb.monitor_sleep(500).await?;
    }
 
    gdb.quit().await?; // TODO implement drop

    Ok(())
}