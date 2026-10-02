//! One sample: consistent snapshot, scene, identity, then tap / plugin / HP-delta events.

use mhdn_rpc::MemorySource;
use xxhash_rust::xxh3::xxh3_64;

use crate::chain::is_guest_heap;
use crate::damage::{compose_sources, PluginHit, ResolvedTap};
use crate::model::{Anchor, MonsterState, Snapshot, Vec3, BONE_POS_OFFSET};
use crate::profile::Profile;
use crate::scene::{Scene, SceneMachine};
use crate::snapshot::{capture, CaptureCache, SnapshotError};
use crate::tap::{
    hook_branch, install, uninstall, InstallOutcome, PatchMemory, TapError, TapEvent, ENTRY_SIZE,
    HOOK_ADDR, RING_ADDR, WRITE_SEQ_ADDR,
};
use crate::track::MonsterTrack;

const PLUGIN_ADDR: u32 = 0x0700_0000;
const DISCONNECT_AFTER: u32 = 8;

#[derive(Debug, Clone)]
pub struct Sample {
    pub snapshot: Option<Snapshot>,
    pub events: Vec<crate::DamageEvent>,
    pub torn: bool,
}

#[derive(Debug)]
pub struct Pipeline {
    scene: SceneMachine,
    track: MonsterTrack,
    cache: CaptureCache,
    last_tap_seq: u32,
    lost_tap: u64,
    failures: u32,
    fingerprints_ok: Option<bool>,
    tap_installed: bool,
    tap_blocked: bool,
    plugin_present: Option<bool>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            scene: SceneMachine::new(),
            track: MonsterTrack::default(),
            cache: CaptureCache::default(),
            last_tap_seq: 0,
            lost_tap: 0,
            failures: 0,
            fingerprints_ok: None,
            tap_installed: false,
            tap_blocked: false,
            plugin_present: None,
        }
    }

    pub fn scene(&self) -> Scene {
        self.scene.committed()
    }

    /// `None` until the fingerprint has been read. `false` means this build is not the profile.
    pub fn supported(&self) -> Option<bool> {
        self.fingerprints_ok
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn lost_tap(&self) -> u64 {
        self.lost_tap
    }

    pub fn poll(
        &mut self,
        mem: &mut dyn MemorySource,
        profile: &Profile,
        host_us: u64,
    ) -> Result<Sample, SnapshotError> {
        let raw = match capture(mem, profile, &mut self.cache) {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                return Ok(Sample {
                    snapshot: None,
                    events: Vec::new(),
                    torn: true,
                });
            }
            Err(err) => {
                self.note_failure();
                return Err(err);
            }
        };
        self.failures = 0;
        let change = self.scene.observe(raw.in_quest, raw.loading);
        if change.left_quest {
            self.track.clear();
            self.cache.clear();
        }
        let live = if change.scene == Scene::InQuest {
            self.track.update(&raw.monsters, raw.guest_frame)
        } else {
            Vec::new()
        };
        let events = if change.scene == Scene::InQuest {
            self.events(mem, profile, raw.guest_frame, &live)
        } else {
            Vec::new()
        };
        let monsters = live
            .iter()
            .zip(raw.monsters.iter())
            .map(|(monster, raw)| MonsterState {
                key: monster.key,
                hp: raw.hp,
                pos: raw.pos,
                visible: raw.visible,
                large: raw.large,
                poisoned: raw.poisoned,
            })
            .collect();
        Ok(Sample {
            snapshot: Some(Snapshot {
                guest_frame: raw.guest_frame,
                host_us,
                scene: change.scene,
                camera: raw.camera,
                hunter_pos: raw.hunter_pos,
                monsters,
            }),
            events,
            torn: false,
        })
    }

    /// Install the tap while a hunt is committed, and only after the `.text` hashes match.
    pub fn maintain_tap(
        &mut self,
        mem: &mut dyn PatchMemory,
        profile: &Profile,
    ) -> Result<(), TapError> {
        if profile.damage_tap.is_none() || self.tap_blocked {
            return Ok(());
        }
        if self.fingerprints_ok.is_none() {
            self.fingerprints_ok = Some(fingerprints_match(mem, profile));
        }
        if self.fingerprints_ok != Some(true) {
            return Ok(());
        }
        let hook = read_patch_u32(mem, HOOK_ADDR)?;
        if hook == hook_branch() {
            self.tap_installed = true;
            return Ok(());
        }
        if self.scene.committed() != Scene::InQuest {
            return Ok(());
        }
        match install(mem) {
            Ok(InstallOutcome::Installed | InstallOutcome::AlreadyInstalled) => {
                self.tap_installed = true;
                Ok(())
            }
            Err(
                TapError::ForeignHook { .. }
                | TapError::CaveOccupied
                | TapError::UnexpectedWord { .. },
            ) => {
                self.tap_blocked = true;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn shutdown_tap(&mut self, mem: &mut dyn PatchMemory) {
        if !self.tap_installed {
            return;
        }
        if uninstall(mem).is_ok() {
            self.tap_installed = false;
        }
    }

    fn note_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= DISCONNECT_AFTER {
            let change = self.scene.disconnect();
            if change.left_quest {
                self.track.clear();
                self.cache.clear();
            }
        }
    }

    fn events(
        &mut self,
        mem: &mut dyn MemorySource,
        profile: &Profile,
        frame: u32,
        live: &[crate::track::LiveMonster],
    ) -> Vec<crate::DamageEvent> {
        let hook = mem.read_u32(HOOK_ADDR).unwrap_or(0);
        let tap_active = hook == hook_branch();
        let plugin = self.plugin_hits(mem);
        let taps = if tap_active {
            self.read_new_taps(mem)
        } else {
            Vec::new()
        };
        let resolved: Vec<ResolvedTap> = if plugin.is_some() {
            Vec::new()
        } else {
            taps.iter()
                .map(|event| ResolvedTap {
                    event: *event,
                    anchor: bone_anchor(mem, event),
                })
                .collect()
        };
        compose_sources(
            Scene::InQuest,
            frame,
            live,
            &resolved,
            plugin.as_deref(),
            profile.species.poison_tick,
            tap_active,
        )
    }

    fn read_new_taps(&mut self, mem: &mut dyn MemorySource) -> Vec<TapEvent> {
        let Ok(write_seq) = mem.read_u32(WRITE_SEQ_ADDR) else {
            return Vec::new();
        };
        if write_seq == 0 || write_seq == self.last_tap_seq {
            return Vec::new();
        }
        if self.last_tap_seq != 0 {
            let gap = write_seq.wrapping_sub(self.last_tap_seq);
            if gap > 64 {
                self.lost_tap = self.lost_tap.saturating_add(u64::from(gap - 64));
            }
        }
        let start = write_seq.saturating_sub(64).max(self.last_tap_seq);
        let mut events = Vec::new();
        for seq in (start + 1)..=write_seq {
            let addr = RING_ADDR + (seq % 64) * ENTRY_SIZE;
            let mut buf = [0u8; ENTRY_SIZE as usize];
            if mem.read(addr, &mut buf).is_err() {
                continue;
            }
            if let Some(event) = TapEvent::decode(&buf) {
                if event.seq == seq && event.seq > self.last_tap_seq {
                    events.push(event);
                }
            }
        }
        self.last_tap_seq = write_seq;
        events
    }

    fn plugin_hits(&mut self, mem: &mut dyn MemorySource) -> Option<Vec<PluginHit>> {
        if self.plugin_present == Some(false) {
            return None;
        }
        let hits = read_plugin(mem);
        if hits.is_none() {
            self.plugin_present = Some(false);
        } else {
            self.plugin_present = Some(true);
        }
        hits
    }
}

fn bone_anchor(mem: &mut dyn MemorySource, event: &TapEvent) -> Anchor {
    let ptr = event.stack[2];
    if !is_guest_heap(ptr) {
        return Anchor::Unknown;
    }
    let addr = ptr.wrapping_add(BONE_POS_OFFSET);
    match mem.read_f32x3(addr) {
        Ok([x, y, z]) if x.is_finite() && y.is_finite() && z.is_finite() => {
            Anchor::World(Vec3::new(x, y, z))
        }
        _ => Anchor::Unknown,
    }
}

fn read_plugin(mem: &mut dyn MemorySource) -> Option<Vec<PluginHit>> {
    let mut magic = [0u8; 4];
    mem.read(PLUGIN_ADDR, &mut magic).ok()?;
    if &magic != b"MHDN" {
        return None;
    }
    if mem.read_u32(PLUGIN_ADDR + 4).ok()? != 1 {
        return None;
    }
    let count = mem.read_u32(PLUGIN_ADDR + 8).ok()?;
    if count > 64 {
        return None;
    }
    let mut hits = Vec::new();
    for index in 0..count {
        let addr = PLUGIN_ADDR + 12 + index * 8;
        let hp_addr = mem.read_u32(addr).ok()?;
        let amount = mem.read_u32(addr + 4).ok()?;
        if amount > 0 {
            hits.push(PluginHit { hp_addr, amount });
        }
    }
    Some(hits)
}

fn fingerprints_match(mem: &mut dyn PatchMemory, profile: &Profile) -> bool {
    let windows: Vec<_> = profile
        .meta
        .fingerprint
        .iter()
        .filter(|window| !window.is_placeholder())
        .collect();
    if windows.is_empty() {
        return true;
    }
    for window in windows {
        let mut buf = vec![0u8; window.len as usize];
        if mem.read(window.addr, &mut buf).is_err() {
            return false;
        }
        if xxh3_64(&buf) != window.xxh3 {
            return false;
        }
    }
    true
}

fn read_patch_u32(mem: &mut dyn PatchMemory, addr: u32) -> Result<u32, TapError> {
    let mut buf = [0u8; 4];
    mem.read(addr, &mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HP_FROM_OBJECT;
    use crate::scene::Scene;
    use crate::sparse::SparseMemory;
    use crate::support::{hp_addr, live_profile, place_monster, stage_hunt};
    use crate::tap::{
        hook_branch, ENTRY_SIZE, EXPECTED_HOOK, EXPECTED_HP_STORE, EXPECTED_NEXT, RING_ADDR,
    };
    use crate::{DamageConfidence, DamageKind, EventSource};
    use mhdn_rpc::MemorySource;

    fn prepared() -> (Pipeline, SparseMemory, Profile) {
        let mut profile = live_profile();
        profile.meta.fingerprint.clear();
        let mut mem = SparseMemory::new();
        stage_hunt(&mut mem, &profile, 1, true, false);
        place_monster(&mut mem, 0, 774, 774, 1, [10.0, 20.0, 30.0], 0);
        mem.write_u32(HOOK_ADDR, EXPECTED_HOOK);
        mem.write_u32(EXPECTED_NEXT_ADDR, EXPECTED_NEXT);
        mem.write_u32(0x008D_03FC, EXPECTED_HP_STORE);
        (Pipeline::new(), mem, profile)
    }

    const EXPECTED_NEXT_ADDR: u32 = 0x008D_03EC;

    fn warm(pipeline: &mut Pipeline, mem: &mut SparseMemory, profile: &Profile) {
        for frame in 1..=3 {
            mem.write_u32(profile.frame_counter.addr.unwrap(), frame);
            pipeline
                .poll(mem, profile, u64::from(frame) * 16_000)
                .unwrap();
        }
    }

    #[test]
    fn passive_hit_conserves_hp_after_the_scene_commits() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        assert_eq!(pipeline.scene(), Scene::InQuest);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [11.0, 20.0, 30.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 64_000).unwrap();
        let events = sample.events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].amount, 14);
        assert_eq!(events[0].source, EventSource::Passive);
        assert_eq!(events[0].confidence, DamageConfidence::HpDelta);
        assert_eq!(events[0].kind, DamageKind::Hit);
        assert_eq!(sample.snapshot.unwrap().monsters[0].pos.x, 11.0);
    }

    #[test]
    fn skipped_frames_are_aggregated_and_poison_ticks_are_labeled() {
        let (mut pipeline, mut mem, mut profile) = prepared();
        profile.species.poison_tick = Some(5);
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 9);
        place_monster(&mut mem, 0, 769, 774, 1, [10.0, 20.0, 30.0], 1);
        let sample = pipeline.poll(&mut mem, &profile, 90_000).unwrap();
        assert_eq!(sample.events.len(), 1);
        assert_eq!(sample.events[0].amount, 5);
        assert_eq!(
            sample.events[0].confidence,
            DamageConfidence::AggregatedHpDelta
        );
        assert_eq!(sample.events[0].kind, DamageKind::Poison);
    }

    #[test]
    fn a_respawn_does_not_invent_damage() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        mem.write_u32(0x082C_E730 + 0x14, 0);
        pipeline.poll(&mut mem, &profile, 40_000).unwrap();
        mem.write_u32(profile.frame_counter.addr.unwrap(), 5);
        place_monster(&mut mem, 0, 774, 774, 1, [1.0, 2.0, 3.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 50_000).unwrap();
        assert!(sample.events.is_empty());
    }

    #[test]
    fn tap_hits_replace_the_hp_delta_and_keep_the_bone() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        assert_eq!(mem_word(&mut mem, HOOK_ADDR), hook_branch());
        let object = hp_addr(0) - HP_FROM_OBJECT;
        let bone = 0x300A_0000;
        mem.write_vec3(bone + BONE_POS_OFFSET, [9.0, 8.0, 7.0]);
        write_tap(&mut mem, 1, -14, object, bone, 90);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 70_000).unwrap();
        assert_eq!(sample.events.len(), 1);
        assert_eq!(sample.events[0].source, EventSource::Tap);
        assert_eq!(sample.events[0].confidence, DamageConfidence::Exact);
        assert_eq!(sample.events[0].amount, 14);
        assert_eq!(sample.events[0].part_hp, Some(90));
        assert_eq!(
            sample.events[0].anchor,
            Anchor::World(crate::model::Vec3::new(9.0, 8.0, 7.0))
        );
        pipeline.shutdown_tap(&mut mem);
        assert_eq!(mem_word(&mut mem, HOOK_ADDR), EXPECTED_HOOK);
    }

    #[test]
    fn steady_state_reads_stay_inside_the_rpc_budget() {
        let (mut pipeline, mut mem, profile) = prepared();
        for slot in 1..4 {
            place_monster(
                &mut mem,
                slot,
                200,
                200,
                10 + slot as u16,
                [1.0, 1.0, 1.0],
                0,
            );
        }
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        let mut counted = CountMem {
            inner: &mut mem,
            reads: 0,
        };
        pipeline.poll(&mut counted, &profile, 80_000).unwrap();
        let per_second = counted.reads * 60;
        assert!(
            per_second <= 2500,
            "steady-state reads {reads} * 60 = {per_second}",
            reads = counted.reads
        );
    }

    fn write_tap(mem: &mut SparseMemory, seq: u32, r1: i32, monster: u32, bone: u32, part_hp: u32) {
        let mut raw = [0u8; ENTRY_SIZE as usize];
        raw[0..4].copy_from_slice(&seq.to_le_bytes());
        raw[4..8].copy_from_slice(&r1.to_le_bytes());
        raw[8..12].copy_from_slice(&monster.to_le_bytes());
        raw[24..28].copy_from_slice(&part_hp.to_le_bytes());
        raw[28..32].copy_from_slice(&bone.to_le_bytes());
        let addr = RING_ADDR + (seq % 64) * ENTRY_SIZE;
        mem.write_bytes(addr, &raw);
        mem.write_u32(WRITE_SEQ_ADDR, seq);
    }

    fn mem_word(mem: &mut SparseMemory, addr: u32) -> u32 {
        mem.read_u32(addr).unwrap()
    }

    struct CountMem<'a> {
        inner: &'a mut SparseMemory,
        reads: u32,
    }

    impl MemorySource for CountMem<'_> {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> mhdn_rpc::Result<()> {
            self.reads += 1;
            MemorySource::read(self.inner, addr, buf)
        }
    }
}
