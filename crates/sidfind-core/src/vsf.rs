//! Minimal VICE snapshot (.vsf) parsing: just enough to extract the C64's 64 KB of RAM.
//!
//! A snapshot is a header followed by modules. Every module starts with a 22-byte header (a
//! 16-byte name, a major and a minor version byte and the total size of the module including
//! this header, as a little endian u32). The `C64MEM` module holds the RAM.

const MAGIC: &[u8] = b"VICE Snapshot File\x1a";
/// Magic, version (2 bytes) and machine name (16 bytes).
const FIXED_HEADER: usize = MAGIC.len() + 2 + 16;
/// Newer snapshots follow the header with the VICE version and SVN revision (4 bytes each).
const VERSION_TAG: &[u8] = b"VICE Version\x1a";
const VERSION_INFO: usize = 8;
const MODULE_HEADER: usize = 22;
/// The `C64MEM` payload starts with the CPU port data and direction bytes, and EXROM and GAME.
const MEM_PREFIX: usize = 4;
pub const RAM_SIZE: usize = 0x10000;

pub fn is_vsf(file: &[u8]) -> bool {
    file.starts_with(MAGIC)
}

/// Returns the 64 KB of RAM of a C64 snapshot.
pub fn ram(file: &[u8]) -> Result<&[u8], String> {
    if !is_vsf(file) {
        return Err("not a VICE snapshot".into());
    }
    let machine = file.get(MAGIC.len() + 2..FIXED_HEADER).ok_or("the snapshot is truncated")?;
    let machine = machine.split(|&b| b == 0).next().unwrap_or(&[]);
    if !machine.starts_with(b"C64") {
        return Err(format!("only C64 snapshots are supported, this one is from {}", String::from_utf8_lossy(machine)));
    }

    let mut pos = FIXED_HEADER;
    if file[pos..].starts_with(VERSION_TAG) {
        pos += VERSION_TAG.len() + VERSION_INFO;
    }
    while pos + MODULE_HEADER <= file.len() {
        let header = &file[pos..pos + MODULE_HEADER];
        let size = u32::from_le_bytes(header[18..22].try_into().unwrap()) as usize;
        if size < MODULE_HEADER || size > file.len() - pos {
            return Err("the snapshot is corrupt or truncated (bad module size)".into());
        }
        if header[..16].split(|&b| b == 0).next() == Some(b"C64MEM") {
            let payload = &file[pos + MODULE_HEADER..pos + size];
            return payload
                .get(MEM_PREFIX..MEM_PREFIX + RAM_SIZE)
                .ok_or_else(|| "the snapshot's C64MEM module is too short to hold the RAM".into());
        }
        pos += size;
    }
    Err("the snapshot has no C64MEM module".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut m = vec![0u8; 16];
        m[..name.len()].copy_from_slice(name.as_bytes());
        m.extend_from_slice(&[0, 1]);
        m.extend_from_slice(&((MODULE_HEADER + payload.len()) as u32).to_le_bytes());
        m.extend_from_slice(payload);
        m
    }

    fn mem_payload(ram: &[u8]) -> Vec<u8> {
        let mut p = vec![0x37, 0x2f, 0, 0];
        p.extend_from_slice(ram);
        p.extend_from_slice(&[0xee; 15]); // VICE writes a few more bytes after the RAM
        p
    }

    fn fake_vsf(machine: &str, version_block: bool, modules: &[Vec<u8>]) -> Vec<u8> {
        let mut f = MAGIC.to_vec();
        f.extend_from_slice(&[2, 0]);
        let mut name = [0u8; 16];
        name[..machine.len()].copy_from_slice(machine.as_bytes());
        f.extend_from_slice(&name);
        if version_block {
            f.extend_from_slice(VERSION_TAG);
            f.extend_from_slice(&[0; VERSION_INFO]);
        }
        modules.iter().for_each(|m| f.extend_from_slice(m));
        f
    }

    fn ramfill() -> Vec<u8> {
        (0..RAM_SIZE).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn finds_the_ram_after_other_modules() {
        let ram_image = ramfill();
        let f = fake_vsf("C64SC", true, &[module("MAINCPU", &[0; 40]), module("C64MEM", &mem_payload(&ram_image))]);
        assert_eq!(ram(&f).unwrap(), &ram_image[..]);
    }

    #[test]
    fn works_without_the_version_block() {
        let ram_image = ramfill();
        let f = fake_vsf("C64", false, &[module("C64MEM", &mem_payload(&ram_image))]);
        assert_eq!(ram(&f).unwrap(), &ram_image[..]);
    }

    #[test]
    fn rejects_other_machines() {
        let f = fake_vsf("VIC20", true, &[module("C64MEM", &mem_payload(&ramfill()))]);
        assert!(ram(&f).unwrap_err().contains("VIC20"));
    }

    #[test]
    fn rejects_files_without_ram() {
        assert!(!is_vsf(b"not a snapshot"));
        assert!(ram(b"not a snapshot").is_err());
        assert!(ram(MAGIC).is_err());
        let f = fake_vsf("C64SC", true, &[module("MAINCPU", &[0; 40])]);
        assert!(ram(&f).unwrap_err().contains("no C64MEM"));
        let f = fake_vsf("C64SC", true, &[module("C64MEM", &[0; 100])]);
        assert!(ram(&f).unwrap_err().contains("too short"));
    }

    #[test]
    fn bad_module_sizes_do_not_panic() {
        let mut f = fake_vsf("C64SC", true, &[module("MAINCPU", &[0; 40])]);
        let size_at = FIXED_HEADER + VERSION_TAG.len() + VERSION_INFO + 18;
        for size in [0u32, 21, 62 + 1, u32::MAX] {
            f[size_at..size_at + 4].copy_from_slice(&size.to_le_bytes());
            assert!(ram(&f).is_err());
        }
    }
}
