//! Consistent snapshots. The guest frame is read before and after the body; a mismatch is torn.

use mhdn_rpc::MemorySource;

use crate::chain::{is_guest_heap, ChainStep, PointerCache};
use crate::model::{CameraState, RawMonster, Vec3, LARGE_MIN_HP, MAX_PLAUSIBLE_HP};
use crate::plan::{coalesce, MAX_BLOCK, MAX_GAP};
use crate::profile::{FieldRef, FieldSpec, FieldType, Profile};

const COLD_EVERY: u32 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    Read { addr: u32 },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { addr } => write!(formatter, "guest read failed at 0x{addr:08X}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// A slot that resolved, but whose HP cannot belong to a live monster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedRead {
    pub hp_addr: u32,
    pub species: u16,
    pub hp: u32,
    pub max_hp: u32,
    pub reason: ReadReject,
}

impl RejectedRead {
    pub fn diag_line(&self) -> String {
        format!(
            "hp_bad addr=0x{:08X} species={} hp={} max_hp={} reason={}",
            self.hp_addr, self.species, self.hp, self.max_hp, self.reason
        )
    }
}

/// Why a resolved slot was left out of the sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadReject {
    HpAboveMax,
    MaxHpRange,
    Species,
}

impl ReadReject {
    fn as_str(self) -> &'static str {
        match self {
            Self::HpAboveMax => "hp_above_max",
            Self::MaxHpRange => "max_hp_range",
            Self::Species => "species",
        }
    }
}

impl std::fmt::Display for ReadReject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
struct ColdFields {
    hp_addr: u32,
    hp: u32,
    species: u16,
    poisoned: bool,
    visible: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CaptureCache {
    pointers: PointerCache,
    /// Static address of the monster-list pointer that matched `expect`.
    list_slot: Option<u32>,
    cold: Vec<ColdFields>,
    polls: u32,
}

impl CaptureCache {
    pub(crate) fn clear(&mut self) {
        self.pointers.invalidate();
        self.list_slot = None;
        self.cold.clear();
        self.polls = 0;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RawSnapshot {
    pub guest_frame: u32,
    pub loading: bool,
    pub camera: Option<CameraState>,
    pub hunter_pos: Option<Vec3>,
    /// Word at `profile.hunter.base` (`0x0814E620`). `None` when that read fails.
    pub hunter_slot: Option<u32>,
    pub monsters: Vec<RawMonster>,
    pub rejected: Vec<RejectedRead>,
}

pub(crate) fn capture(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    cache: &mut CaptureCache,
) -> Result<Option<RawSnapshot>, SnapshotError> {
    for _ in 0..3 {
        let mut scratch = cache.clone();
        let frame = read_u32(mem, profile.frame_counter.addr)?;
        let body = read_body(mem, profile, &mut scratch, frame)?;
        let again = read_u32(mem, profile.frame_counter.addr)?;
        if frame == again {
            scratch.polls = scratch.polls.wrapping_add(1);
            *cache = scratch;
            return Ok(Some(body));
        }
    }
    Ok(None)
}

fn read_body(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    cache: &mut CaptureCache,
    guest_frame: u32,
) -> Result<RawSnapshot, SnapshotError> {
    let loading = match &profile.scene.loading {
        Some(flag) => read_flag(mem, flag.addr, flag.ty)? == flag.value,
        None => false,
    };
    let (monsters, rejected) = read_monsters(mem, profile, cache, guest_frame)?;
    let (hunter_pos, hunter_slot) = read_hunter(mem, profile)?;
    Ok(RawSnapshot {
        guest_frame,
        loading,
        camera: read_camera(mem, profile)?,
        hunter_pos,
        hunter_slot,
        monsters,
        rejected,
    })
}

fn read_monsters(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    cache: &mut CaptureCache,
    _guest_frame: u32,
) -> Result<(Vec<RawMonster>, Vec<RejectedRead>), SnapshotError> {
    let Some(list_base) = resolve_list(mem, profile, cache)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let slots = profile.monster_list.slots.min(16);
    let stride = profile.monster_list.slot_stride;
    let table = add_offset(list_base, profile.monster_list.slot_offset);
    let Some(table) = table else {
        return Ok((Vec::new(), Vec::new()));
    };
    let table_len = slots.saturating_mul(stride);
    let mut table_bytes = vec![0u8; table_len as usize];
    read_exact(mem, table, &mut table_bytes)?;

    let crate::profile::ChainSpec::Offsets(offsets) = &profile.monster_list.chain else {
        return Ok((Vec::new(), Vec::new()));
    };
    if offsets.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let steps = chain_steps(offsets);
    let mut monsters = Vec::new();
    let mut rejected = Vec::new();
    for slot in 0..slots {
        let at = (slot * stride) as usize;
        if at + 4 > table_bytes.len() {
            break;
        }
        let slot_ptr = u32::from_le_bytes(table_bytes[at..at + 4].try_into().expect("4 bytes"));
        if slot_ptr == 0 || !is_guest_heap(slot_ptr) {
            continue;
        }
        let hp_addr = match cache.pointers.resolve(mem, slot_ptr, &steps) {
            Ok(addr) => addr,
            Err(_) => {
                cache.pointers.forget(slot_ptr, &steps);
                continue;
            }
        };
        if let Some(monster) =
            read_monster(mem, profile, cache, slot, slot_ptr, hp_addr, &mut rejected)?
        {
            monsters.push(monster);
        } else {
            cache.pointers.forget(slot_ptr, &steps);
        }
    }
    Ok((monsters, rejected))
}

fn read_monster(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    cache: &mut CaptureCache,
    slot: u32,
    slot_ptr: u32,
    hp_addr: u32,
    rejected: &mut Vec<RejectedRead>,
) -> Result<Option<RawMonster>, SnapshotError> {
    let Some(hp_spec) = relative(&profile.monster.hp) else {
        return Ok(None);
    };
    let Some(max_spec) = relative(&profile.monster.max_hp) else {
        return Ok(None);
    };
    let mut hot = vec![
        field_range(hp_addr, hp_spec),
        field_range(hp_addr, max_spec),
    ];
    if let Some(spec) = relative(&profile.monster.pos) {
        hot.push(field_range(hp_addr, spec));
    }
    if let Some(spec) = relative(&profile.monster.size) {
        hot.push(field_range(hp_addr, spec));
    }
    let hot: Vec<(u32, u32)> = hot.into_iter().flatten().collect();
    let blocks = read_blocks(mem, &hot)?;
    let Some(hp) = parse_u32(&blocks, hp_addr, hp_spec) else {
        return Ok(None);
    };
    let Some(max_hp) = parse_u32(&blocks, hp_addr, max_spec) else {
        return Ok(None);
    };
    // Zero max HP is an empty slot. Any other value outside 1..=MAX_PLAUSIBLE_HP is garbage.
    if max_hp == 0 {
        return Ok(None);
    }
    if max_hp > MAX_PLAUSIBLE_HP || hp > max_hp {
        let reason = if max_hp > MAX_PLAUSIBLE_HP {
            ReadReject::MaxHpRange
        } else {
            ReadReject::HpAboveMax
        };
        rejected.push(rejected_read(mem, profile, hp_addr, hp, max_hp, reason)?);
        return Ok(None);
    }
    if profile.species.has_species_table() {
        let species = read_species(mem, profile, hp_addr)?;
        if !profile.species.knows_species(species) {
            rejected.push(RejectedRead {
                hp_addr,
                species,
                hp,
                max_hp,
                reason: ReadReject::Species,
            });
            return Ok(None);
        }
    }
    let pos = match relative(&profile.monster.pos) {
        Some(spec) => parse_vec3(&blocks, hp_addr, spec).unwrap_or(Vec3::new(0.0, 0.0, 0.0)),
        None => Vec3::new(0.0, 0.0, 0.0),
    };
    let refresh = cache.polls.is_multiple_of(COLD_EVERY)
        || cache
            .cold
            .iter()
            .find(|cold| cold.hp_addr == hp_addr)
            .is_none_or(|cold| cold.hp != hp);
    let (species, poisoned, visible) = if refresh {
        let species = read_species(mem, profile, hp_addr)?;
        let poisoned = read_poison(mem, profile, hp_addr)?;
        let visible = read_visible(mem, profile, hp_addr, hp)?;
        if let Some(slot) = cache.cold.iter_mut().find(|cold| cold.hp_addr == hp_addr) {
            *slot = ColdFields {
                hp_addr,
                hp,
                species,
                poisoned,
                visible,
            };
        } else {
            cache.cold.push(ColdFields {
                hp_addr,
                hp,
                species,
                poisoned,
                visible,
            });
        }
        (species, poisoned, visible)
    } else {
        let cold = cache
            .cold
            .iter_mut()
            .find(|cold| cold.hp_addr == hp_addr)
            .expect("cold fields exist when refresh is false");
        cold.hp = hp;
        (cold.species, cold.poisoned, cold.visible)
    };
    Ok(Some(RawMonster {
        hp_addr,
        slot_ptr,
        hp,
        max_hp,
        species,
        pos,
        visible,
        large: max_hp >= LARGE_MIN_HP,
        poisoned,
        slot,
    }))
}

fn rejected_read(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    hp_addr: u32,
    hp: u32,
    max_hp: u32,
    reason: ReadReject,
) -> Result<RejectedRead, SnapshotError> {
    Ok(RejectedRead {
        hp_addr,
        species: read_species(mem, profile, hp_addr)?,
        hp,
        max_hp,
        reason,
    })
}

fn resolve_list(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    cache: &mut CaptureCache,
) -> Result<Option<u32>, SnapshotError> {
    if let Some(slot) = cache.list_slot {
        if let Ok(value) = mem.read_u32(slot) {
            let matches = profile
                .monster_list
                .base_candidates
                .iter()
                .any(|candidate| candidate.addr == slot && candidate.expect.contains(&value));
            if matches && is_guest_heap(value) {
                return Ok(Some(value));
            }
        }
        cache.list_slot = None;
    }
    for candidate in &profile.monster_list.base_candidates {
        let Ok(value) = mem.read_u32(candidate.addr) else {
            continue;
        };
        if candidate.expect.contains(&value) && is_guest_heap(value) {
            cache.list_slot = Some(candidate.addr);
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// Feet, plus the raw word at the static hunter slot. One read of that address.
fn read_hunter(
    mem: &mut dyn MemorySource,
    profile: &Profile,
) -> Result<(Option<Vec3>, Option<u32>), SnapshotError> {
    let Some(base) = profile.hunter.base else {
        return Ok((None, None));
    };
    let Ok(word) = mem.read_u32(base) else {
        return Ok((None, None));
    };
    Ok((hunter_feet(mem, profile, word)?, Some(word)))
}

fn hunter_feet(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    object: u32,
) -> Result<Option<Vec3>, SnapshotError> {
    if !is_guest_heap(object) {
        return Ok(None);
    }
    let Some(spec) = relative(&profile.hunter.pos) else {
        return Ok(None);
    };
    let Some((addr, _)) = field_range(object, spec) else {
        return Ok(None);
    };
    let mut buf = [0u8; 12];
    read_exact(mem, addr, &mut buf)?;
    Ok(Some(vec3_from(&buf)))
}

fn read_camera(
    mem: &mut dyn MemorySource,
    profile: &Profile,
) -> Result<Option<CameraState>, SnapshotError> {
    let Some(base) = profile.camera.base else {
        return Ok(None);
    };
    let Ok(object) = mem.read_u32(base) else {
        return Ok(None);
    };
    if !is_guest_heap(object) {
        return Ok(None);
    }
    let eye_spec = match profile.camera.eye.as_ref() {
        Some(FieldRef::Relative(spec)) => spec,
        Some(FieldRef::Unresolved) | None => return Ok(None),
    };
    let target_spec = match profile.camera.target.as_ref() {
        Some(FieldRef::Relative(spec)) => spec,
        Some(FieldRef::Unresolved) | None => return Ok(None),
    };
    let fov_spec = match profile.camera.fov_y.as_ref() {
        Some(FieldRef::Relative(spec)) => spec,
        Some(FieldRef::Unresolved) | None => return Ok(None),
    };
    let ranges = [eye_spec, target_spec, fov_spec]
        .into_iter()
        .filter_map(|spec| field_range(object, spec))
        .collect::<Vec<_>>();
    let blocks = read_blocks(mem, &ranges)?;
    let Some(eye) = parse_vec3(&blocks, object, eye_spec) else {
        return Ok(None);
    };
    let Some(target) = parse_vec3(&blocks, object, target_spec) else {
        return Ok(None);
    };
    let Some(fov_y) = parse_f32(&blocks, object, fov_spec) else {
        return Ok(None);
    };
    Ok(Some(CameraState {
        eye,
        target,
        fov_y,
        fov_unit: profile.camera.fov_unit,
    }))
}

fn read_species(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    hp_addr: u32,
) -> Result<u16, SnapshotError> {
    let Some(spec) = relative(&profile.monster.species) else {
        return Ok(0);
    };
    let Some((addr, len)) = field_range(hp_addr, spec) else {
        return Ok(0);
    };
    let mut buf = vec![0u8; len as usize];
    read_exact(mem, addr, &mut buf)?;
    Ok(parse_small(&buf, spec.ty).unwrap_or(0) as u16)
}

fn read_poison(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    hp_addr: u32,
) -> Result<bool, SnapshotError> {
    let Some(spec) = relative(&profile.monster.poison) else {
        return Ok(false);
    };
    let Some((addr, len)) = field_range(hp_addr, spec) else {
        return Ok(false);
    };
    let mut buf = vec![0u8; len as usize];
    read_exact(mem, addr, &mut buf)?;
    Ok(parse_small(&buf, spec.ty).unwrap_or(0) != 0)
}

fn read_visible(
    mem: &mut dyn MemorySource,
    profile: &Profile,
    hp_addr: u32,
    hp: u32,
) -> Result<bool, SnapshotError> {
    let Some(spec) = relative(&profile.monster.visible_flag) else {
        return Ok(hp != 0);
    };
    let Some((addr, len)) = field_range(hp_addr, spec) else {
        return Ok(hp != 0);
    };
    let mut buf = vec![0u8; len as usize];
    read_exact(mem, addr, &mut buf)?;
    let value = parse_small(&buf, spec.ty).unwrap_or(0);
    Ok(spec.hidden_value != Some(value))
}

fn read_blocks(
    mem: &mut dyn MemorySource,
    ranges: &[(u32, u32)],
) -> Result<Vec<(u32, Vec<u8>)>, SnapshotError> {
    let mut blocks = Vec::new();
    for (addr, len) in coalesce(ranges.to_vec(), MAX_BLOCK, MAX_GAP) {
        let mut buf = vec![0u8; len as usize];
        read_exact(mem, addr, &mut buf)?;
        blocks.push((addr, buf));
    }
    Ok(blocks)
}

fn parse_u32(blocks: &[(u32, Vec<u8>)], base: u32, spec: &FieldSpec) -> Option<u32> {
    let (addr, _) = field_range(base, spec)?;
    let bytes = slice_at(blocks, addr, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn parse_f32(blocks: &[(u32, Vec<u8>)], base: u32, spec: &FieldSpec) -> Option<f32> {
    let (addr, _) = field_range(base, spec)?;
    let bytes = slice_at(blocks, addr, 4)?;
    Some(f32::from_le_bytes(bytes.try_into().ok()?))
}

fn parse_vec3(blocks: &[(u32, Vec<u8>)], base: u32, spec: &FieldSpec) -> Option<Vec3> {
    let (addr, _) = field_range(base, spec)?;
    let bytes = slice_at(blocks, addr, 12)?;
    Some(vec3_from(bytes))
}

fn slice_at(blocks: &[(u32, Vec<u8>)], addr: u32, len: usize) -> Option<&[u8]> {
    for (base, buf) in blocks {
        if addr < *base {
            continue;
        }
        let offset = (addr - base) as usize;
        let end = offset.checked_add(len)?;
        if end <= buf.len() {
            return Some(&buf[offset..end]);
        }
    }
    None
}

fn vec3_from(bytes: &[u8]) -> Vec3 {
    let word =
        |index: usize| f32::from_le_bytes(bytes[index..index + 4].try_into().expect("4 bytes"));
    Vec3::new(word(0), word(4), word(8))
}

fn parse_small(bytes: &[u8], ty: FieldType) -> Option<u32> {
    match ty {
        FieldType::U8 => bytes.first().copied().map(u32::from),
        FieldType::U16 => {
            let pair = bytes.get(..2)?;
            Some(u32::from(u16::from_le_bytes(pair.try_into().ok()?)))
        }
        FieldType::U32 | FieldType::F32 => {
            let word = bytes.get(..4)?;
            Some(u32::from_le_bytes(word.try_into().ok()?))
        }
        FieldType::Vec3 => None,
    }
}

fn field_range(base: u32, spec: &FieldSpec) -> Option<(u32, u32)> {
    let addr = add_offset(base, spec.off)?;
    let len = match spec.ty {
        FieldType::U8 => 1,
        FieldType::U16 => 2,
        FieldType::U32 | FieldType::F32 => 4,
        FieldType::Vec3 => 12,
    };
    Some((addr, len))
}

fn relative(field: &FieldRef) -> Option<&FieldSpec> {
    match field {
        FieldRef::Relative(spec) => Some(spec),
        FieldRef::Unresolved => None,
    }
}

fn chain_steps(offsets: &[i32]) -> Vec<ChainStep> {
    let last = offsets.len() - 1;
    offsets
        .iter()
        .enumerate()
        .map(|(index, offset)| ChainStep {
            offset: *offset,
            deref: index != last,
        })
        .collect()
}

fn add_offset(addr: u32, offset: i32) -> Option<u32> {
    if offset >= 0 {
        addr.checked_add(offset as u32)
    } else {
        addr.checked_sub(offset.unsigned_abs())
    }
}

fn read_u32(mem: &mut dyn MemorySource, addr: Option<u32>) -> Result<u32, SnapshotError> {
    let Some(addr) = addr else {
        return Ok(0);
    };
    mem.read_u32(addr).map_err(|_| SnapshotError::Read { addr })
}

fn read_flag(mem: &mut dyn MemorySource, addr: u32, ty: FieldType) -> Result<u32, SnapshotError> {
    let len = match ty {
        FieldType::U8 => 1,
        FieldType::U16 => 2,
        FieldType::U32 => 4,
        FieldType::F32 | FieldType::Vec3 => {
            return Err(SnapshotError::Read { addr });
        }
    };
    let mut buf = vec![0u8; len];
    read_exact(mem, addr, &mut buf)?;
    Ok(parse_small(&buf, ty).unwrap_or(0))
}

fn read_exact(mem: &mut dyn MemorySource, addr: u32, buf: &mut [u8]) -> Result<(), SnapshotError> {
    mem.read(addr, buf)
        .map_err(|_| SnapshotError::Read { addr })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse::SparseMemory;
    use crate::support::{live_profile, stage_hunt};
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn a_mid_read_frame_change_is_discarded() {
        let profile = live_profile();
        let mut backing = SparseMemory::new();
        stage_hunt(&mut backing, &profile, 10, true, false);
        let flips = Rc::new(Cell::new(0u32));
        let mut mem = FlipFrame {
            inner: backing,
            flips: flips.clone(),
            frame_addr: profile.frame_counter.addr.unwrap(),
        };
        let mut cache = CaptureCache::default();
        let shot = capture(&mut mem, &profile, &mut cache).unwrap();
        assert!(shot.is_none());
        assert!(flips.get() >= 2);
    }

    struct FlipFrame {
        inner: SparseMemory,
        flips: Rc<Cell<u32>>,
        frame_addr: u32,
    }

    impl MemorySource for FlipFrame {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> mhdn_rpc::Result<()> {
            self.inner.read(addr, buf)?;
            if addr == self.frame_addr {
                let seen = self.flips.get();
                self.flips.set(seen + 1);
                let value: u32 = if seen.is_multiple_of(2) { 10 } else { 11 };
                buf[..4].copy_from_slice(&value.to_le_bytes());
            }
            Ok(())
        }
    }
}
