mod gdb;
mod loader;

use std::{path::PathBuf, time::Duration};
use clap::Parser;
use tokio::io;

use gdb::Gdb;
use loader::upload_binary_file_to_external_flash;

#[derive(Debug, Parser)]
#[command(version, about = "Image dithering and palette extraction tool", long_about = None)]
struct Cli {
    /// Input binary file path (required).
    #[arg(short = 'b', long = "binary", value_name = "BINARY_PATH", required = true)]
    binary_path: PathBuf,
    
    /// arm-none-eabi-gdb executive path (required).
    #[arg(short = 'g', long = "gdb", value_name = "GDB_PATH", required = true)]
    gdb_path: PathBuf,
    
    /// Firmware elf file path (required).
    #[arg(short = 'e', long = "elf", value_name = "ELF_PATH", required = true)]
    elf_path: PathBuf,

    /// Name of target function at which program should break before uploading.
    #[arg(short = 'B', long = "break", value_name = "BREAK_FUN", default_value_t = String::from("MX_ThreadX_Init"))]
    break_function_name: String,

    /// Target RAM buffer name.
    #[arg(short = 'r', long = "rambuf", value_name = "RAM_BUFFER", default_value_t = String::from("loader_ram_buffer"))]
    ram_buffer_name: String,

    /// Target copy function name.
    #[arg(short = 'c', long = "copy", value_name = "COPY_FUN", default_value_t = String::from("loader_copy_to_ext_flash"))]
    copy_function_name: String,

    /// GDB server address.
    #[arg(short = 's', long = "server", value_name = "SERVER-ADDRESS", default_value_t = String::from("localhost:61234"))]
    server_address: String,

    /// Chunk size, should be multiple of FLASH memory unit size.
    #[arg(short = 'C', long = "chunk", value_name = "CHUNK_SIZE", default_value_t = 64 * 1024)]
    chunk_size_bytes: usize,

    /// Offset at which saving will start, should be multiple of FLASH memory unit size.
    #[arg(short = 'o', long = "offset", value_name = "FLASH_OFFSET", default_value_t = 0x0)]
    flash_save_offset: usize,
    
    /// Additional information about execution process (optional)
    #[arg(short = 'd', long = "debug", value_name = "DEBUG_ENABLED", default_value_t = false)]
    debug: bool  
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli_args = Cli::parse();
    
    if cli_args.debug {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .format_timestamp_millis()
            .format_file(true)
            .format_file(true)
            .format_line_number(true)
            .init();
    }

    log::debug!("Got args: '{:?}'.", cli_args);

    run_procedure(cli_args)
        .await
        .map_err(|e| e.into())
}

fn per_chunk_handler(
    chunk_idx: usize, 
    chunks_total_count: usize, 
    processed_data: usize,
    total_data: usize,
    millis_since_start: u128
) {
    let chunks_done = chunk_idx + 1;
    println!("{millis_since_start} ms, chunk={chunks_done}/{chunks_total_count}, bytes={processed_data}/{total_data}B;")
}

async fn run_procedure(cli_args: Cli) -> io::Result<()> {
    let mut gdb = Gdb::try_new(
        cli_args.gdb_path, 
        cli_args.elf_path, 
        cli_args.server_address
    ).await?;

    gdb.monitor_reset().await?;

    gdb.break_at(&cli_args.break_function_name).await?;

    // tokio::time::sleep(Duration::from_secs(1)).await;

    gdb.continue_execution().await?;

    // tokio::time::sleep(Duration::from_secs(1)).await;

    gdb.monitor_halt().await?;

    // tokio::time::sleep(Duration::from_secs(1)).await;

    // Chunk size should match bock size
    upload_binary_file_to_external_flash(
        &mut gdb,
        cli_args.binary_path, 
        &cli_args.ram_buffer_name, 
        cli_args.chunk_size_bytes, 
        cli_args.flash_save_offset, 
        &cli_args.copy_function_name,
        Some(per_chunk_handler)
    ).await?;

    gdb.monitor_sleep(250).await?;
 
    gdb.quit_and_wait().await?; // TODO implement drop

    Ok(())
}