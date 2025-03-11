// #![allow(unused)]
use std::path::{Path, PathBuf};
use regex::Regex;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::select;
use tokio::time::{timeout, Duration};

pub struct Gdb {
    _gdb_subprocess: Child,
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
            _gdb_subprocess: gdb_subcommand,
            stdout_reader,
            stderr_reader,
            stdin_writer
        };

        // Make no return request
        gdb.make_request("set confirm off").await?;

        // Clear all pending responses
        let _ = gdb.await_responses(None, Duration::from_millis(250)).await;

        // Connect, can take a while
        let _ = gdb.make_request_await_response(
            format!("target remote {server}").as_str(),
            None,
            Duration::from_millis(500)
        ).await?;

        Ok(gdb)
    }

    /// Write command to GDB
    pub async fn make_request(&mut self, cmd: &str) -> io::Result<()> {
        log::debug!("Requesting cmd='{cmd}'...");
        self.stdin_writer.write_all(format!("{}\n", cmd).as_bytes()).await?;
        self.stdin_writer.flush().await
    }

    /// Collect until await time reached or excepcted count succeeded
    async fn await_responses(&mut self, expected_count: Option<usize>, await_timeout: Duration) -> Vec<String> {
        // Collect all responses until timeout
        let mut responses = Vec::new();

        let _ = timeout(await_timeout, async {

            loop {
                let mut line_stdout_buffer = String::new();
                let mut line_stderr_buffer = String::new();

                select! {
                    stdout_result = self.stdout_reader.read_line(&mut line_stdout_buffer) => {
                        match stdout_result {
                            Ok(0) => {
                                log::warn!("GDB process stdout closed unexpectedly!");
                                break;
                            },
                            Ok(_) => {
                                let trimmed_line = line_stdout_buffer.trim().to_string();
                                log::debug!("STDOUT: {trimmed_line}");
                                responses.push(trimmed_line);
                                line_stdout_buffer.clear();
                            },
                            Err(e) => {
                                log::error!("Error reading stdout: {e}");
                                break;
                            }
                        }
                    },

                    stderr_result = self.stderr_reader.read_line(&mut line_stderr_buffer) => {
                        match stderr_result {
                            Ok(0) => {
                                log::warn!("GDB process stdout closed unexpectedly!");
                                break;
                            },
                            Ok(_) => {
                                let trimmed_line = line_stderr_buffer.trim().to_string();
                                log::debug!("STDERR: {trimmed_line}");
                                responses.push(trimmed_line);
                                line_stderr_buffer.clear();
                            },
                            Err(e) => {
                                log::error!("Error reading stderr: {e}");
                                break;
                            }
                        }
                    }
                }

                // Check if collected enough responses
                if let Some(expected_responses_count) = expected_count {
                    if expected_responses_count == responses.len() {
                        log::info!("Speedup >>>>>>>>>>");
                        break;
                    }
                }
            }
        })
        .await
        .is_ok();
    
        log::debug!("Responses: {responses:?}");
        responses
    }

    pub async fn make_request_await_response(
        &mut self,
        cmd: &str,
        expected_count: Option<usize>,
        await_timeout: Duration
    ) -> Result<Vec<String>, io::Error> {
        // Make request
        self.make_request(cmd).await?;
    
        if matches!(expected_count, Some(0)) {
            // No response expected
            Ok(vec![])
        } else {
            // Collect all responses until timeout
            Ok(self.await_responses(expected_count, await_timeout).await)
        }
    }

    pub async fn quit(&mut self) -> io::Result<()> {
        self.make_request(
            "quit", 
        ).await
    }

    #[allow(unused)]
    pub async fn help(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "help", 
            None,
            Duration::from_millis(500)
        ).await
    }

    pub async fn monitor_halt(&mut self) -> Result<Vec<String>, io::Error> {
        // Nothing resulting, maybe because used after hitting breakpoint
        self.make_request_await_response(
            "monitor halt", 
            Some(0),
            Duration::from_millis(0)
        ).await
    }

    pub async fn continue_execution(&mut self) -> Result<Vec<String>, io::Error> {
        // Several lines returned, one inseide:  - chars=34, response_out='Breakpoint 1, MX_ThreadX_Init ()',
        self.make_request_await_response(
            "continue", 
            None,
            Duration::from_millis(750) // TODO not sure if it will give some output
        ).await
    }

    pub async fn monitor_reset(&mut self) -> Result<Vec<String>, io::Error> {
        // Dont know why response is in stderr
        //- chars=19, response_err='Resetting target',
        self.make_request_await_response(
            "monitor reset",
            Some(1),
            Duration::from_millis(250)
        ).await
    }

    async fn call_generic(&mut self, function: &str, has_return: bool) -> Result<String, io::Error> {
        let results = self.make_request_await_response(
            format!("call {function}").as_str(), 
            if has_return { Some(1) } else { None },
            Duration::from_millis(250)
        )
        .await?;

        extract_call_result(results, has_return)
    }

    pub async fn call(&mut self, function_name: &str, has_return: bool) -> Result<String, io::Error> {
        // Can have return, then it outputs something like this:
        // chars=15, response_out='$23 = 118 'v''.
        // If target function has void return type then result empty.
        self.call_generic(format!("{function_name}()").as_str(), has_return).await
    }

    #[allow(unused)]
    pub async fn call_with_u32(&mut self, function_name: &str, arg: u32, has_return: bool) -> Result<String, io::Error> {
        // seems has return if function returns something chars=15, response_out='$23 = 118 'v''
        self.call_generic(format!("{function_name}({arg})").as_str(), has_return).await
    }

    pub async fn call_with_u32_u32(&mut self, function_name: &str, arg1: u32, arg2: u32, has_return: bool) -> Result<String, io::Error> {
        // seems has return if function returns something chars=15, response_out='$23 = 118 'v''
        self.call_generic(format!("{function_name}({arg1}, {arg2})").as_str(), has_return).await
    }

    pub async fn call_with_u32_u32_resulting_u32(&mut self, function_name: &str, arg1: u32, arg2: u32, has_return: bool) -> Result<u32, io::Error> {
        // seems has return if function returns something chars=15, response_out='$23 = 118 'v''
        let result = self.call_with_u32_u32(function_name, arg1, arg2, has_return)
            .await?;
        extract_variable_value_from_response_line(&result)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read format corrupted"))
    }

    #[allow(unused)]
    pub async fn read_variable_u32(&mut self, variable_name: &str) -> Result<u32, io::Error> {
        // 1line - chars=15, response_out='$12 = 8228421',
        let response = self.make_request_await_response(
            format!("print {variable_name}").as_str(), 
            Some(1),
            Duration::from_millis(250)
        ).await?;

        let first_line = response.first().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read missing result"))?;
        extract_variable_value_from_response_line(first_line)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read format corrupted"))
    }


    pub async fn break_at(&mut self, function_name: &str) -> Result<Vec<String>, io::Error> {
        // 1 line: - chars=131, response_out='Breakpoint 1 at 0x8009bc8: file /workspaces/STM32U5_CMake_DevContainer_TouchGFX_Template/target/Core/Src/app_threadx.c, line 118.'
        self.make_request_await_response(
            format!("break {function_name}").as_str(), 
            Some(1),
            Duration::from_millis(750)
        ).await
    }

    pub async fn monitor_sleep(&mut self, millis: u32) -> Result<Vec<String>, io::Error> {
        // 1 line, "Sleep 250ms"
        self.make_request_await_response(
            format!("monitor sleep {millis}").as_str(), 
            Some(1),
            Duration::from_millis(millis as u64 + 250)
        ).await
    }

    pub async fn write_binary_file_to_mem<P>(&mut self, ram_buffer_name: &str, binary_filepath: P) -> Result<u32, io::Error> 
    where 
        P: AsRef<Path>
    {
        // 1 line - chars=106, response_out='Restoring binary file C:\WS\gdbloader\tmp_bin_chunks\chunk_0_.bin into memory (0x200b76a8 to 0x200c76a8)',
        let lines = self.make_request_await_response(
            format!(
                "restore {} binary {}", 
                binary_filepath.as_ref().to_str().unwrap(), 
                ram_buffer_name
            ).as_str(),
            Some(1),
            Duration::from_millis(1000)
        ).await?;

        let first_line = lines.first().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read missing result"))?;
        let (from_address, to_address) = extract_adresses_from_response_line(first_line)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Corrupted result format"))?;
        let bytes_count = to_address - from_address;
        Ok(bytes_count)
    }

    // pub async fn read_binary_file_from_mem<P>(&mut self, start_address: u32, end_address: u32, result_filepath: P) -> Result<Vec<String>, io::Error> 
    // where 
    //     P: AsRef<Path>
    // {
    //     // gdb will create file
    //     self.make_request_await_response(
    //         format!(
    //             "dump binary memory {} {:#x} {:#x}",
    //             result_filepath.as_ref().to_str().unwrap(), 
    //             start_address, 
    //             end_address
    //         )
    //         .as_str(),
    //         None, // TODO examin
    //         Duration::from_millis(1000)
    //     ).await
    // }


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

fn extract_call_result(results: Vec<String>, has_return: bool) -> Result<String, io::Error> {
    if !has_return {
        Ok(String::new())
    } else {
        match results.first() {
            Some(s) => Ok(s.to_owned()),
            None => Err(io::Error::new(io::ErrorKind::InvalidData, "call with no output"))
        }
    }
}

// impl Drop for Gdb {
//     fn drop(&mut self) {
//         tokio::spawn(async move {

//         })
//     }
// }