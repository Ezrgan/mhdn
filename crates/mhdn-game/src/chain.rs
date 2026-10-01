//! Resolve `base → [+offset, deref]*` against a memory image.
//!
//! A loaded pointer must sit in the old heap, the linear heap, or the New 3DS
//! heap at `0x30000000`, and be 4-byte aligned. The static slot that starts a
//! chain may live in the process image. The cache drops a chain when a walk
//! fails, and [`PointerCache::invalidate`] drops every chain when the scene changes.

use std::collections::HashMap;

use mhdn_rpc::MemorySource;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChainStep {
    pub offset: i32,
    pub deref: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChainError {
    #[error("read failed at 0x{addr:08X}")]
    Read { addr: u32 },
    #[error("pointer 0x{addr:08X} is not 4-byte aligned")]
    Misaligned { addr: u32 },
    #[error("pointer 0x{addr:08X} is outside a guest heap")]
    OutsideHeap { addr: u32 },
}

#[derive(Debug, Default, Clone)]
pub struct PointerCache {
    hits: HashMap<(u32, Vec<ChainStep>), u32>,
}

impl PointerCache {
    pub fn new() -> Self {
        Self {
            hits: HashMap::new(),
        }
    }

    pub fn invalidate(&mut self) {
        self.hits.clear();
    }

    pub fn forget(&mut self, base: u32, steps: &[ChainStep]) {
        self.hits.remove(&(base, steps.to_vec()));
    }

    pub fn resolve(
        &mut self,
        mem: &mut dyn MemorySource,
        base: u32,
        steps: &[ChainStep],
    ) -> Result<u32, ChainError> {
        let key = (base, steps.to_vec());
        if let Some(hit) = self.hits.get(&key).copied() {
            return Ok(hit);
        }
        match walk(mem, base, steps) {
            Ok(addr) => {
                self.hits.insert(key, addr);
                Ok(addr)
            }
            Err(err) => {
                self.hits.remove(&key);
                Err(err)
            }
        }
    }
}

fn walk(mem: &mut dyn MemorySource, base: u32, steps: &[ChainStep]) -> Result<u32, ChainError> {
    require_readable(base)?;
    let mut addr = base;
    for step in steps {
        addr = add_offset(addr, step.offset)?;
        if step.deref {
            require_readable(addr)?;
            let value = mem.read_u32(addr).map_err(|_| ChainError::Read { addr })?;
            require_heap(value)?;
            addr = value;
        } else {
            require_heap(addr)?;
        }
    }
    Ok(addr)
}

fn add_offset(addr: u32, offset: i32) -> Result<u32, ChainError> {
    let next = if offset >= 0 {
        addr.checked_add(offset as u32)
    } else {
        addr.checked_sub(offset.unsigned_abs())
    };
    next.ok_or(ChainError::OutsideHeap { addr })
}

/// Static slots in the process image, plus any heap address, can be read.
fn require_readable(addr: u32) -> Result<(), ChainError> {
    if !addr.is_multiple_of(4) {
        return Err(ChainError::Misaligned { addr });
    }
    let in_image = (0x0010_0000..0x0400_0000).contains(&addr);
    if in_image || heap_contains(addr) {
        Ok(())
    } else {
        Err(ChainError::OutsideHeap { addr })
    }
}

fn require_heap(addr: u32) -> Result<(), ChainError> {
    if !addr.is_multiple_of(4) {
        return Err(ChainError::Misaligned { addr });
    }
    if heap_contains(addr) {
        Ok(())
    } else {
        Err(ChainError::OutsideHeap { addr })
    }
}

pub(crate) fn is_guest_heap(addr: u32) -> bool {
    heap_contains(addr)
}

fn heap_contains(addr: u32) -> bool {
    (0x0800_0000..0x1000_0000).contains(&addr)
        || (0x1400_0000..0x1C00_0000).contains(&addr)
        || (0x3000_0000..0x4000_0000).contains(&addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_rpc::FileMemorySource;

    fn put_u32(image: &mut [u8], base: u32, addr: u32, value: u32) {
        let offset = (addr - base) as usize;
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn resolves_a_monster_style_chain_and_caches_it() {
        let base = 0x0800_0000u32;
        let mut image = vec![0u8; 0x6000];
        put_u32(&mut image, base, 0x0800_1000, 0x0800_2000);
        put_u32(&mut image, base, 0x0800_2014, 0x0800_3000);
        put_u32(&mut image, base, 0x0800_40A8, 0x0800_5000);
        put_u32(&mut image, base, 0x0800_5360, 774);
        let mut mem = FileMemorySource::from_bytes(image, base);
        let steps = [
            ChainStep {
                offset: 0,
                deref: true,
            },
            ChainStep {
                offset: 0x14,
                deref: true,
            },
            ChainStep {
                offset: 0x10A8,
                deref: true,
            },
            ChainStep {
                offset: 0x360,
                deref: false,
            },
        ];
        let mut cache = PointerCache::new();
        let hp = cache.resolve(&mut mem, 0x0800_1000, &steps).unwrap();
        assert_eq!(hp, 0x0800_5360);
        assert_eq!(cache.resolve(&mut mem, 0x0800_1000, &steps).unwrap(), hp);
        cache.invalidate();
        assert_eq!(cache.resolve(&mut mem, 0x0800_1000, &steps).unwrap(), hp);
        assert_eq!(mem.read_u32(hp).unwrap(), 774);
    }

    #[test]
    fn rejects_a_pointer_into_code_and_an_unaligned_object() {
        let mut cache = PointerCache::new();
        let mut mem =
            FileMemorySource::from_bytes(0x0010_0000u32.to_le_bytes().to_vec(), 0x0800_1000);
        let step = [ChainStep {
            offset: 0,
            deref: true,
        }];
        assert_eq!(
            cache.resolve(&mut mem, 0x0800_1000, &step).unwrap_err(),
            ChainError::OutsideHeap { addr: 0x0010_0000 }
        );

        let mut mem =
            FileMemorySource::from_bytes(0x3006_5559u32.to_le_bytes().to_vec(), 0x0800_1000);
        assert_eq!(
            cache.resolve(&mut mem, 0x0800_1000, &step).unwrap_err(),
            ChainError::Misaligned { addr: 0x3006_5559 }
        );
    }

    #[test]
    fn accepts_the_new_3ds_heap_used_by_live_monsters() {
        let mut image = vec![0u8; 8];
        image[..4].copy_from_slice(&0x3006_5558u32.to_le_bytes());
        let mut mem = FileMemorySource::from_bytes(image, 0x00D3_A8E0);
        let steps = [
            ChainStep {
                offset: 0,
                deref: true,
            },
            ChainStep {
                offset: 0x360,
                deref: false,
            },
        ];
        let hp = PointerCache::new()
            .resolve(&mut mem, 0x00D3_A8E0, &steps)
            .unwrap();
        assert_eq!(hp, 0x3006_58B8);
    }
}
