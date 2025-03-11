# GDB Loader
Tool to interact with GDB server via arm-none-eabi-gdb

Saving works! here ~6MiB file transfered using 64KiB chunks.

```sh
[2025-03-11T09:15:48.892Z DEBUG src\gdb.rs:143 gdbloader::gdb] Responses: ["(gdb) Restoring binary file C:\\WS\\gdbloader\\tmp_bin_chunks\\chunk_102_.bin into memory (0x200b76a8 to 0x200c76a8)"]
Captures({0: 86..112/"(0x200b76a8 to 0x200c76a8)", 1: 89..97/"200b76a8", 2: 103..111/"200c76a8"})
From=0x200b76a8, to=0x200c76a8
start_address=537622184, end_address=537687720
[2025-03-11T09:15:48.897Z INFO  src\loader.rs:107 gdbloader::loader] Got RAM writing results: 65536
[2025-03-11T09:15:48.898Z DEBUG src\gdb.rs:75 gdbloader::gdb] Requesting cmd='call loader_copy_to_ext_flash(6684672, 65536)'...
[2025-03-11T09:15:48.957Z DEBUG src\gdb.rs:100 gdbloader::gdb] STDOUT: (gdb) $103 = 8199517
[2025-03-11T09:15:48.958Z INFO  src\gdb.rs:134 gdbloader::gdb] Speedup >>>>>>>>>>
[2025-03-11T09:15:48.959Z DEBUG src\gdb.rs:143 gdbloader::gdb] Responses: ["(gdb) $103 = 8199517"]
[2025-03-11T09:15:48.960Z INFO  src\loader.rs:118 gdbloader::loader] Got target_checksum=8199517, host_checksum=8199517, matches=true
[2025-03-11T09:15:48.960Z INFO  src\loader.rs:90 gdbloader::loader] Preparing chunk_idx=103/104, chunk_size=39840 B, remaining=39840 B.
[2025-03-11T09:15:48.961Z DEBUG src\loader.rs:42 gdbloader::loader] Saving tmp chunk: "C:\\WS\\gdbloader\\tmp_bin_chunks\\chunk_103_.bin" with 39840 B...
[2025-03-11T09:15:48.962Z DEBUG src\loader.rs:50 gdbloader::loader] Saving tmp chunk done!
```