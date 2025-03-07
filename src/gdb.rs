use std::path::PathBuf;

use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::time::{timeout, Duration};

pub struct Gdb {
    gdb_subprocess: Child,
    stdout_reader: BufReader<ChildStdout>,
    stdin_writer: BufWriter<ChildStdin>,
}

impl Gdb {
    pub async  fn try_new(
        executive_path: PathBuf,
        target_elf_path: PathBuf,
        server: String,
    ) -> Result<Self, io::Error> {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .format_timestamp_millis()
            .init();

        log::info!("Creating GDB");

        let mut gdb_subcommand = Command::new(executive_path)
            .arg("-q")
            .arg(target_elf_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to start GDB");
    
        let stdout = gdb_subcommand.stdout.take().expect("Failed to open stdout");
        let stdin = gdb_subcommand.stdin.take().expect("Failed to open stdin");
    
        let stdout_reader = BufReader::new(stdout);
        let stdin_writer = BufWriter::new(stdin);

        let mut gdb = Self {
            gdb_subprocess: gdb_subcommand,
            stdout_reader,
            stdin_writer
        };

        let _ = gdb.await_response(Duration::from_secs(1)).await;

        let _ = gdb.make_request_await_response(
            format!("target remote {server}").as_str(),
            Duration::from_millis(500)
        ).await?;
        
        let _ = gdb.make_request_await_response(
            "set confirm off",
            Duration::from_millis(100)
        ).await?;

        Ok(gdb)
    }

    pub async fn make_request(&mut self, cmd: &str) -> io::Result<()> {
        log::debug!("Requesting cmd='{cmd}'...");
        self.stdin_writer.write_all(format!("{}\n", cmd).as_bytes()).await?;
        self.stdin_writer.flush().await
    }

    async fn await_response(&mut self, await_timeout: Duration) -> Vec<String> {
        log::debug!("Responses:");
        // Collect all responses until timeout
        let mut response = Vec::new();
        let _ = timeout(await_timeout, async {
            let mut line_buffer = String::new();
            while let Ok(line_len) = self.stdout_reader.read_line(&mut line_buffer).await {
                if line_len > 0 {
                    log::debug!("- chars={line_len}, response='{line_buffer}',");
                    response.push(line_buffer.trim().to_string());
                }
                line_buffer.clear();
            }
        })
        .await.is_ok();
        response
    }

    pub async fn make_request_await_response(
        &mut self,
        cmd: &str,
        await_timeout: Duration
    ) -> Result<Vec<String>, io::Error> {
        // Make request
        self.make_request(cmd).await?;
    
        // Collect all responses until timeout
        Ok(self.await_response(await_timeout).await)
    }

    pub async fn quit(&mut self) -> io::Result<()> {
        let _ = self.make_request_await_response(
            "quit", 
            Duration::from_millis(500)
        ).await?;

        self.gdb_subprocess.wait().await?;
        log::info!("Subprocesses finished");

        Ok(())
    }

    pub async fn help(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "help", 
            Duration::from_millis(250)
        ).await
    }
}

// impl Drop for Gdb {
//     fn drop(&mut self) {
//         tokio::spawn(async move {

//         })
//     }
// }




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