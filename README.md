# GDB Loader

GdbLoader is a command-line tool designed to upload binary files to external flash memory on ARM embedded systems via GDB.

## Features
- Asynchronous Operations: Utilizes Tokio for efficient, non-blocking I/O.
- Chunked Binary Transfer: Splits binary files into configurable chunks.
- Checksum Verification: Ensures data integrity by comparing host and target checksums.
- Extensible Configuration: CLI parameters.

## Requirements
- Target Device: Your target device should support remote debugging via GDB.
- Transfer API: The target device should have a pre-allocated RAM buffer for temporarily storing data chunks. Its size should be a multiple of the external FLASH memory unit (e.g., page or sector).
- GDB Server: A GDB server (such as Segger JLink) is typically available on localhost:61234.

## Transfer API
The target device must imlement a trasfer API for coping data from a temporary RAM buffer to external flash memory. For example, add the following code to your ARM project:
```C
#define LOADER_RAM_BUFFER_SIZE (64 * 1024)

volatile uint8_t __attribute__((section(".loader_ram_buff_section")))  
    loader_ram_buffer[LOADER_RAM_BUFFER_SIZE];

uint32_t __attribute__((section(".loader_code_section"))) 
    loader_copy_to_ext_flash(
        uint32_t flash_offset, 
        uint32_t loader_bytes_count) 
{
  if (loader_bytes_count > LOADER_RAM_BUFFER_SIZE) {
    Error_Handler();
  }

  // TODO Copy to flash
  (void)flash_offset;

  // Calculate checksum
  uint32_t checksum = 0;
  for (uint32_t idx = 0; idx < loader_bytes_count; ++idx) {
    checksum += (uint32_t)(loader_ram_buffer[idx]);
  }

  loader_bytes_count = 0;
  loader_checksum = checksum;

  return checksum;
}
```

Next, add the following sections to your linker script. The **KEEP** directive prevents the linker from discarding the code, and **NOLOAD** ensures that the data is not included in the final binary image:
```ld
  .loader_ram_buff_section (NOLOAD) : {
    . = ALIGN(4);
    KEEP(*(.loader_ram_buff_section))
  } > RAM

  .loader_code_section : {
    . = ALIGN(4);
    KEEP(*(.loader_code_section))
  } > FLASH
```
Compile code, check if **.map **file includes the buffer and function definitions:

```plain
.loader_ram_buff_section
                0x200b76a8    0x10000
                0x200b76a8                        . = ALIGN (0x4)
 *(.loader_ram_buff_section)
 .loader_ram_buff_section
                0x200b76a8    0x10000 CMakeFiles/tmplatemkfileu5dk.dir/Core/Src/main.c.obj
                0x200b76a8                loader_ram_buffer
                0x200c76a8                        . = ALIGN (0x4)

...

.loader_code_section
                0x08055ad8       0x68
                0x08055ad8                        . = ALIGN (0x4)
 *(.loader_code_section)
 .loader_code_section
                0x08055ad8       0x68 CMakeFiles/tmplatemkfileu5dk.dir/Core/Src/main.c.obj
                0x08055ad8                loader_copy_to_ext_flash
                0x08055b40                        . = ALIGN (0x4)
```

## Installation

Clone the repository and install using Cargo:
```sh
git clone https://github.com/Gieneq/GDB-Loader.git
cd gdbloader
cargo install --path .
```

## How it works?

Suppose you have a section in external memory called **ExtFlashSection** defined in your linker script:
```ld
  ExtFlashSection : {
    *(ExtFlashSection ExtFlashSection.*)
    *(.gnu.linkonce.r.*)
    . = ALIGN(0x4);
  } >EXT_FLASH
```
Uploading may fail if GDB cannot access addresses in the external memory bank. To fix this, you need to extract the **ExtFlashSection** from the binary and modify the linker script by adding **NOLOAD**.

First, check your `.map` file to confirm that the section is placed at the beginning of the external FLASH bank:
```plain
ExtFlashSection
                0xa0000000   0x9594b0
 *(ExtFlashSection ExtFlashSection.*)
 ExtFlashSection
```

Extract the section from the compiled binary using `objcopy`:
```sh
arm-none-eabi-objcopy -O binary --only-section=ExtFlashSection path_to_compiled_firmware.elf ext_flash_section.bin
```

Now you have a binary chunk ready to be transferred. Modify your linker script to add **NOLOAD** (be careful with spaces, as they can affect the linker script):
```ld
  ExtFlashSection (NOLOAD) : {
    *(ExtFlashSection ExtFlashSection.*)
    *(.gnu.linkonce.r.*)
    . = ALIGN(0x4);
  } >EXT_FLASH
```

Upload your firmware to the target, and then use the CLI to upload the external FLASH content:
```sh
cargo run -- -b ext_flash_section.bin -g arm-none-eabi-gdb -e path_to_compiled_firmware.elf
```
This command transfers the binary to the RAM, where a default buffer is allocated, using a default chunk size of 64 KiB.

To see full project check [ST32U5 Cmake DevContainer](https://github.com/Gieneq/STM32U5_CMake_DevContainer_TouchGFX_Template) template.

## License
This project is licensed under the MIT License.

