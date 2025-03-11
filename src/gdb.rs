use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use regex::Regex;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::select;
use tokio::time::{timeout, Duration};

pub struct Gdb {
    gdb_subprocess: Child,
    stdout_reader: BufReader<ChildStdout>,
    stderr_reader: BufReader<ChildStderr>,
    stdin_writer: BufWriter<ChildStdin>,
}

/// A wrapper for interacting with a GDB process asynchronously.
///
/// This struct spawns a GDB subprocess and provides methods to send commands,
/// receive responses, and perform common actions such as setting breakpoints,
/// calling functions, and transferring binary data. Many commands expect responses
/// in specific formats as noted in the method documentation.
impl Gdb {
    /// Creates a new GDB instance by spawning a GDB subprocess.
    ///
    /// # Parameters
    /// - `executive_path`: The path to the GDB executable.
    /// - `target_elf_path`: The path to the target ELF file.
    /// - `server`: The remote server address to connect to.
    ///
    /// # Process Flow
    /// 1. Spawns the GDB process with piped stdin, stdout, and stderr.
    /// 2. Sends the command `"set confirm off"` (no expected response).
    /// 3. Clears any pending responses.
    /// 4. Connects to the remote server with `"target remote {server}"` (response may take time).
    ///
    /// # Returns
    /// Returns an instance of `Gdb` on success.
    pub async fn try_new(
        executive_path: PathBuf,
        target_elf_path: PathBuf,
        server: String,
    ) -> Result<Self, io::Error> {
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

        // Send "set confirm off" with no expected return response.
        gdb.make_request("set confirm off").await?;

        // Clear all pending responses.
        let _ = gdb.await_responses(None, Duration::from_millis(250)).await;

        // Connect to the target; this command can take a while.
        let _ = gdb.make_request_await_response(
            format!("target remote {server}").as_str(),
            None,
            Duration::from_millis(500)
        ).await?;

        Ok(gdb)
    }

    /// Sends a command to GDB.
    ///
    /// # Parameters
    /// - `cmd`: The command string to be sent.
    ///
    /// # Returns
    /// An `io::Result<()>` indicating whether the command was successfully written.
    pub async fn make_request(&mut self, cmd: &str) -> io::Result<()> {
        log::debug!("Requesting cmd='{cmd}'...");
        self.stdin_writer.write_all(format!("{}\n", cmd).as_bytes()).await?;
        self.stdin_writer.flush().await
    }

    /// Awaits responses from GDB until the timeout or until the expected number of responses is collected.
    ///
    /// # Parameters
    /// - `expected_count`: Optional expected number of responses.
    /// - `await_timeout`: The maximum duration to wait for responses.
    ///
    /// # Returns
    /// A `Vec<String>` containing the lines received from GDB.
    async fn await_responses(&mut self, expected_count: Option<usize>, await_timeout: Duration) -> Vec<String> {
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

                // If a specific number of responses was expected and reached, exit early.
                if let Some(expected_responses_count) = expected_count {
                    if expected_responses_count == responses.len() {
                        log::trace!("Collected enough responses {expected_responses_count}.");
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

    /// Sends a command to GDB and awaits responses.
    ///
    /// # Parameters
    /// - `cmd`: The command string to be sent.
    /// - `expected_count`: Optional expected number of responses.
    /// - `await_timeout`: The maximum duration to wait for responses.
    ///
    /// # Returns
    /// A `Result` with a vector of response lines, or an `io::Error`.
    pub async fn make_request_await_response(
        &mut self,
        cmd: &str,
        expected_count: Option<usize>,
        await_timeout: Duration
    ) -> Result<Vec<String>, io::Error> {
        // Make request
        self.make_request(cmd).await?;
    
        if matches!(expected_count, Some(0)) {
            // No response is expected.
            Ok(vec![])
        } else {
            Ok(self.await_responses(expected_count, await_timeout).await)
        }
    }

    /// Sends the "quit" command to GDB and wait until subprocess is finished.
    ///
    /// # Returns
    /// An `io::Result<()>` indicating whether the command was successfully sent.
    pub async fn quit_and_wait(&mut self) -> io::Result<()> {
        self.make_request(
            "quit", 
        ).await?;

        self.gdb_subprocess
            .wait()
            .await
            .map(|status_code| {
                if status_code.success() {
                    log::info!("Subprocess finished successfully!");
                } else {
                    log::info!("Subprocess finished failed: {}!", status_code.to_string());
                };
            })       
    }

    /// Sends the "help" command to GDB and awaits the response.
    ///
    /// # Returns
    /// A `Result` containing the help text lines or an `io::Error`.
    #[allow(unused)]
    pub async fn help(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "help", 
            None,
            Duration::from_millis(500)
        ).await
    }

    /// Sends the "monitor halt" command.
    ///
    /// # Expected Result
    /// Generally, no response is expected after sending this command.
    ///
    /// # Returns
    /// A `Result` containing an empty vector or an `io::Error`.
    pub async fn monitor_halt(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "monitor halt", 
            Some(0),
            Duration::from_millis(0)
        ).await
    }

    /// Sends the "continue" command to resume execution.
    ///
    /// # Expected Result
    /// Several lines may be returned. For example, one of the lines might be:
    /// `Breakpoint 1, MX_ThreadX_Init ()`
    ///
    /// # Returns
    /// A `Result` containing the response lines or an `io::Error`.
    pub async fn continue_execution(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "continue", 
            None,
            Duration::from_millis(750)
        ).await
    }

    /// Sends the "monitor reset" command to reset the target.
    ///
    /// # Expected Result
    /// A single stderr response line, for example:
    /// `Resetting target`
    ///
    /// # Returns
    /// A `Result` containing the response lines or an `io::Error`.
    /// 
    /// # Note
    /// Response for some reason is on stderr.
    pub async fn monitor_reset(&mut self) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            "monitor reset",
            Some(1),
            Duration::from_millis(250)
        ).await
    }

    /// Generic helper to call a function on the target.
    ///
    /// # Parameters
    /// - `function`: The function call string (e.g., `"foo()"` or `"bar(42)"`).
    /// - `has_return`: Indicates if a return value is expected.
    ///
    /// # Returns
    /// On success, returns the output of the function call as a `String` (empty if no return is expected).
    async fn call_generic(&mut self, function: &str, has_return: bool) -> Result<String, io::Error> {
        let results = self.make_request_await_response(
            format!("call {function}").as_str(), 
            if has_return { Some(1) } else { None },
            Duration::from_millis(250)
        )
        .await?;

        extract_call_result(results, has_return)
    }

    /// Calls a function on the target with no arguments.
    ///
    /// # Expected Result
    /// - If the function returns a value, the output might be something like:
    ///   `$23 = 118 'v'`
    /// - If the function returns void, an empty string is returned.
    ///
    /// # Parameters
    /// - `function_name`: The name of the function to call.
    /// - `has_return`: Whether a return value is expected.
    ///
    /// # Returns
    /// A `Result` containing the function output or an `io::Error`.
    #[allow(unused)]
    pub async fn call(&mut self, function_name: &str, has_return: bool) -> Result<String, io::Error> {
        self.call_generic(format!("{function_name}()").as_str(), has_return).await
    }

    /// Calls a function on the target with one `u32` argument.
    ///
    /// # Expected Result
    /// Works similarly to [`Gdb::call`], returning the function's output if a return value is expected.
    ///
    /// # Parameters
    /// - `function_name`: The name of the function to call.
    /// - `arg`: The `u32` argument.
    /// - `has_return`: Whether a return value is expected.
    ///
    /// # Returns
    /// A `Result` containing the function output or an `io::Error`.
    #[allow(unused)]
    pub async fn call_with_u32(
        &mut self, 
        function_name: &str, 
        arg: u32, 
        has_return: bool
    ) -> Result<String, io::Error> {
        self.call_generic(format!("{function_name}({arg})").as_str(), has_return).await
    }

    /// Calls a function on the target with two `u32` arguments.
    ///
    /// # Expected Result
    /// Works similarly to [`Gdb::call`], returning the function's output if a return value is expected.
    ///
    /// # Parameters
    /// - `function_name`: The name of the function to call.
    /// - `arg1`: The first `u32` argument.
    /// - `arg2`: The second `u32` argument.
    /// - `has_return`: Whether a return value is expected.
    ///
    /// # Returns
    /// A `Result` containing the function output or an `io::Error`.
    pub async fn call_with_u32_u32(
        &mut self, function_name: &str, 
        arg1: u32, 
        arg2: u32, 
        has_return: bool
    ) -> Result<String, io::Error> {
        self.call_generic(format!("{function_name}({arg1}, {arg2})").as_str(), has_return).await
    }

    /// Calls a function on the target with two `u32` arguments and extracts a `u32` return value.
    ///
    /// # Expected Result
    /// For example, if the function output is `$23 = 118 'v'`, this method extracts and returns `118`.
    ///
    /// # Parameters
    /// - `function_name`: The name of the function to call.
    /// - `arg1`: The first `u32` argument.
    /// - `arg2`: The second `u32` argument.
    /// - `has_return`: Whether a return value is expected.
    ///
    /// # Returns
    /// A `Result` containing the extracted `u32` value or an `io::Error` if request or parsing fails.
    pub async fn call_with_u32_u32_resulting_u32(
        &mut self, 
        function_name: &str, 
        arg1: u32, 
        arg2: u32, 
        has_return: bool
    ) -> Result<u32, io::Error> {
        let result = self.call_with_u32_u32(function_name, arg1, arg2, has_return)
            .await?;
        extract_variable_value_from_response_line(&result)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read format corrupted"))
    }

    /// Reads a `u32` variable from the target.
    ///
    /// # Expected Result
    /// The response should be a single line in the format, for example:
    /// `$12 = 8228421`
    ///
    /// # Parameters
    /// - `variable_name`: The name of the variable to read.
    ///
    /// # Returns
    /// A `Result` containing the parsed `u32` value or an `io::Error` if request or parsing fails.
    #[allow(unused)]
    pub async fn read_variable_u32(&mut self, variable_name: &str) -> Result<u32, io::Error> {
        let response = self.make_request_await_response(
            format!("print {variable_name}").as_str(), 
            Some(1),
            Duration::from_millis(250)
        ).await?;

        let first_line = response.first().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read missing result"))?;
        extract_variable_value_from_response_line(first_line)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Read format corrupted"))
    }

    /// Sets a breakpoint at the specified function.
    ///
    /// # Expected Result
    /// A single response line similar to:
    /// `Breakpoint 1 at 0x8009bc8: file /path/to/file, line 118.`
    ///
    /// # Parameters
    /// - `function_name`: The function where the breakpoint should be set.
    ///
    /// # Returns
    /// A `Result` containing the response lines or an `io::Error`.
    pub async fn break_at(&mut self, function_name: &str) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            format!("break {function_name}").as_str(), 
            Some(1),
            Duration::from_millis(750)
        ).await
    }

    /// Instructs the target to sleep for a specified number of milliseconds.
    ///
    /// # Expected Result
    /// A single response line similar to: `"Sleep 250ms"`
    ///
    /// # Parameters
    /// - `millis`: The number of milliseconds to sleep.
    ///
    /// # Returns
    /// A `Result` containing the response lines or an `io::Error`.
    pub async fn monitor_sleep(&mut self, millis: u32) -> Result<Vec<String>, io::Error> {
        self.make_request_await_response(
            format!("monitor sleep {millis}").as_str(), 
            Some(1),
            Duration::from_millis(millis as u64 + 250)
        ).await
    }

    /// Writes a binary file into memory.
    ///
    /// # Expected Result
    /// The response should be a single line similar to:
    /// `Restoring binary file <filepath> binary <ram_buffer_name> into memory (0x200b76a8 to 0x200c76a8)`
    /// and calculates the byte count from the resulting addresses.
    ///
    /// # Parameters
    /// - `ram_buffer_name`: The name of the RAM buffer.
    /// - `binary_filepath`: The file path of the binary file.
    ///
    /// # Returns
    /// A `Result` containing the number of bytes written or an `io::Error` if parsing fails.
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
}

/// Returns a reference to the static regex for hexadecimal addresses.
fn get_hex_adress_regex() -> &'static Regex {
    static REGEX_HEX_ADRESSES: OnceLock<Regex> = OnceLock::new();

    REGEX_HEX_ADRESSES.get_or_init(|| {
        Regex::new(r"0x([0-9a-fA-F]+)").unwrap()
    })
}

/// Returns a reference to the static regex for hexadecimal addresses.
fn get_hex_adresses_range_regex() -> &'static Regex {
    static REGEX_ADRESSES_RANGE: OnceLock<Regex> = OnceLock::new();

    REGEX_ADRESSES_RANGE.get_or_init(|| {
        Regex::new(r"\(0x([0-9a-fA-F]+) to 0x([0-9a-fA-F]+)\)").unwrap()
    })
}

/// Extracts the start and end addresses from a response line.
///
/// # Parameters
/// - `line`: A response line containing addresses in the format `(0xXXXX to 0xYYYY)`.
///
/// # Returns
/// An `Option` containing a tuple of `(start_address, end_address)` if parsing succeeds.
fn extract_adresses_from_response_line(line: &str) -> Option<(u32, u32)> {
    if let Some(captures) = get_hex_adresses_range_regex().captures(line) {
        log::trace!("{captures:?}");

        let finds = get_hex_adress_regex()
            .find_iter(line)
            .map(|a| a.as_str().into())
            .collect::<Vec<String>>();

        if finds.len() == 2 {
            let first_str = &finds[0];
            let second_str = &finds[1];
            log::trace!("From={}, to={}", first_str, second_str);

            let start_address = u32::from_str_radix(&first_str[2..], 16).ok()?;
            let end_address = u32::from_str_radix(&second_str[2..], 16).ok()?;
            log::trace!("start_address={}, end_address={}", start_address, end_address);
            return Some((start_address, end_address));
        }
    }

    None
}

/// Extracts a `u32` value from a response line.
///
/// # Parameters
/// - `line`: A response line expected to contain a number at the end.
///
/// # Returns
/// An `Option` containing the extracted `u32` value.
fn extract_variable_value_from_response_line(line: &str) -> Option<u32> {
    line.split(" ")
        .last()
        .and_then(|s| s.parse().ok())
}

/// Extracts the result of a call from the collected response lines.
///
/// # Parameters
/// - `results`: A vector of response lines from a function call.
/// - `has_return`: Whether a return value is expected.
///
/// # Returns
/// A `Result` containing the first line of the response if `has_return` is true,
/// or an empty string otherwise. Returns an `io::Error` if no output is available when expected.
fn extract_call_result(results: Vec<String>, has_return: bool) -> Result<String, io::Error> {
    if !has_return {
        Ok(String::new())
    } else {
        results
            .first()
            .cloned()
            .ok_or(io::Error::new(io::ErrorKind::InvalidData, "call with no output"))
    }
}
