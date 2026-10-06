//! Game model: profiles, snapshots, and damage events.

#![forbid(unsafe_code)]

mod attacker;
mod chain;
mod damage;
mod model;
mod pipeline;
mod plan;
mod profile;
mod sampler;
mod scene;
mod snapshot;
#[cfg(test)]
mod sparse;
mod tap;
mod track;

#[cfg(test)]
mod support;

pub use attacker::{Attacker, AttackerFilter, DEFAULT_ATTACKER_FILTER_ATTACKERS};
pub use chain::{ChainError, ChainStep, PointerCache};
pub use damage::{
    events_from_tap, overkill, tap_sum_for, DamageConfidence, DamageEvent, DamageKind, DmgDrop,
    EventSource, TapCredit, UnmatchedTap,
};
pub use model::{
    Anchor, CameraState, MonsterKey, MonsterState, Snapshot, Vec3, HP_FROM_OBJECT, LARGE_MIN_HP,
};
pub use pipeline::{Pipeline, Sample};
pub use profile::{
    load_dir, select, CameraMode, CameraProfile, ChainSpec, DamageTap, ExpectedWord, FieldRef,
    FieldSpec, FieldType, FingerprintWindow, FovUnit, MatchKind, MemorySpan, MonsterLayout,
    MonsterList, ObservedWindow, Profile, ProfileError, SceneFlag, Selection, SpeciesProfile,
    TapRing, MHXX_JP_TITLE_ID,
};
pub use sampler::{sample_period, spawn, EventQueue, Latest, Pump, Sampler, SamplerJoin};
pub use scene::Scene;
pub use snapshot::{ReadReject, RejectedRead};
pub use tap::{
    install as tap_install, install_wide as tap_install_wide, read_events as tap_read_events,
    read_events_wide as tap_read_events_wide, uninstall as tap_uninstall, wide_requested,
    InstallOutcome, PatchMemory, RingLayout, TapError, TapEvent, CALLER_HIT, CALLER_MOUNT_TOPPLE,
    CALLER_STATUS, CAVE_ADDR, ENTRY_SIZE, EXPECTED_HOOK, EXPECTED_HP_STORE, EXPECTED_NEXT,
    HOOK_ADDR, RETURN_ADDR, RING_ADDR, WRITE_SEQ_ADDR,
};
