//! Snapshot types shared by the sampler and the damage pipeline.

use crate::profile::FovUnit;
use crate::scene::Scene;

/// A large monster in the live notes starts at 720 HP. Bars under this are small monsters.
pub const LARGE_MIN_HP: u32 = 400;

/// `r12 + 0x360` is the HP word the monster list resolves to.
pub const HP_FROM_OBJECT: u32 = 0x360;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn from_array(value: [f32; 3]) -> Self {
        Self::new(value[0], value[1], value[2])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonsterKey {
    pub struct_addr: u32,
    pub species: u16,
    pub max_hp: u32,
    pub generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_y: f32,
    pub fov_unit: FovUnit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    Unknown,
    World(Vec3),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonsterState {
    pub key: MonsterKey,
    pub hp: u32,
    pub pos: Vec3,
    pub visible: bool,
    pub large: bool,
    pub poisoned: bool,
    /// Index in the monster-list slot table. Empty slots are omitted, so this is not the vec index.
    pub slot: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub guest_frame: u32,
    pub host_us: u64,
    pub scene: Scene,
    pub camera: Option<CameraState>,
    pub hunter_pos: Option<Vec3>,
    pub monsters: Vec<MonsterState>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawMonster {
    pub hp_addr: u32,
    pub slot_ptr: u32,
    pub hp: u32,
    pub max_hp: u32,
    pub species: u16,
    pub pos: Vec3,
    pub visible: bool,
    pub large: bool,
    pub poisoned: bool,
    pub slot: u32,
}
