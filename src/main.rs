use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::time::{timeout, Duration};

const ELF_ABS_PATH: &str = "C:/WS/STM32U5_CMake_DevContainer_TouchGFX_Template/target/build/tmplatemkfileu5dk.elf";
const GDB_EXEC: &str = "arm-none-eabi-gdb";
const GDB_SERVER: &str = "localhost:61234"; //"host.docker.internal:61234"

async fn gdb_make_request(
    gdb_stdin_writer: &mut BufWriter<ChildStdin>,
    cmd: &str,
) -> io::Result<()> {
    println!("Requesting cmd='{cmd}'...");
    gdb_stdin_writer.write_all(format!("{}\n", cmd).as_bytes()).await?;
    gdb_stdin_writer.flush().await
}

async fn gdb_await_response(
    gdb_stdout_reader: &mut BufReader<ChildStdout>,
    await_timeout: Duration
) -> Vec<String> {
    println!("Responses:");
    // Collect all responses until timeout
    let mut response = Vec::new();
    let _ = timeout(await_timeout, async {
        let mut line_buffer = String::new();
        while let Ok(line_len) = gdb_stdout_reader.read_line(&mut line_buffer).await {
            if line_len > 0 {
                println!("- chars={line_len}, response='{line_buffer}',");
                response.push(line_buffer.trim().to_string());
            }
            line_buffer.clear();
        }
    })
    .await.is_ok();
    response
}

async fn gdb_request(
    gdb_stdout_reader: &mut BufReader<ChildStdout>,
    gdb_stdin_writer: &mut BufWriter<ChildStdin>,
    cmd: &str,
    await_timeout: Duration
) -> Result<Vec<String>, io::Error> {
    // Make request
    gdb_make_request(gdb_stdin_writer, cmd).await?;

    // Collect all responses until timeout
    Ok(gdb_await_response(gdb_stdout_reader, await_timeout).await)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Start Loader");
    let mut gdb = Command::new(GDB_EXEC)
        .arg("-q")
        .arg(ELF_ABS_PATH)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start GDB");

        let stdout = gdb.stdout.take().expect("Failed to open stdout");
        let stdin = gdb.stdin.take().expect("Failed to open stdin");
    
        let mut stdout_reader = BufReader::new(stdout);
        let mut stdin_writer = BufWriter::new(stdin);

        // Clear any initial messages
        let _ = gdb_await_response(
            &mut stdout_reader, Duration::from_millis(500)
        ).await;

        // No need for confirmation, especially `quit`
        let _ = gdb_request(
            &mut stdout_reader, 
            &mut stdin_writer, 
            "set confirm off", 
            Duration::from_secs(1)
        ).await
        .unwrap();

        // Connect to target server
        let _ = gdb_request(
            &mut stdout_reader, 
            &mut stdin_writer, 
            format!("target remote {GDB_SERVER}").as_str(), 
            Duration::from_millis(500)
        ).await
        .unwrap();

        let _ = gdb_request(
            &mut stdout_reader, 
            &mut stdin_writer, 
            "quit", 
            Duration::from_secs(1)
        ).await
        .unwrap();

    // Await until the command completes
    println!("GDB process exit awaiting...");
    let status = gdb.wait().await?;
    println!("the command exited with: {}", status);
    Ok(())
}


// set $ram_addr = 0x20010000  
// set $chunk_size = 1024    
//         "set confirm off",
//         "monitor reset",
//         "break MX_ThreadX_Init",
//         "continue",
//         "monitor halt",
//         "call green_togl()",
//         "monitor sleep 500",
//         "call green_togl()",
//         "monitor sleep 500",
//         "call green_togl()",
//         "monitor sleep 500",
//         "call green_togl()",