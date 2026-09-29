//! Game model: profiles, snapshots, and damage events.

#![forbid(unsafe_code)]

mod profile;
mod tap;

pub use profile::{
    load_dir, select, CameraMode, CameraProfile, ChainSpec, FieldRef, FieldSpec, FieldType,
    FingerprintWindow, FovUnit, MatchKind, MonsterLayout, MonsterList, ObservedWindow, Profile,
    ProfileError, Selection, SpeciesProfile, MHXX_JP_TITLE_ID,
};
pub use tap::{
    install as tap_install, read_events as tap_read_events, uninstall as tap_uninstall,
    InstallOutcome, PatchMemory, TapError, TapEvent,
};
