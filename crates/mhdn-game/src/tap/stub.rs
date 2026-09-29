//! Assembled ARM stub for the damage tap (plan 2.20).
//!
//! The hook site is `0x008D03E8` (`ldr r12, [r3, #0xA8]`). `r1` is the signed HP
//! delta (negative for damage), `r3` is the component that owns the monster
//! pointer, and `lr` is the caller's return. The stub runs that load, appends
//! one 40-byte ring entry, publishes `write_seq`, and branches to `0x008D03EC`.
//! `r2` is borrowed and restored: the very next instruction compares it.

/// `ldr r12, [r3, #0xA8]`
pub const EXPECTED_HOOK: u32 = 0xE593_C0A8;
/// `ldr r0, [r12, #0x360]`
pub const EXPECTED_NEXT: u32 = 0xE59C_0360;
/// `str r0, [r12, #0x360]`
pub const EXPECTED_HP_STORE: u32 = 0xE58C_0360;

pub const HOOK_ADDR: u32 = 0x008D_03E8;
pub const RETURN_ADDR: u32 = 0x008D_03EC;
pub const CAVE_ADDR: u32 = 0x00BF_2D00;

pub const RING_ADDR: u32 = 0x00D3_2000;
pub const ENTRY_SIZE: u32 = 40;
pub const CAPACITY: u32 = 64;
pub const WRITE_SEQ_ADDR: u32 = RING_ADDR + ENTRY_SIZE * CAPACITY;
pub const LOCAL_SEQ_ADDR: u32 = WRITE_SEQ_ADDR + 4;
pub const SCRATCH_ADDR: u32 = WRITE_SEQ_ADDR + 8;

const POOL_SCRATCH: u32 = 38;
const POOL_SEQ: u32 = 39;
const POOL_RING: u32 = 40;
const POOL_WRITE_SEQ: u32 = 41;

/// One ring slot: `seq`, `r1`, `r12`, `r3`, `lr`, then five words from `sp` at the hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapEvent {
    pub seq: u32,
    pub r1: i32,
    pub monster: u32,
    pub r3: u32,
    pub lr: u32,
    pub stack: [u32; 5],
}

impl TapEvent {
    /// `r1` is the signed HP delta, so a hit of 8 is stored as `-8`.
    pub fn damage(self) -> i32 {
        self.r1.wrapping_neg()
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < ENTRY_SIZE as usize {
            return None;
        }
        let word =
            |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
        let seq = word(0);
        if seq == 0 {
            return None;
        }
        Some(Self {
            seq,
            r1: word(1) as i32,
            monster: word(2),
            r3: word(3),
            lr: word(4),
            stack: [word(5), word(6), word(7), word(8), word(9)],
        })
    }
}

/// Unconditional ARM `B` from `from` to `to`.
pub fn arm_b(from: u32, to: u32) -> u32 {
    let imm = to.wrapping_sub(from.wrapping_add(8)) as i32 >> 2;
    0xEA00_0000 | (imm as u32 & 0x00FF_FFFF)
}

/// Target of an unconditional `B` encoded at `from`, if `word` is one.
pub fn decode_b(from: u32, word: u32) -> Option<u32> {
    if word >> 24 != 0xEA {
        return None;
    }
    let imm = word & 0x00FF_FFFF;
    let signed = if imm & 0x0080_0000 != 0 {
        imm | 0xFF00_0000
    } else {
        imm
    } as i32;
    Some(
        from.wrapping_add(8)
            .wrapping_add((signed as u32).wrapping_mul(4)),
    )
}

/// Branch written at [`HOOK_ADDR`]. One aligned store, after the cave is filled.
pub fn hook_branch() -> u32 {
    arm_b(HOOK_ADDR, CAVE_ADDR)
}

/// Words of the cave stub, including the literal pool.
pub fn stub_words() -> Vec<u32> {
    let ldr_pc = |rt: u32, index: u32, pool: u32| {
        let from = CAVE_ADDR + index * 4;
        let literal = CAVE_ADDR + pool * 4;
        let imm = literal.wrapping_sub(from.wrapping_add(8));
        debug_assert!(imm < 0x1000);
        0xE590_0000 | (15 << 16) | (rt << 12) | imm
    };
    let str_imm = |rt: u32, rn: u32, imm: u32| 0xE580_0000 | (rn << 16) | (rt << 12) | imm;
    let ldr_imm = |rt: u32, rn: u32, imm: u32| 0xE590_0000 | (rn << 16) | (rt << 12) | imm;

    let mut words = vec![
        EXPECTED_HOOK,                 // 0  ldr r12, [r3, #0xA8]
        ldr_pc(0, 1, POOL_SCRATCH),    // 1  ldr r0, scratch
        str_imm(2, 0, 0),              // 2  str r2, [r0]
        ldr_pc(2, 3, POOL_SEQ),        // 3  ldr r2, local_seq
        ldr_imm(0, 2, 0),              // 4  ldr r0, [r2]
        0xE280_0001,                   // 5  add r0, r0, #1
        str_imm(0, 2, 0),              // 6  str r0, [r2]
        0xE200_003F,                   // 7  and r0, r0, #63
        0xE080_0100,                   // 8  add r0, r0, r0, lsl #2
        0xE1A0_0180,                   // 9  lsl r0, r0, #3   (index * 40)
        ldr_pc(2, 10, POOL_RING),      // 10 ldr r2, ring
        0xE082_2000,                   // 11 add r2, r2, r0
        ldr_pc(0, 12, POOL_SEQ),       // 12 ldr r0, local_seq
        ldr_imm(0, 0, 0),              // 13 ldr r0, [r0]
        str_imm(0, 2, 0),              // 14 str r0, [r2]       seq
        str_imm(1, 2, 4),              // 15 str r1, [r2, #4]
        str_imm(12, 2, 8),             // 16 str r12, [r2, #8]
        str_imm(3, 2, 12),             // 17 str r3, [r2, #12]
        str_imm(14, 2, 16),            // 18 str lr, [r2, #16]
        ldr_imm(0, 13, 0),             // 19 ldr r0, [sp]
        str_imm(0, 2, 20),             // 20
        ldr_imm(0, 13, 4),             // 21 ldr r0, [sp, #4]
        str_imm(0, 2, 24),             // 22
        ldr_imm(0, 13, 8),             // 23
        str_imm(0, 2, 28),             // 24
        ldr_imm(0, 13, 12),            // 25
        str_imm(0, 2, 32),             // 26
        ldr_imm(0, 13, 16),            // 27
        str_imm(0, 2, 36),             // 28
        0xEE07_0FBA,                   // 29 mcr p15, 0, r0, c7, c10, 5
        ldr_pc(0, 30, POOL_SEQ),       // 30
        ldr_imm(0, 0, 0),              // 31 ldr r0, [r0]
        ldr_pc(2, 32, POOL_WRITE_SEQ), // 32
        str_imm(0, 2, 0),              // 33 str r0, [r2]  write_seq
        0xEE07_0FBA,                   // 34 barrier after the publish
        ldr_pc(0, 35, POOL_SCRATCH),   // 35
        ldr_imm(2, 0, 0),              // 36 ldr r2, [r0]
        0,                             // 37 b RETURN, filled below
    ];
    let branch_index = 37u32;
    let from = CAVE_ADDR + branch_index * 4;
    words[branch_index as usize] = arm_b(from, RETURN_ADDR);
    words.extend_from_slice(&[SCRATCH_ADDR, LOCAL_SEQ_ADDR, RING_ADDR, WRITE_SEQ_ADDR]);
    debug_assert_eq!(words.len(), 42);
    words
}

pub fn stub_bytes() -> Vec<u8> {
    stub_words()
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_arm_words() {
        assert_eq!(EXPECTED_HOOK, 0xE593_C0A8);
        assert_eq!(arm_b(0x008D_03E8, 0x00BF_2D00), hook_branch());
        assert_eq!(decode_b(HOOK_ADDR, hook_branch()), Some(CAVE_ADDR));
    }

    #[test]
    fn stub_branches_back_and_keeps_the_original_load() {
        let words = stub_words();
        assert_eq!(words[0], EXPECTED_HOOK);
        let from = CAVE_ADDR + 37 * 4;
        assert_eq!(decode_b(from, words[37]), Some(RETURN_ADDR));
        assert!(words.len() * 4 < 0x300, "stub must fit the cave");
        assert_eq!(words[38], SCRATCH_ADDR);
        assert_eq!(words[41], WRITE_SEQ_ADDR);
    }

    #[test]
    fn event_damage_is_negated_r1() {
        let mut raw = [0u8; 40];
        raw[0..4].copy_from_slice(&1u32.to_le_bytes());
        raw[4..8].copy_from_slice(&(-8i32 as u32).to_le_bytes());
        let event = TapEvent::decode(&raw).unwrap();
        assert_eq!(event.damage(), 8);
        assert!(TapEvent::decode(&[0u8; 40]).is_none());
    }
}
