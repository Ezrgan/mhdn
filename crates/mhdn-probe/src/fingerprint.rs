use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use mhdn_rpc::MemorySource;

use crate::error::{ProbeError, Result};

pub const TMD_TITLE_VERSION_OFFSET: u64 = 0x1DC;
pub const TEXT_ORIGIN: u32 = 0x0010_0000;
pub const TEXT_WINDOW_STRIDE: u32 = 0x4_0000;
pub const TEXT_WINDOW_LEN: u32 = 4096;

pub fn xxh3_64(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

pub fn text_windows(count: u32) -> Vec<(u32, u32)> {
    (0..count)
        .map(|index| (TEXT_ORIGIN + index * TEXT_WINDOW_STRIDE, TEXT_WINDOW_LEN))
        .collect()
}

pub fn hash_windows(mem: &mut dyn MemorySource, windows: &[(u32, u32)]) -> Result<Vec<(u32, u64)>> {
    let mut hashes = Vec::with_capacity(windows.len());
    for &(addr, len) in windows {
        let mut buf = vec![0u8; len as usize];
        mem.read(addr, &mut buf)?;
        hashes.push((addr, xxh3_64(&buf)));
    }
    Ok(hashes)
}

pub fn read_tmd_title_version(path: &Path) -> Result<u16> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(TMD_TITLE_VERSION_OFFSET))?;
    let mut buf = [0u8; 2];
    file.read_exact(&mut buf).map_err(|_| {
        ProbeError::msg(format!(
            "{} is too small to contain a title version at 0x{TMD_TITLE_VERSION_OFFSET:X}",
            path.display()
        ))
    })?;
    Ok(u16::from_be_bytes(buf))
}

/// Azahar's installed update TMD for MHXX JP, when this machine has one.
pub fn default_azahar_update_tmd() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(
        "Library/Application Support/Azahar/sdmc/Nintendo 3DS/00000000000000000000000000000000/00000000000000000000000000000000/title/0004000e/00197100/content",
    );
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tmd"))
        .collect();
    files.sort();
    files.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_rpc::FileMemorySource;

    #[test]
    fn xxh3_of_empty_matches_the_spec() {
        assert_eq!(xxh3_64(b""), 0x2D06_8005_38D3_94C2);
    }

    #[test]
    fn hashes_text_windows_from_a_dump() {
        let mut image = vec![0u8; 0x8_0000];
        image[0..4].copy_from_slice(b"ARM!");
        let mut mem = FileMemorySource::from_bytes(image, TEXT_ORIGIN);
        let windows = text_windows(2);
        let hashes = hash_windows(&mut mem, &windows).unwrap();
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[0].0, TEXT_ORIGIN);
        assert_ne!(hashes[0].1, hashes[1].1);
    }

    #[test]
    fn reads_a_big_endian_title_version() {
        let path = std::env::temp_dir().join(format!("mhdn-tmd-{}.tmd", std::process::id()));
        let mut bytes = vec![0u8; 0x200];
        bytes[0x1DC] = 0x10;
        bytes[0x1DD] = 0x80;
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_tmd_title_version(&path).unwrap(), 4224);
        let _ = std::fs::remove_file(&path);
    }
}
