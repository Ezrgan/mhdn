//! Sparse guest memory for offline snapshots. Missing bytes read as zero.

use std::collections::BTreeMap;

use mhdn_rpc::{MemorySource, RpcError};

use crate::tap::{PatchMemory, TapError};

#[derive(Debug, Default, Clone)]
pub struct SparseMemory {
    bytes: BTreeMap<u32, u8>,
}

impl SparseMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_bytes(&mut self, addr: u32, data: &[u8]) {
        for (index, byte) in data.iter().enumerate() {
            self.bytes.insert(addr.wrapping_add(index as u32), *byte);
        }
    }

    pub fn write_u32(&mut self, addr: u32, value: u32) {
        self.write_bytes(addr, &value.to_le_bytes());
    }

    pub fn write_u16(&mut self, addr: u32, value: u16) {
        self.write_bytes(addr, &value.to_le_bytes());
    }

    pub fn write_u8(&mut self, addr: u32, value: u8) {
        self.write_bytes(addr, &[value]);
    }

    pub fn write_f32(&mut self, addr: u32, value: f32) {
        self.write_bytes(addr, &value.to_le_bytes());
    }

    pub fn write_vec3(&mut self, addr: u32, value: [f32; 3]) {
        for (index, component) in value.iter().enumerate() {
            self.write_f32(addr.wrapping_add((index * 4) as u32), *component);
        }
    }

    fn read_bytes(&self, addr: u32, buf: &mut [u8]) {
        for (index, slot) in buf.iter_mut().enumerate() {
            *slot = self
                .bytes
                .get(&addr.wrapping_add(index as u32))
                .copied()
                .unwrap_or(0);
        }
    }
}

impl MemorySource for SparseMemory {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), RpcError> {
        self.read_bytes(addr, buf);
        Ok(())
    }
}

impl PatchMemory for SparseMemory {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), TapError> {
        self.read_bytes(addr, buf);
        Ok(())
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), TapError> {
        self.write_bytes(addr, data);
        Ok(())
    }
}
