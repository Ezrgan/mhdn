//! Offline guest images for the pipeline tests.

use crate::profile::Profile;
use crate::sparse::SparseMemory;

pub fn live_profile() -> Profile {
    Profile::from_toml_str(include_str!("../../../profiles/mhxx-jp-v1.4-es.toml"))
        .expect("live profile")
}

pub fn stage_hunt(
    mem: &mut SparseMemory,
    profile: &Profile,
    frame: u32,
    in_quest: bool,
    loading: bool,
) {
    let frame_addr = profile.frame_counter.addr.expect("frame counter");
    mem.write_u32(frame_addr, frame);
    let quest = profile.scene.in_quest.as_ref().expect("in_quest");
    mem.write_u32(quest.addr, if in_quest { quest.value } else { 0 });
    let load = profile.scene.loading.as_ref().expect("loading");
    mem.write_u8(load.addr, if loading { load.value as u8 } else { 0 });
    place_hunter_and_camera(mem);
}

const LIST_BASE: u32 = 0x082C_E730;

pub fn place_monster(
    mem: &mut SparseMemory,
    slot: u32,
    hp: u32,
    max_hp: u32,
    species: u16,
    pos: [f32; 3],
    poison: u16,
) {
    mem.write_u32(0x00D3_A8E0, LIST_BASE);
    mem.write_u32(0x00D2_CAA0, 0);
    mem.write_u32(0x00D3_0AA0, 0);
    let component = 0x0800_2000 + slot * 0x1000;
    let object = 0x3006_0000 + slot * 0x1_0000;
    let slot_addr = LIST_BASE + 0x14 + slot * 4;
    mem.write_u32(slot_addr, component);
    mem.write_u32(component + 0x10A8, object);
    let hp_addr = object + 0x360;
    mem.write_u32(hp_addr, hp);
    mem.write_u32(hp_addr + 4, max_hp);
    mem.write_u16(hp_addr + 0x5A18, species);
    mem.write_vec3(hp_addr.wrapping_sub(800), pos);
    mem.write_f32(hp_addr.wrapping_sub(432), 1.0);
    mem.write_u16(hp_addr + 0x54E4, poison);
    mem.write_u8(hp_addr.wrapping_sub(5128), 0);
}

pub fn hp_addr(slot: u32) -> u32 {
    let object = 0x3006_0000 + slot * 0x1_0000;
    object + 0x360
}

fn place_hunter_and_camera(mem: &mut SparseMemory) {
    mem.write_u32(0x0814_E620, 0x3004_0000);
    mem.write_vec3(0x3004_0040, [10.0, 0.0, 20.0]);
    mem.write_u32(0x0814_CACC, 0x3003_B3D0);
    mem.write_f32(0x3003_B40C, 50.0);
    mem.write_vec3(0x3003_B410, [1.0, 2.0, 3.0]);
    mem.write_vec3(0x3003_B430, [4.0, 5.0, 6.0]);
}
