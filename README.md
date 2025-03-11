# GDB Loader

GdbLoader is a command-line tool designed to upload binary files to external flash memory on embedded systems via GDB. It reads target-specific parameters from a configuration JSON file and transfers the binary in chunks using asynchronous operations.


## Features
- Asynchronous Operations: Utilizes Tokio for efficient, non-blocking I/O.
- Chunked Binary Transfer: Splits binary files into configurable chunks.
- Checksum Verification: Ensures data integrity by comparing host and target checksums.
- Extensible Configuration: Uses a JSON configuration file to specify target parameters.

## Requirements
- Target Device: Your target device should support remote debugging via GDB.
- Running GDB server like Segger JLink typically on localhost:61234

## Installation
Install using cargo:
```sh
git clone https://github.com/Gieneq/GDB-Loader.git
cd gdbloader
cargo install --path .
```

## Usage

Basic example with default parameters:
```sh
cargo run -- -b C:/WS/gdbloader/res/testfiles/images.bin -g arm-none-eabi-gdb -e C:/WS/STM32U5_CMake_DevContainer_TouchGFX_Template/target/build/tmplatemkfileu5dk.elf
```
It will trasnfer binary to default RAM adress with default functions API with chunk size of 64KiB.


## License
This project is licensed under the MIT License.

