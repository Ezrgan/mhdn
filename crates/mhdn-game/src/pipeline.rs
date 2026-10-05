//! One sample: consistent snapshot, scene, identity, then tap / plugin / HP-delta events.

use mhdn_rpc::MemorySource;
use xxhash_rust::xxh3::xxh3_64;

use crate::damage::{compose_sources, DmgDrop, PluginHit, ResolvedTap, UnmatchedTap};
use crate::model::{Anchor, MonsterState, Snapshot};
use crate::profile::Profile;
use crate::scene::{Scene, SceneMachine};
use crate::snapshot::{capture, CaptureCache, RejectedRead, SnapshotError};
use crate::tap::{
    hook_branch, install, install_wide, uninstall, InstallOutcome, PatchMemory, RingLayout,
    TapError, TapEvent, HOOK_ADDR, RING_ADDR, WIDE_ENTRY_SIZE,
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
    rejected: Vec<RejectedRead>,
    drops: Vec<DmgDrop>,
    unmatched: Vec<UnmatchedTap>,
    failures: u32,
    fingerprints_ok: Option<bool>,
    tap_installed: bool,
    tap_blocked: bool,
    plugin_present: Option<bool>,
    wide: bool,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self::with_wide(false)
    }

    /// Reads `MHDN_TAP_WIDE` once, when the session starts.
    pub fn from_env() -> Self {
        Self::with_wide(crate::tap::wide_requested())
    }

    pub fn with_wide(wide: bool) -> Self {
        Self {
            scene: SceneMachine::new(),
            track: MonsterTrack::default(),
            cache: CaptureCache::default(),
            last_tap_seq: 0,
            lost_tap: 0,
            rejected: Vec::new(),
            drops: Vec::new(),
            unmatched: Vec::new(),
            failures: 0,
            fingerprints_ok: None,
            tap_installed: false,
            tap_blocked: false,
            plugin_present: None,
            wide,
        }
    }

    fn ring(&self) -> RingLayout {
        if self.wide {
            RingLayout::wide()
        } else {
            RingLayout::standard()
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

    pub fn tap_installed(&self) -> bool {
        self.tap_installed
    }

    pub fn tap_blocked(&self) -> bool {
        self.tap_blocked
    }

    pub fn lost_tap(&self) -> u64 {
        self.lost_tap
    }

    /// Impossible HP reads from the latest `poll`.
    pub fn take_rejected_reads(&mut self) -> Vec<RejectedRead> {
        std::mem::take(&mut self.rejected)
    }

    /// HP-delta and residual events dropped because they exceeded the bar.
    pub fn take_dmg_drops(&mut self) -> Vec<DmgDrop> {
        std::mem::take(&mut self.drops)
    }

    /// Tap hits whose object was not in the monster list.
    pub fn take_unmatched_taps(&mut self) -> Vec<UnmatchedTap> {
        std::mem::take(&mut self.unmatched)
    }

    pub(crate) fn drain_diag_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.extend(
            self.take_rejected_reads()
                .iter()
                .map(RejectedRead::diag_line),
        );
        lines.extend(self.take_dmg_drops().iter().map(DmgDrop::diag_line));
        lines.extend(
            self.take_unmatched_taps()
                .iter()
                .map(UnmatchedTap::diag_line),
        );
        lines
    }

    pub fn poll(
        &mut self,
        mem: &mut dyn MemorySource,
        profile: &Profile,
        host_us: u64,
    ) -> Result<Sample, SnapshotError> {
        self.rejected.clear();
        self.drops.clear();
        self.unmatched.clear();
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
        self.rejected = raw.rejected;
        // The profile's in-quest word reads 0 for most of a live hunt. A resolved monster list is the hunt.
        let hunting = !raw.monsters.is_empty();
        let change = self.scene.observe(hunting, raw.loading && !hunting);
        if change.left_quest {
            self.track.clear();
            self.cache.clear();
        }
        let live = if change.scene == Scene::InQuest {
            self.track.update(&raw.monsters, raw.guest_frame)
        } else {
            Vec::new()
        };
        let taps = self.poll_taps(mem);
        let events = if change.scene == Scene::InQuest {
            self.events(
                mem,
                raw.guest_frame,
                &live,
                taps,
                profile,
                raw.hunter_slot,
            )
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
                slot: raw.slot,
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
            self.adopt_tap(mem);
            return Ok(());
        }
        if self.scene.committed() != Scene::InQuest {
            return Ok(());
        }
        match if self.wide {
            install_wide(mem)
        } else {
            install(mem)
        } {
            Ok(InstallOutcome::Installed | InstallOutcome::AlreadyInstalled) => {
                self.adopt_tap(mem);
                Ok(())
            }
            Err(
                TapError::ForeignHook { .. }
                | TapError::CaveOccupied
                | TapError::RingOccupied
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

    /// Start reading after whatever the ring already holds, so an old hunt is never replayed.
    fn adopt_tap(&mut self, mem: &mut dyn PatchMemory) {
        if self.tap_installed {
            return;
        }
        if let Ok(seq) = read_patch_u32(mem, self.ring().write_seq) {
            self.last_tap_seq = seq;
            self.tap_installed = true;
        }
    }

    /// Consume the ring on every sample, in or out of a hunt. `None` while the hook is not ours.
    fn poll_taps(&mut self, mem: &mut dyn MemorySource) -> Option<Vec<TapEvent>> {
        if !self.tap_installed {
            return None;
        }
        if mem.read_u32(HOOK_ADDR).ok() != Some(hook_branch()) {
            self.tap_installed = false;
            return None;
        }
        Some(self.read_new_taps(mem))
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
        frame: u32,
        live: &[crate::track::LiveMonster],
        taps: Option<Vec<TapEvent>>,
        profile: &Profile,
        hunter_slot: Option<u32>,
    ) -> Vec<crate::DamageEvent> {
        let tap_active = taps.is_some();
        let plugin = self.plugin_hits(mem);
        // `sp[2]` is the monster-list slot, not the struck bone, so the anchor comes from the monster.
        let resolved: Vec<ResolvedTap> = if plugin.is_some() {
            Vec::new()
        } else {
            taps.unwrap_or_default()
                .into_iter()
                .map(|event| ResolvedTap {
                    event,
                    anchor: Anchor::Unknown,
                })
                .collect()
        };
        let composed = compose_sources(
            Scene::InQuest,
            frame,
            live,
            &resolved,
            plugin.as_deref(),
            profile.species.poison_tick,
            tap_active,
        );
        self.drops = composed.drops;
        self.unmatched = composed.unmatched;
        let mut events = composed.events;
        for event in &mut events {
            if event.tap_sp.is_some() {
                event.hunter_slot = hunter_slot;
            }
        }
        events
    }

    fn read_new_taps(&mut self, mem: &mut dyn MemorySource) -> Vec<TapEvent> {
        let layout = self.ring();
        let Ok(write_seq) = mem.read_u32(layout.write_seq) else {
            return Vec::new();
        };
        if write_seq == 0 || write_seq == self.last_tap_seq {
            return Vec::new();
        }
        if self.last_tap_seq != 0 {
            let gap = write_seq.wrapping_sub(self.last_tap_seq);
            if gap > layout.capacity {
                self.lost_tap = self
                    .lost_tap
                    .saturating_add(u64::from(gap - layout.capacity));
            }
        }
        let start = write_seq
            .saturating_sub(layout.capacity)
            .max(self.last_tap_seq);
        let mut events = Vec::new();
        for seq in (start + 1)..=write_seq {
            let addr = RING_ADDR + (seq % layout.capacity) * layout.entry_size;
            let mut buf = [0u8; WIDE_ENTRY_SIZE as usize];
            let len = layout.entry_size as usize;
            if mem.read(addr, &mut buf[..len]).is_err() {
                continue;
            }
            if let Some(event) = TapEvent::decode(&buf[..len]) {
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
    use crate::profile::SpeciesAnchor;
    use crate::scene::Scene;
    use crate::snapshot::ReadReject;
    use crate::sparse::SparseMemory;
    use crate::support::{hp_addr, live_profile, place_monster, stage_hunt};
    use crate::tap::{
        hook_branch, ENTRY_SIZE, EXPECTED_HOOK, EXPECTED_HP_STORE, EXPECTED_NEXT, RING_ADDR,
        WIDE_CAPACITY, WIDE_ENTRY_SIZE, WIDE_WRITE_SEQ_ADDR, WRITE_SEQ_ADDR,
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
        assert_eq!(events[0].hp_before, Some(774));
        assert_eq!(events[0].hp_after, Some(760));
        assert_eq!(events[0].frames_since, Some(1));
        assert!(events[0].tap_sp.is_none());
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
        assert_eq!(sample.events[0].frames_since, Some(6));
        assert_eq!(sample.events[0].hp_before, Some(774));
        assert_eq!(sample.events[0].hp_after, Some(769));
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
    fn tap_hits_replace_the_hp_delta_and_anchor_on_the_monster() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        assert_eq!(mem_word(&mut mem, HOOK_ADDR), hook_branch());
        let object = hp_addr(0) - HP_FROM_OBJECT;
        write_tap(&mut mem, 1, -14, object, 0x300A_0000, 90);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 70_000).unwrap();
        assert_eq!(sample.events.len(), 1);
        assert_eq!(sample.events[0].source, EventSource::Tap);
        assert_eq!(sample.events[0].confidence, DamageConfidence::Exact);
        assert_eq!(sample.events[0].amount, 14);
        assert_eq!(sample.events[0].part_hp, Some(90));
        assert_eq!(sample.events[0].tap_sp, Some([0, 90, 0x300A_0000, 0, 0]));
        assert_eq!(sample.events[0].hp_before, Some(774));
        assert_eq!(sample.events[0].hp_after, Some(760));
        assert_eq!(sample.events[0].frames_since, Some(1));
        assert_eq!(sample.events[0].anchor, Anchor::Unknown);
        assert_eq!(sample.events[0].hunter_slot, Some(0x3004_0000));
        assert_eq!(
            sample.events[0].key.map(|key| key.struct_addr),
            Some(hp_addr(0))
        );
        pipeline.shutdown_tap(&mut mem);
        assert_eq!(mem_word(&mut mem, HOOK_ADDR), EXPECTED_HOOK);
    }

    #[test]
    fn a_tap_keeps_the_word_read_at_the_static_hunter_slot() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        let object = hp_addr(0) - HP_FROM_OBJECT;
        write_tap(&mut mem, 1, -14, object, 0x300A_0000, 90);
        mem.write_u32(0x0814_E620, 0x3004_BEEF);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 70_000).unwrap();
        assert_eq!(sample.events.len(), 1);
        assert!(sample.events[0].tap_sp.is_some());
        assert_eq!(sample.events[0].hunter_slot, Some(0x3004_BEEF));
        assert_eq!(sample.events[0].anchor, Anchor::Unknown);
        assert_eq!(sample.events[0].kind, DamageKind::Hit);
    }

    #[test]
    fn a_failed_static_slot_read_stays_unset_on_the_tap() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        let object = hp_addr(0) - HP_FROM_OBJECT;
        write_tap(&mut mem, 1, -14, object, 0x300A_0000, 90);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let sample = pipeline
            .poll(
                &mut FailHunterSlot { inner: &mut mem },
                &profile,
                70_000,
            )
            .unwrap();
        assert_eq!(sample.events.len(), 1);
        assert!(sample.events[0].tap_sp.is_some());
        assert_eq!(sample.events[0].hunter_slot, None);
        assert_eq!(sample.events[0].anchor, Anchor::Unknown);
        assert_eq!(sample.events[0].kind, DamageKind::Hit);
    }

    #[test]
    fn an_adopted_tap_does_not_replay_old_hits_and_drains_outside_the_hunt() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        let object = hp_addr(0) - HP_FROM_OBJECT;
        for seq in 1..=40 {
            write_tap(&mut mem, seq, -5, object, 0, 90);
        }
        let mut fresh = Pipeline::new();
        warm(&mut fresh, &mut mem, &profile);
        fresh.maintain_tap(&mut mem, &profile).unwrap();
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        assert!(fresh
            .poll(&mut mem, &profile, 64_000)
            .unwrap()
            .events
            .is_empty());

        mem.write_u32(0x082C_E730 + 0x14, 0);
        for frame in 5..(5 + u32::from(crate::scene::LEAVE_HUNT)) {
            mem.write_u32(profile.frame_counter.addr.unwrap(), frame);
            fresh
                .poll(&mut mem, &profile, u64::from(frame) * 16_000)
                .unwrap();
        }
        assert_ne!(fresh.scene(), Scene::InQuest);
        write_tap(&mut mem, 41, -7, object, 0, 90);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 100);
        assert!(fresh
            .poll(&mut mem, &profile, 1_600_000)
            .unwrap()
            .events
            .is_empty());

        place_monster(&mut mem, 0, 774, 774, 1, [10.0, 20.0, 30.0], 0);
        for frame in 101..=103 {
            mem.write_u32(profile.frame_counter.addr.unwrap(), frame);
            fresh
                .poll(&mut mem, &profile, u64::from(frame) * 16_000)
                .unwrap();
        }
        assert_eq!(fresh.scene(), Scene::InQuest);
        write_tap(&mut mem, 42, -9, object, 0, 90);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 104);
        let events = fresh.poll(&mut mem, &profile, 1_700_000).unwrap().events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].amount, 9);
    }

    #[test]
    fn the_hunt_follows_the_monster_list_not_the_quest_word() {
        let mut profile = live_profile();
        profile.meta.fingerprint.clear();
        let mut mem = SparseMemory::new();
        stage_hunt(&mut mem, &profile, 1, false, true);
        place_monster(&mut mem, 0, 500, 774, 1, [1.0, 2.0, 3.0], 0);
        let mut pipeline = Pipeline::new();
        warm(&mut pipeline, &mut mem, &profile);
        assert_eq!(pipeline.scene(), Scene::InQuest);

        let mut village = SparseMemory::new();
        stage_hunt(&mut village, &profile, 1, true, false);
        let mut pipeline = Pipeline::new();
        warm(&mut pipeline, &mut village, &profile);
        assert_eq!(pipeline.scene(), Scene::Village);
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

    #[test]
    fn wide_tap_reads_the_short_ring_and_keeps_the_extra_stack_words() {
        let (_default, mut mem, profile) = prepared();
        let mut pipeline = Pipeline::with_wide(true);
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        let object = hp_addr(0) - HP_FROM_OBJECT;
        let mut raw = [0u8; WIDE_ENTRY_SIZE as usize];
        raw[0..4].copy_from_slice(&1u32.to_le_bytes());
        raw[4..8].copy_from_slice(&(-14i32 as u32).to_le_bytes());
        raw[8..12].copy_from_slice(&object.to_le_bytes());
        raw[80..84].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        let addr = RING_ADDR + (1 % WIDE_CAPACITY) * WIDE_ENTRY_SIZE;
        mem.write_bytes(addr, &raw);
        mem.write_u32(WIDE_WRITE_SEQ_ADDR, 1);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 70_000).unwrap();
        assert_eq!(sample.events.len(), 1);
        assert_eq!(sample.events[0].source, EventSource::Tap);
        assert_eq!(sample.events[0].amount, 14);
        assert_eq!(
            sample.events[0].tap_sp_hi.map(|words| words[10]),
            Some(0x0102_0304)
        );
        mem.write_u32(WIDE_WRITE_SEQ_ADDR, 5);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 5);
        pipeline.poll(&mut mem, &profile, 80_000).unwrap();
        assert_eq!(pipeline.lost_tap(), 2);
    }

    #[test]
    fn garbage_hp_then_a_real_read_emits_nothing() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 496_093_848, 774, 1, [10.0, 20.0, 30.0], 0);
        let bad = pipeline.poll(&mut mem, &profile, 64_000).unwrap();
        assert!(bad.events.is_empty());
        assert!(bad.snapshot.unwrap().monsters.is_empty());
        let rejected = pipeline.take_rejected_reads();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].hp_addr, hp_addr(0));
        assert_eq!(rejected[0].species, 1);
        assert_eq!(rejected[0].hp, 496_093_848);
        assert_eq!(rejected[0].max_hp, 774);
        assert_eq!(rejected[0].reason, ReadReject::HpAboveMax);
        assert_eq!(
            rejected[0].diag_line(),
            format!(
                "hp_bad addr=0x{:08X} species=1 hp=496093848 max_hp=774 reason=hp_above_max",
                hp_addr(0)
            )
        );

        mem.write_u32(profile.frame_counter.addr.unwrap(), 5);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let restored = pipeline.poll(&mut mem, &profile, 80_000).unwrap();
        assert!(restored.events.is_empty());
        assert_eq!(restored.snapshot.unwrap().monsters.len(), 1);

        mem.write_u32(profile.frame_counter.addr.unwrap(), 6);
        place_monster(&mut mem, 0, 750, 774, 1, [10.0, 20.0, 30.0], 0);
        let hit = pipeline.poll(&mut mem, &profile, 96_000).unwrap();
        assert_eq!(hit.events.len(), 1);
        assert_eq!(hit.events[0].amount, 10);
        assert!(pipeline.take_dmg_drops().is_empty());
    }

    #[test]
    fn one_absent_sample_during_an_area_change_emits_nothing() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        mem.write_u32(0x082C_E730 + 0x14, 0);
        let gap = pipeline.poll(&mut mem, &profile, 64_000).unwrap();
        assert!(gap.events.is_empty());
        assert!(gap.snapshot.unwrap().monsters.is_empty());
        assert!(pipeline.take_rejected_reads().is_empty());

        mem.write_u32(profile.frame_counter.addr.unwrap(), 5);
        place_monster(&mut mem, 0, 700, 774, 1, [10.0, 20.0, 30.0], 0);
        let back = pipeline.poll(&mut mem, &profile, 80_000).unwrap();
        assert!(back.events.is_empty());
        assert_eq!(back.snapshot.unwrap().monsters[0].hp, 700);

        mem.write_u32(profile.frame_counter.addr.unwrap(), 6);
        place_monster(&mut mem, 0, 690, 774, 1, [10.0, 20.0, 30.0], 0);
        let hit = pipeline.poll(&mut mem, &profile, 96_000).unwrap();
        assert_eq!(hit.events.len(), 1);
        assert_eq!(hit.events[0].amount, 10);
    }

    #[test]
    fn a_reused_slot_with_another_species_emits_nothing() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 100, 500, 2, [10.0, 20.0, 30.0], 0);
        let reused = pipeline.poll(&mut mem, &profile, 64_000).unwrap();
        assert!(reused.events.is_empty());
        let monster = &reused.snapshot.unwrap().monsters[0];
        assert_eq!(monster.key.species, 2);
        assert_eq!(monster.key.generation, 2);

        mem.write_u32(profile.frame_counter.addr.unwrap(), 5);
        place_monster(&mut mem, 0, 90, 500, 2, [10.0, 20.0, 30.0], 0);
        let hit = pipeline.poll(&mut mem, &profile, 80_000).unwrap();
        assert_eq!(hit.events.len(), 1);
        assert_eq!(hit.events[0].amount, 10);
    }

    #[test]
    fn max_hp_outside_the_plausible_range_is_rejected() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 200_000, 200_000, 1, [10.0, 20.0, 30.0], 0);
        let ceiling = pipeline.poll(&mut mem, &profile, 64_000).unwrap();
        assert_eq!(ceiling.snapshot.unwrap().monsters.len(), 1);
        assert!(pipeline.take_rejected_reads().is_empty());

        mem.write_u32(profile.frame_counter.addr.unwrap(), 5);
        place_monster(&mut mem, 0, 100, 200_001, 1, [10.0, 20.0, 30.0], 0);
        let bad = pipeline.poll(&mut mem, &profile, 80_000).unwrap();
        assert!(bad.events.is_empty());
        assert!(bad.snapshot.unwrap().monsters.is_empty());
        let rejected = pipeline.take_rejected_reads();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].hp, 100);
        assert_eq!(rejected[0].max_hp, 200_001);
        assert_eq!(rejected[0].reason, ReadReject::MaxHpRange);
        assert!(rejected[0].diag_line().contains("reason=max_hp_range"));
    }

    #[test]
    fn an_unlisted_species_is_rejected_when_the_profile_has_a_table() {
        let (mut pipeline, mut mem, mut profile) = prepared();
        profile.species.by_id.insert(
            "1".to_string(),
            SpeciesAnchor {
                anchor_height: 150.0,
            },
        );
        warm(&mut pipeline, &mut mem, &profile);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 774, 774, 44, [10.0, 20.0, 30.0], 0);
        let bad = pipeline.poll(&mut mem, &profile, 64_000).unwrap();
        assert!(bad.events.is_empty());
        assert!(bad.snapshot.unwrap().monsters.is_empty());
        let rejected = pipeline.take_rejected_reads();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].species, 44);
        assert_eq!(rejected[0].reason, ReadReject::Species);
        assert!(rejected[0].diag_line().contains("reason=species"));
    }

    #[test]
    fn a_tap_for_an_unknown_object_is_counted_and_not_emitted() {
        let (mut pipeline, mut mem, profile) = prepared();
        warm(&mut pipeline, &mut mem, &profile);
        pipeline.maintain_tap(&mut mem, &profile).unwrap();
        let lr = 0x008B_A260;
        write_tap(&mut mem, 1, -14, 0x1111_0000, 0, 0);
        let slot = RING_ADDR + ENTRY_SIZE;
        mem.write_u32(slot + 16, lr);
        mem.write_u32(profile.frame_counter.addr.unwrap(), 4);
        place_monster(&mut mem, 0, 760, 774, 1, [10.0, 20.0, 30.0], 0);
        let sample = pipeline.poll(&mut mem, &profile, 70_000).unwrap();
        assert_eq!(sample.events.len(), 1);
        assert_eq!(sample.events[0].source, EventSource::Passive);
        assert_eq!(sample.events[0].amount, 14);
        assert!(sample
            .events
            .iter()
            .all(|event| event.source != EventSource::Tap));
        let unmatched = pipeline.take_unmatched_taps();
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0].object, 0x1111_0000);
        assert_eq!(unmatched[0].lr, lr);
        assert_eq!(
            unmatched[0].diag_line(),
            "tap_unmatched obj=0x11110000 lr=0x008BA260"
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

    struct FailHunterSlot<'a> {
        inner: &'a mut SparseMemory,
    }

    impl MemorySource for FailHunterSlot<'_> {
        fn read(&mut self, addr: u32, buf: &mut [u8]) -> mhdn_rpc::Result<()> {
            if addr == 0x0814_E620 {
                return Err(mhdn_rpc::RpcError::ReadFailed { addr });
            }
            MemorySource::read(self.inner, addr, buf)
        }
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
