use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{RpcError, Result};

/// Abstract memory reader used by game logic and offline tests.
pub trait MemorySource {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()>;

    fn read_u32(&mut self, addr: u32) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.read(addr, &mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_f32x3(&mut self, addr: u32) -> Result<[f32; 3]> {
        let mut buf = [0u8; 12];
        self.read(addr, &mut buf)?;
        Ok([
            f32::from_le_bytes(buf[0..4].try_into().expect("slice")),
            f32::from_le_bytes(buf[4..8].try_into().expect("slice")),
            f32::from_le_bytes(buf[8..12].try_into().expect("slice")),
        ])
    }
}

impl MemorySource for crate::client::RpcClient {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()> {
        crate::client::RpcClient::read(self, addr, buf)
    }
}

/// Linear view of a flat memory dump: guest address `base_addr + file_offset`.
#[derive(Debug)]
pub struct FileMemorySource {
    data: Vec<u8>,
    base_addr: u32,
}

impl FileMemorySource {
    pub fn from_file(path: impl AsRef<Path>, base_addr: u32) -> Result<Self> {
        let mut file = File::open(path.as_ref())?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(Self { data, base_addr })
    }

    pub fn from_bytes(data: Vec<u8>, base_addr: u32) -> Self {
        Self { data, base_addr }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl MemorySource for FileMemorySource {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()> {
        let start = addr.checked_sub(self.base_addr).ok_or(RpcError::ReadFailed {
            addr,
        })? as usize;
        let end = start
            .checked_add(buf.len())
            .ok_or(RpcError::ReadFailed { addr })?;
        if end > self.data.len() {
            return Err(RpcError::ReadFailed { addr });
        }
        buf.copy_from_slice(&self.data[start..end]);
        Ok(())
    }
}

/// Placeholder for Fase 3 recorded sessions (`.mhrec`).
#[derive(Debug, Default)]
pub struct ReplaySource {
    _private: (),
}

impl ReplaySource {
    pub fn not_implemented() -> Self {
        Self { _private: () }
    }
}

impl MemorySource for ReplaySource {
    fn read(&mut self, _addr: u32, _buf: &mut [u8]) -> Result<()> {
        Err(RpcError::InvalidResponse)
    }
}

/// Read a slice from an on-disk dump without loading the whole file (for large dumps).
pub fn read_dump_range(
    path: impl AsRef<Path>,
    base_addr: u32,
    addr: u32,
    buf: &mut [u8],
) -> Result<()> {
    let offset = addr
        .checked_sub(base_addr)
        .ok_or(RpcError::ReadFailed { addr })? as u64;
    let mut file = File::open(path.as_ref())?;
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf).map_err(|_| RpcError::ReadFailed { addr })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_memory_source_maps_base() {
        let mem = FileMemorySource::from_bytes(vec![1, 2, 3, 4, 5, 6], 0x1000);
        let mut buf = [0u8; 2];
        mem.read(0x1002, &mut buf).unwrap();
        assert_eq!(buf, [3, 4]);
    }

    #[test]
    fn file_memory_source_out_of_range() {
        let mut mem = FileMemorySource::from_bytes(vec![0; 4], 0);
        let mut buf = [0u8; 8];
        assert!(mem.read(0, &mut buf).is_err());
    }

    #[test]
    fn replay_source_returns_error() {
        let mut replay = ReplaySource::not_implemented();
        let mut buf = [0u8; 4];
        assert!(replay.read(0, &mut buf).is_err());
    }
}
