//! Game model: profiles, snapshots, and damage events.

#![forbid(unsafe_code)]

mod profile;

pub use profile::{
    load_dir, select, CameraMode, CameraProfile, ChainSpec, FieldRef, FieldSpec, FieldType,
    FingerprintWindow, FovUnit, MatchKind, MonsterLayout, MonsterList, ObservedWindow, Profile,
    ProfileError, Selection, SpeciesProfile, MHXX_JP_TITLE_ID,
};
