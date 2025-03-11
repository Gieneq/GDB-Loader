mod gdb;
mod loader;

use std::path::PathBuf;

use gdb::Gdb;
use loader::upload_binary_file_to_external_flash;

// TODO replace with CLI params or config file
const ELF_ABS_PATH: &str = "C:/WS/STM32U5_CMake_DevContainer_TouchGFX_Template/target/build/tmplatemkfileu5dk.elf";
// arm-none-eabi-gdb -q C:/WS/STM32U5_CMake_DevContainer_TouchGFX_Template/target/build/tmplatemkfileu5dk.elf
// const BIN_PATH: &str = "C:/WS/gdbloader/res/testfiles/images.bin";
const BIN_PATH: &str = "C:/WS/gdbloader/res/testfiles/big_images.bin";
const GDB_EXEC: &str = "arm-none-eabi-gdb";
const GDB_SERVER: &str = "localhost:61234"; //"host.docker.internal:61234"
// target remote localhost:61234
// restore C:/WS/gdbloader/res/testfiles/images.bin binary loader_ram_buffer
// restore C:/WS/gdbloader/res/testfiles/images.bin binary &loader_ram_buffer
// restore C:/WS/gdbloader/res/testfiles/images.bin binary 0x200b76a8

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut gdb = Gdb::try_new(
        PathBuf::from(GDB_EXEC), 
        PathBuf::from(ELF_ABS_PATH), 
        GDB_SERVER.to_string()
    ).await?;

    // gdb.help().await?;

    gdb.monitor_reset().await?;

    gdb.break_at("MX_ThreadX_Init").await?;

    gdb.continue_execution().await?;

    gdb.monitor_halt().await?;

    // Chunk size should match bock size
    upload_binary_file_to_external_flash(
        &mut gdb,
        BIN_PATH, 
        "loader_ram_buffer", 
        64 * 1024, 
        0x0, 
        "loader_checksum",
        "loader_copy_to_ext_flash"
    ).await?;

    for _ in 0..3 {
        gdb.call("green_togl", false).await?;
        gdb.monitor_sleep(250).await?;
    }
 
    gdb.quit().await?; // TODO implement drop

    Ok(())
}