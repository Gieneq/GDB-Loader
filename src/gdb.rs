#![allow(unused)]
use std::path::{Path, PathBuf};
use regex::Regex;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::time::{timeout, Duration};

pub struct Gdb {
    gdb_subprocess: Child,
    stdout_reader: BufReader<ChildStdout>,
    stderr_reader: BufReader<ChildStderr>,
    stdin_writer: BufWriter<ChildStdin>,
}

impl Gdb {
    pub async fn try_new(
        executive_path: PathBuf,
        target_elf_path: PathBuf,
        server: String,
    ) -> Result<Self, io::Error> {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .format_timestamp_millis()
            .format_file(true)
            .format_file(true)
            .format_line_number(true)
            .init();

        log::info!("Creating GDB");

        let mut gdb_subcommand = Command::new(executive_path)
            .arg("-q")
            .arg(target_elf_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to start GDB");
    
        let stdout = gdb_subcommand.stdout.take().expect("Failed to open stdout");
        let stderr = gdb_subcommand.stderr.take().expect("Failed to open stderr");
        let stdin = gdb_subcommand.stdin.take().expect("Failed to open stdin");
    
        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);
        let stdin_writer = BufWriter::new(stdin);

        let mut gdb = Self {
            gdb_subprocess: gdb_subcommand,
            stdout_reader,
            stderr_reader,
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
        // Collect all responses until timeout
        let mut response = Vec::new();

        // Those should be somehow combined
        let _ = timeout(await_timeout, async {
            let mut line_buffer = String::new();
            while let Ok(line_len) = self.stdout_reader.read_line(&mut line_buffer).await {
                if line_len == 0 {
                        log::warn!("GDB process might have exited unexpectedly!");
                        break; // Exit loop if GDB is no longer providing output
                }
                let trimmed_line = line_buffer.trim();
                log::debug!("- chars={line_len}, response_out='{trimmed_line}',");
                response.push(trimmed_line.to_string());
                
                line_buffer.clear();
            }
        })
        .await.is_ok();

        let _ = timeout(await_timeout, async {
            let mut line_buffer = String::new();
            while let Ok(line_len) = self.stderr_reader.read_line(&mut line_buffer).await {
                if line_len == 0 {
                        log::warn!("GDB process might have exited unexpectedly!");
                        break; // Exit loop if GDB is no longer providing output
                }
                let trimmed_line = line_buffer.trim();
                log::debug!("- chars={line_len}, response_err='{trimmed_line}',");
                response.push(trimmed_line.to_string());
                
                line_buffer.clear();
            }
        })
        .await.is_ok();
    
        log::debug!("Responses: {response:?}");
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

    pub async fn monitor_halt(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "monitor halt", 
            Duration::from_millis(750)
        ).await
    }

    pub async fn continue_execution(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "continue", 
            Duration::from_millis(750) // TODO not sure if it will give some output
        ).await
    }

    pub async fn monitor_reset(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "monitor reset", 
            Duration::from_millis(250)
        ).await
    }

    pub async fn call(&mut self, function_name: &str) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            format!("call {function_name}()").as_str(), 
            Duration::from_millis(250)
        ).await
    }

    pub async fn call_with_u32(&mut self, function_name: &str, arg: u32) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            format!("call {function_name}({arg})").as_str(), 
            Duration::from_millis(250)
        ).await
    }

    pub async fn read_variable_u32(&mut self, variable_name: &str) -> Result<u32, io::Error> {
        let response = self.make_request_await_response(
            format!("print {variable_name}").as_str(), 
            Duration::from_millis(250)
        ).await?;

        let first_line = response.first().expect("Response should contain at least one line");
        let value = extract_variable_value_from_response_line(first_line).unwrap();
        Ok(value)
    }


    pub async fn break_at(&mut self, function_name: &str) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            format!("break {function_name}").as_str(), 
            Duration::from_millis(750)
        ).await
    }

    pub async fn monitor_sleep(&mut self, millis: u32) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            format!("monitor sleep {millis}").as_str(), 
            Duration::from_millis(millis as u64 + 250)
        ).await
    }

    pub async fn write_binary_file_to_mem<P>(&mut self, ram_buffer_name: &str, binary_filepath: P) -> Result<u32, io::Error> 
    where 
        P: AsRef<Path>
    {
        let lines = self.make_request_await_response(
            format!(
                "restore {} binary {}", 
                binary_filepath.as_ref().to_str().unwrap(), 
                ram_buffer_name
        ).as_str(),
            Duration::from_millis(1000)
        ).await?;

        let first_line = lines.first().expect("Response should contain at least one line");
        let (from_address, to_address) = extract_adresses_from_response_line(first_line).expect("Line should contain from to adresses");
        let bytes_count = to_address - from_address;
        Ok(bytes_count)
    }

    pub async fn read_binary_file_from_mem<P>(&mut self, start_address: u32, end_address: u32, result_filepath: P) -> Result<Vec<String>, io::Error> 
    where 
        P: AsRef<Path>
    {
        // gdb will create file
        self.make_request_await_response(
            format!(
                "dump binary memory {} {:#x} {:#x}",
                result_filepath.as_ref().to_str().unwrap(), 
                start_address, 
                end_address
            )
            .as_str(),
            Duration::from_millis(1000)
        ).await
    }


}


fn extract_adresses_from_response_line(line: &str) -> Option<(u32, u32)> {
    let re = Regex::new(r"\(0x([0-9a-fA-F]+) to 0x([0-9a-fA-F]+)\)").unwrap();

    if let Some(captures) = re.captures(line) {
        println!("{captures:?}");
        let re2 = Regex::new(r"0x([0-9a-fA-F]+)").unwrap();
        let finds = re2.find_iter(line).map(|a| a.as_str().into()).collect::<Vec<String>>();
        if finds.len() == 2 {
            let first_str = &finds[0];
            let second_str = &finds[1];
            println!("From={}, to={}", first_str, second_str);

            let start_address = u32::from_str_radix(&first_str[2..], 16).ok()?;
            let end_address = u32::from_str_radix(&second_str[2..], 16).ok()?;
            println!("start_address={}, end_address={}", start_address, end_address);
            return Some((start_address, end_address));
        }
    }

    None
}

fn extract_variable_value_from_response_line(line: &str) -> Option<u32> {
    line.split(" ")
        .last()
        .and_then(|s| s.parse().ok())
}

// impl Drop for Gdb {
//     fn drop(&mut self) {
//         tokio::spawn(async move {

//         })
//     }
// }