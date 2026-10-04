//! Install and read the damage-tap code patch. Addresses stay in [`stub`] until
//! the profile grows a `[damage_tap]` section (plan 3.1).

mod callers;
mod stub;

pub use callers::{CALLER_HIT, CALLER_MOUNT_TOPPLE, CALLER_STATUS};
pub use stub::{
    hook_branch, stub_bytes, wide_stub_bytes, TapEvent, CAPACITY, CAVE_ADDR, ENTRY_SIZE,
    EXPECTED_HOOK, EXPECTED_HP_STORE, EXPECTED_NEXT, HOOK_ADDR, RETURN_ADDR, RING_ADDR,
    WIDE_CAPACITY, WIDE_ENTRY_SIZE, WIDE_RING_SPAN, WIDE_WRITE_SEQ_ADDR, WRITE_SEQ_ADDR,
};

use std::fmt;
use stub::decode_b;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapError {
    ForeignHook {
        found: u32,
    },
    UnexpectedWord {
        addr: u32,
        found: u32,
        expected: u32,
    },
    CaveOccupied,
    RingOccupied,
    NotInstalled,
    Memory(String),
}

impl fmt::Display for TapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignHook { found } => write!(
                f,
                "hook 0x{HOOK_ADDR:08X} is 0x{found:08X}, not the original load; refusing to patch"
            ),
            Self::UnexpectedWord {
                addr,
                found,
                expected,
            } => write!(
                f,
                "word at 0x{addr:08X} is 0x{found:08X}, expected 0x{expected:08X}"
            ),
            Self::CaveOccupied => write!(f, "code cave at 0x{CAVE_ADDR:08X} is not empty"),
            Self::RingOccupied => {
                write!(f, "wide tap ring at 0x{RING_ADDR:08X} is not empty")
            }
            Self::NotInstalled => write!(f, "damage tap is not installed"),
            Self::Memory(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for TapError {}

pub trait PatchMemory {
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), TapError>;
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), TapError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    AlreadyInstalled,
}

/// Geometry of the ring the installed stub writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingLayout {
    pub entry_size: u32,
    pub capacity: u32,
    pub write_seq: u32,
}

impl RingLayout {
    pub fn standard() -> Self {
        Self {
            entry_size: ENTRY_SIZE,
            capacity: CAPACITY,
            write_seq: WRITE_SEQ_ADDR,
        }
    }

    pub fn wide() -> Self {
        Self {
            entry_size: WIDE_ENTRY_SIZE,
            capacity: WIDE_CAPACITY,
            write_seq: WIDE_WRITE_SEQ_ADDR,
        }
    }

    /// `MHDN_TAP_WIDE=1` selects the research ring. Anything else keeps the default stub.
    pub fn selected() -> Self {
        if wide_requested() {
            Self::wide()
        } else {
            Self::standard()
        }
    }
}

pub fn wide_requested() -> bool {
    std::env::var_os("MHDN_TAP_WIDE").is_some_and(|value| value == "1")
}

fn read_u32(mem: &mut dyn PatchMemory, addr: u32) -> Result<u32, TapError> {
    let mut buf = [0u8; 4];
    mem.read(addr, &mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

/// Write the stub, then the hook branch. Refuses a foreign instruction at the hook.
pub fn install(mem: &mut dyn PatchMemory) -> Result<InstallOutcome, TapError> {
    let hook = read_u32(mem, HOOK_ADDR)?;
    let branch = hook_branch();
    debug_assert_eq!(decode_b(HOOK_ADDR, branch), Some(CAVE_ADDR));
    if hook == branch {
        return Ok(InstallOutcome::AlreadyInstalled);
    }
    if hook != EXPECTED_HOOK {
        return Err(TapError::ForeignHook { found: hook });
    }
    let next = read_u32(mem, RETURN_ADDR)?;
    if next != EXPECTED_NEXT {
        return Err(TapError::UnexpectedWord {
            addr: RETURN_ADDR,
            found: next,
            expected: EXPECTED_NEXT,
        });
    }
    let store = read_u32(mem, 0x008D_03FC)?;
    if store != EXPECTED_HP_STORE {
        return Err(TapError::UnexpectedWord {
            addr: 0x008D_03FC,
            found: store,
            expected: EXPECTED_HP_STORE,
        });
    }
    let bytes = stub_bytes();
    let mut cave = vec![0u8; bytes.len()];
    mem.read(CAVE_ADDR, &mut cave)?;
    if cave != bytes && cave.iter().any(|b| *b != 0) {
        return Err(TapError::CaveOccupied);
    }
    if cave != bytes {
        mem.write(CAVE_ADDR, &bytes)?;
    }
    mem.write(HOOK_ADDR, &branch.to_le_bytes())?;
    Ok(InstallOutcome::Installed)
}

/// Wide research stub. Refuses unless the documented 256-byte ring page is still zero.
pub fn install_wide(mem: &mut dyn PatchMemory) -> Result<InstallOutcome, TapError> {
    let hook = read_u32(mem, HOOK_ADDR)?;
    let branch = hook_branch();
    debug_assert_eq!(decode_b(HOOK_ADDR, branch), Some(CAVE_ADDR));
    let bytes = wide_stub_bytes();
    if hook == branch {
        let mut cave = vec![0u8; bytes.len()];
        mem.read(CAVE_ADDR, &mut cave)?;
        return if cave == bytes {
            Ok(InstallOutcome::AlreadyInstalled)
        } else {
            Err(TapError::CaveOccupied)
        };
    }
    if hook != EXPECTED_HOOK {
        return Err(TapError::ForeignHook { found: hook });
    }
    let next = read_u32(mem, RETURN_ADDR)?;
    if next != EXPECTED_NEXT {
        return Err(TapError::UnexpectedWord {
            addr: RETURN_ADDR,
            found: next,
            expected: EXPECTED_NEXT,
        });
    }
    let store = read_u32(mem, 0x008D_03FC)?;
    if store != EXPECTED_HP_STORE {
        return Err(TapError::UnexpectedWord {
            addr: 0x008D_03FC,
            found: store,
            expected: EXPECTED_HP_STORE,
        });
    }
    let mut ring = vec![0u8; WIDE_RING_SPAN as usize];
    mem.read(RING_ADDR, &mut ring)?;
    if ring.iter().any(|byte| *byte != 0) {
        return Err(TapError::RingOccupied);
    }
    let mut cave = vec![0u8; bytes.len()];
    mem.read(CAVE_ADDR, &mut cave)?;
    if cave != bytes && cave.iter().any(|byte| *byte != 0) {
        return Err(TapError::CaveOccupied);
    }
    if cave != bytes {
        mem.write(CAVE_ADDR, &bytes)?;
    }
    mem.write(HOOK_ADDR, &branch.to_le_bytes())?;
    Ok(InstallOutcome::Installed)
}

/// Restore the original load. Leaves the cave in place; a title reboot clears both.
pub fn uninstall(mem: &mut dyn PatchMemory) -> Result<(), TapError> {
    let hook = read_u32(mem, HOOK_ADDR)?;
    if hook != hook_branch() {
        return Err(TapError::NotInstalled);
    }
    mem.write(HOOK_ADDR, &EXPECTED_HOOK.to_le_bytes())?;
    Ok(())
}

/// Entries whose `seq` is still the value published in `write_seq`'s ring window.
pub fn read_events(mem: &mut dyn PatchMemory) -> Result<Vec<TapEvent>, TapError> {
    read_events_in(mem, RingLayout::standard())
}

pub fn read_events_wide(mem: &mut dyn PatchMemory) -> Result<Vec<TapEvent>, TapError> {
    read_events_in(mem, RingLayout::wide())
}

fn read_events_in(
    mem: &mut dyn PatchMemory,
    layout: RingLayout,
) -> Result<Vec<TapEvent>, TapError> {
    let write_seq = read_u32(mem, layout.write_seq)?;
    if write_seq == 0 {
        return Ok(Vec::new());
    }
    let mut events = Vec::new();
    let start = write_seq.saturating_sub(layout.capacity);
    for seq in (start + 1)..=write_seq {
        let index = seq % layout.capacity;
        let addr = RING_ADDR + index * layout.entry_size;
        let mut buf = vec![0u8; layout.entry_size as usize];
        mem.read(addr, &mut buf)?;
        if let Some(event) = TapEvent::decode(&buf) {
            if event.seq == seq {
                events.push(event);
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapMem {
        words: HashMap<u32, u32>,
        bytes: HashMap<u32, Vec<u8>>,
    }

    impl MapMem {
        fn fresh() -> Self {
            let mut words = HashMap::new();
            words.insert(HOOK_ADDR, EXPECTED_HOOK);
            words.insert(RETURN_ADDR, EXPECTED_NEXT);
            words.insert(0x008D_03FC, EXPECTED_HP_STORE);
            Self {
                words,
                bytes: HashMap::new(),
            }
        }
    }

    impl PatchMemory for MapMem {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), TapError> {
            if let Some(block) = self.bytes.get(&addr) {
                let n = buf.len().min(block.len());
                buf[..n].copy_from_slice(&block[..n]);
                return Ok(());
            }
            for (base, block) in &self.bytes {
                if addr >= *base {
                    let off = (addr - base) as usize;
                    if off < block.len() {
                        let n = buf.len().min(block.len() - off);
                        buf[..n].copy_from_slice(&block[off..off + n]);
                        return Ok(());
                    }
                }
            }
            for (i, chunk) in buf.chunks_mut(4).enumerate() {
                let word = self.words.get(&(addr + i as u32 * 4)).copied().unwrap_or(0);
                let raw = word.to_le_bytes();
                chunk.copy_from_slice(&raw[..chunk.len()]);
            }
            Ok(())
        }

        fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), TapError> {
            if data.len() == 4 {
                let word = u32::from_le_bytes(data.try_into().expect("4"));
                self.words.insert(addr, word);
            }
            if let Some((base, block)) = self.bytes.iter_mut().find(|(base, block)| {
                addr >= **base && (addr as usize) < (**base as usize) + block.len()
                    || addr == base.wrapping_add(block.len() as u32)
            }) {
                let off = (addr - *base) as usize;
                let end = off + data.len();
                if end > block.len() {
                    block.resize(end, 0);
                }
                block[off..end].copy_from_slice(data);
                return Ok(());
            }
            self.bytes.insert(addr, data.to_vec());
            Ok(())
        }
    }

    #[test]
    fn install_refuses_a_foreign_hook() {
        let mut mem = MapMem::fresh();
        mem.words.insert(HOOK_ADDR, 0xEA0C_8AFE);
        assert!(matches!(
            install(&mut mem),
            Err(TapError::ForeignHook { found: 0xEA0C_8AFE })
        ));
    }

    #[test]
    fn install_writes_the_stub_before_the_branch() {
        let mut mem = MapMem::fresh();
        assert_eq!(install(&mut mem).unwrap(), InstallOutcome::Installed);
        assert_eq!(mem.words.get(&HOOK_ADDR).copied(), Some(hook_branch()));
        let bytes = stub_bytes();
        let block = mem.bytes.get(&CAVE_ADDR).expect("cave written");
        assert_eq!(block, &bytes);
        assert_eq!(install(&mut mem).unwrap(), InstallOutcome::AlreadyInstalled);
        uninstall(&mut mem).unwrap();
        assert_eq!(mem.words.get(&HOOK_ADDR).copied(), Some(EXPECTED_HOOK));
    }

    #[test]
    fn install_refuses_a_dirty_cave() {
        let mut mem = MapMem::fresh();
        mem.bytes.insert(CAVE_ADDR, vec![0xFF; 32]);
        assert!(matches!(install(&mut mem), Err(TapError::CaveOccupied)));
        assert_eq!(mem.words.get(&HOOK_ADDR).copied(), Some(EXPECTED_HOOK));
    }

    #[test]
    fn default_install_ignores_a_dirty_ring() {
        let mut mem = MapMem::fresh();
        mem.bytes.insert(RING_ADDR, vec![1, 2, 3, 4]);
        assert_eq!(install(&mut mem).unwrap(), InstallOutcome::Installed);
        assert_eq!(
            mem.bytes.get(&CAVE_ADDR).map(Vec::as_slice),
            Some(stub_bytes().as_slice())
        );
    }

    #[test]
    fn wide_install_refuses_a_dirty_ring_and_leaves_the_hook() {
        let mut mem = MapMem::fresh();
        mem.bytes.insert(RING_ADDR, vec![0, 0, 0, 1]);
        assert!(matches!(
            install_wide(&mut mem),
            Err(TapError::RingOccupied)
        ));
        assert_eq!(mem.words.get(&HOOK_ADDR).copied(), Some(EXPECTED_HOOK));
        assert!(!mem.bytes.contains_key(&CAVE_ADDR));
    }

    #[test]
    fn wide_install_writes_the_wide_stub_into_a_zero_ring() {
        let mut mem = MapMem::fresh();
        assert_eq!(install_wide(&mut mem).unwrap(), InstallOutcome::Installed);
        assert_eq!(
            mem.bytes.get(&CAVE_ADDR).map(Vec::as_slice),
            Some(wide_stub_bytes().as_slice())
        );
        assert_ne!(wide_stub_bytes(), stub_bytes());
        assert_eq!(
            install_wide(&mut mem).unwrap(),
            InstallOutcome::AlreadyInstalled
        );
    }
}
