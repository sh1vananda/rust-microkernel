use crate::vfs::register_file;
use crate::{serial_println, serial_print};
use core::str;

/// Parses a USTAR format tarball loaded into memory and mounts its contents into the VFS.
/// Returns the number of files successfully mounted.
pub fn init(archive: &'static [u8]) -> Result<usize, &'static str> {
    if archive.is_empty() {
        return Err("Archive is empty");
    }

    let mut count = 0;
    let mut offset = 0;

    while offset + 512 <= archive.len() {
        let header = &archive[offset..offset + 512];
        
        // The end of a tar archive is indicated by two consecutive 512-byte blocks of null bytes.
        // We'll just check if the first byte of the filename is null to detect the end.
        if header[0] == 0 {
            break;
        }

        // Parse Name (100 bytes)
        let name_end = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
        let name = match str::from_utf8(&header[0..name_end]) {
            Ok(n) => n,
            Err(_) => {
                serial_println!("[INITRAMFS] Skipped file with invalid UTF-8 name");
                offset += 512;
                continue;
            }
        };

        // Parse Size (12 bytes, octal, null or space terminated)
        let size_str_end = header[124..136].iter().position(|&c| c == 0 || c == b' ').unwrap_or(12);
        let size_str = str::from_utf8(&header[124..124 + size_str_end]).unwrap_or("0");
        let size = usize::from_str_radix(size_str, 8).unwrap_or(0);

        // Parse Type flag (1 byte)
        let type_flag = header[156];
        
        // Move offset past header
        offset += 512;

        // Regular file ('0' or null byte)
        if type_flag == b'0' || type_flag == 0 {
            if offset + size > archive.len() {
                serial_println!("[INITRAMFS] Warning: File {} extends beyond archive boundaries", name);
                break;
            }

            let file_data = &archive[offset..offset + size];
            register_file(name, file_data);
            count += 1;
            
            serial_println!("[INITRAMFS] Mounted: {} ({} bytes)", name, size);
            serial_print!("  [HEX] ");
            let dump_len = core::cmp::min(size, 120);
            for b in &file_data[0..dump_len] {
                serial_print!("{:02x} ", b);
            }
            serial_println!("");
        }

        // Move offset past file contents. Blocks are always exactly 512 bytes aligned.
        let aligned_size = (size + 511) & !511;
        offset += aligned_size;
    }

    Ok(count)
}
