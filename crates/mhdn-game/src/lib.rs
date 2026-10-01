//! Game model: profiles, snapshots, and damage events.

#![forbid(unsafe_code)]

mod chain;
mod damage;
mod profile;
mod tap;

pub use chain::{ChainError, ChainStep, PointerCache};
pub use damage::{
    events_from_tap, overkill, tap_sum_for, DamageConfidence, DamageEvent, DamageKind, EventSource,
};
pub use profile::{
    load_dir, select, CameraMode, CameraProfile, ChainSpec, DamageTap, ExpectedWord, FieldRef,
    FieldSpec, FieldType, FingerprintWindow, FovUnit, MatchKind, MemorySpan, MonsterLayout,
    MonsterList, ObservedWindow, Profile, ProfileError, SceneFlag, Selection, SpeciesProfile, TapRing,
    MHXX_JP_TITLE_ID,
};
pub use tap::{
    install as tap_install, read_events as tap_read_events, uninstall as tap_uninstall,
    InstallOutcome, PatchMemory, TapError, TapEvent, CAVE_ADDR, ENTRY_SIZE, EXPECTED_HOOK,
    EXPECTED_HP_STORE, EXPECTED_NEXT, HOOK_ADDR, RETURN_ADDR, RING_ADDR, WRITE_SEQ_ADDR,
};
