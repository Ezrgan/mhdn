use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use mhdn_rpc::MemorySource;

use crate::error::{ProbeError, Result};

const CHUNK: usize = 64 * 1024;

/// Read `[start, end)` from a memory source. `end` is exclusive.
pub fn read_range(mem: &mut dyn MemorySource, start: u32, end: u32) -> Result<Vec<u8>> {
    let len = range_len(start, end)?;
    let mut out = vec![0u8; len];
    let mut offset = 0usize;
    while offset < len {
        let n = (len - offset).min(CHUNK);
        let addr = start.wrapping_add(offset as u32);
        mem.read(addr, &mut out[offset..offset + n])?;
        offset += n;
    }
    Ok(out)
}

pub fn write_dump(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    Ok(())
}

pub fn dump_to_file(
    mem: &mut dyn MemorySource,
    start: u32,
    end: u32,
    path: &Path,
) -> Result<DumpStats> {
    let started = Instant::now();
    let bytes = read_range(mem, start, end)?;
    write_dump(path, &bytes)?;
    Ok(DumpStats {
        bytes: bytes.len(),
        elapsed: started.elapsed(),
    })
}

pub struct DumpStats {
    pub bytes: usize,
    pub elapsed: std::time::Duration,
}

fn range_len(start: u32, end: u32) -> Result<usize> {
    if end <= start {
        return Err(ProbeError::msg(format!(
            "dump end 0x{end:08X} must be greater than start 0x{start:08X}"
        )));
    }
    Ok((end - start) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_rpc::FileMemorySource;
    use std::time::Duration;

    #[test]
    fn reads_a_slice_of_a_flat_dump() {
        let image = (0u8..200).collect::<Vec<_>>();
        let mut mem = FileMemorySource::from_bytes(image, 0x0010_0000);
        let got = read_range(&mut mem, 0x0010_0010, 0x0010_0020).unwrap();
        assert_eq!(got, (0x10u8..0x20).collect::<Vec<_>>());
    }

    #[test]
    fn writes_the_range_to_disk() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "mhdn-dump-test-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos()
        ));
        let mut mem = FileMemorySource::from_bytes(vec![1, 2, 3, 4, 5, 6, 7, 8], 0x1000);
        let stats = dump_to_file(&mut mem, 0x1002, 0x1006, &path).unwrap();
        assert_eq!(stats.bytes, 4);
        let written = std::fs::read(&path).unwrap();
        assert_eq!(written, vec![3, 4, 5, 6]);
        let _ = std::fs::remove_file(&path);
    }
}
