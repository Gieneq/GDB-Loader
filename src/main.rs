mod gdb;

use std::path::PathBuf;

use gdb::Gdb;

const ELF_ABS_PATH: &str = "C:/WS/STM32U5_CMake_DevContainer_TouchGFX_Template/target/build/tmplatemkfileu5dk.elf";
const GDB_EXEC: &str = "arm-none-eabi-gdb";
const GDB_SERVER: &str = "localhost:61234"; //"host.docker.internal:61234"

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut gdb = Gdb::try_new(
        PathBuf::from(GDB_EXEC), 
        PathBuf::from(ELF_ABS_PATH), 
        GDB_SERVER.to_string()
    ).await?;

    gdb.help().await?;
 
    gdb.quit().await?; // TODO implement drop

    Ok(())
}