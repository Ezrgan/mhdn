//! Return addresses recorded in `lr` at the damage tap (`0x008D03E8`).
//!
//! Each value is the instruction after `bl 0x008D03E4` in that caller.
//! Live evidence is in `docs/RE_NOTES.md` ("Tap callers").

/// Normal hits. `sp[3]` is the raw damage.
pub const CALLER_HIT: u32 = 0x008B_A260;

/// Fixed damage when a mounted monster is toppled: 150 in high rank, 100 in low rank.
pub const CALLER_MOUNT_TOPPLE: u32 = 0x008B_A870;

/// Status damage, including poison ticks.
pub const CALLER_STATUS: u32 = 0x008B_A214;
