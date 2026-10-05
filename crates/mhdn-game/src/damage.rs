//! Turn tap ring entries into exact damage events.
//!
//! The tap records the signed HP delta before the game clamps the stored HP at
//! zero, so a killing blow can sum to one more point than the bar had left.

use crate::model::{Anchor, MonsterKey};
use crate::scene::Scene;
use crate::track::LiveMonster;
use crate::TapEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    Passive,
    Tap,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageConfidence {
    Exact,
    HpDelta,
    AggregatedHpDelta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Hit,
    Poison,
    Status,
    Topple,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DamageEvent {
    pub seq: u32,
    pub guest_frame: u32,
    pub monster: u32,
    pub amount: u32,
    pub lr: u32,
    pub kind: DamageKind,
    pub source: EventSource,
    pub confidence: DamageConfidence,
    pub key: Option<MonsterKey>,
    pub anchor: Anchor,
    pub part_hp: Option<u32>,
    /// HP of this monster on the previous sample, when the tracker had a baseline.
    pub hp_before: Option<u32>,
    /// HP of this monster on the sample that emitted the event.
    pub hp_after: Option<u32>,
    /// Guest frames from that previous sample to this one.
    pub frames_since: Option<u32>,
    /// `r3` at the hook. Set only for tap events.
    pub tap_r3: Option<u32>,
    /// `sp[0..4]` at the hook. Set only for tap events.
    pub tap_sp: Option<[u32; 5]>,
    /// `sp[5..15]` when the wide stub captured them.
    pub tap_sp_hi: Option<[u32; 11]>,
    /// Word at static `0x0814E620` on a tap that already has a stack.
    /// `None` with [`Self::tap_sp`] set means that read failed.
    pub hunter_slot: Option<u32>,
}

/// One plugin hit. `hp_addr` is the monster HP word, the same address as [`MonsterKey::struct_addr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PluginHit {
    pub hp_addr: u32,
    pub amount: u32,
}

/// An HP-delta or residual larger than the bar it was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmgDrop {
    pub hp_addr: u32,
    pub amount: u32,
    pub prev_hp: u32,
    pub max_hp: u32,
}

impl DmgDrop {
    pub fn diag_line(&self) -> String {
        format!(
            "dmg_drop addr=0x{:08X} amount={} prev_hp={} max_hp={}",
            self.hp_addr, self.amount, self.prev_hp, self.max_hp
        )
    }
}

/// A tap whose monster object was not in the sample's monster list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmatchedTap {
    pub object: u32,
    pub lr: u32,
}

impl UnmatchedTap {
    pub fn diag_line(&self) -> String {
        format!(
            "tap_unmatched obj=0x{:08X} lr=0x{:08X}",
            self.object, self.lr
        )
    }
}

#[derive(Debug, Default)]
pub(crate) struct Composed {
    pub events: Vec<DamageEvent>,
    pub drops: Vec<DmgDrop>,
    pub unmatched: Vec<UnmatchedTap>,
}

/// One event per new `seq`. A non-positive delta is not damage. A repeated `seq` is dropped.
pub fn events_from_tap(events: &[TapEvent]) -> Vec<DamageEvent> {
    let mut out = Vec::new();
    let mut previous = 0u32;
    for event in events {
        if event.seq == 0 || event.seq <= previous {
            continue;
        }
        previous = event.seq;
        let amount = event.damage();
        if amount <= 0 {
            continue;
        }
        let (tap_r3, tap_sp, tap_sp_hi) = tap_words(event);
        out.push(DamageEvent {
            seq: event.seq,
            guest_frame: 0,
            monster: event.monster,
            amount: amount as u32,
            lr: event.lr,
            kind: kind_from_caller(event.lr, false),
            source: EventSource::Tap,
            confidence: DamageConfidence::Exact,
            key: None,
            anchor: Anchor::Unknown,
            part_hp: None,
            hp_before: None,
            hp_after: None,
            frames_since: None,
            tap_r3,
            tap_sp,
            tap_sp_hi,
            hunter_slot: None,
        });
    }
    out
}

pub fn tap_sum_for(events: &[DamageEvent], monster: u32) -> u32 {
    events
        .iter()
        .filter(|event| event.monster == monster && event.source == EventSource::Tap)
        .map(|event| event.amount)
        .sum()
}

/// `Some(extra)` when the tap sum covers the HP that was lost.
/// The extra is 0 while the monster is still alive, and the unused part of the
/// killing blow once HP is clamped at 0. `None` when the sums disagree.
/// Events for one consistent sample.
///
/// Plugin wins over the tap, and the tap wins over HP deltas. HP that the exact
/// source does not explain is emitted once, as a residual delta.
pub(crate) fn compose(
    scene: Scene,
    frame: u32,
    monsters: &[LiveMonster],
    taps: &[ResolvedTap],
    plugin: Option<&[PluginHit]>,
    poison_tick: Option<u32>,
) -> Composed {
    if scene != Scene::InQuest {
        return Composed::default();
    }
    if let Some(hits) = plugin {
        let mut out = Composed {
            events: plugin_events(frame, monsters, hits),
            ..Composed::default()
        };
        let (extra, drops) = residuals(frame, monsters, &sums_plugin(hits), poison_tick);
        out.events.extend(extra);
        out.drops = drops;
        return out;
    }
    if taps.is_empty() {
        return passive_only(frame, monsters, poison_tick);
    }
    let (events, unmatched) = tap_events(frame, monsters, taps);
    let (extra, drops) = residuals(frame, monsters, &sums_tap(taps, monsters), poison_tick);
    let mut out = Composed {
        events,
        drops,
        unmatched,
    };
    out.events.extend(extra);
    out
}

/// `tap_active` is false when the hook is not installed: HP deltas are the only source.
pub(crate) fn compose_sources(
    scene: Scene,
    frame: u32,
    monsters: &[LiveMonster],
    taps: &[ResolvedTap],
    plugin: Option<&[PluginHit]>,
    poison_tick: Option<u32>,
    tap_active: bool,
) -> Composed {
    if !tap_active && plugin.is_none() {
        return if scene == Scene::InQuest {
            passive_only(frame, monsters, poison_tick)
        } else {
            Composed::default()
        };
    }
    compose(scene, frame, monsters, taps, plugin, poison_tick)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedTap {
    pub event: TapEvent,
    pub anchor: Anchor,
}

fn passive_only(frame: u32, monsters: &[LiveMonster], poison_tick: Option<u32>) -> Composed {
    let mut out = Composed::default();
    for monster in monsters {
        apply_loss(
            frame,
            monster,
            poison_tick,
            0,
            &mut out.events,
            &mut out.drops,
        );
    }
    out
}

fn tap_events(
    frame: u32,
    monsters: &[LiveMonster],
    taps: &[ResolvedTap],
) -> (Vec<DamageEvent>, Vec<UnmatchedTap>) {
    let mut events = Vec::new();
    let mut unmatched = Vec::new();
    for tap in taps.iter().filter(|tap| tap.event.damage() > 0) {
        let Some(monster) = tap_monster(monsters, tap) else {
            unmatched.push(UnmatchedTap {
                object: tap.event.monster,
                lr: tap.event.lr,
            });
            continue;
        };
        let (hp_before, hp_after, frames_since) = sample_span(monster, frame);
        let (tap_r3, tap_sp, tap_sp_hi) = tap_words(&tap.event);
        events.push(DamageEvent {
            seq: tap.event.seq,
            guest_frame: frame,
            monster: tap.event.monster,
            amount: tap.event.damage() as u32,
            lr: tap.event.lr,
            kind: kind_from_caller(tap.event.lr, monster.poisoned),
            source: EventSource::Tap,
            confidence: DamageConfidence::Exact,
            key: Some(monster.key),
            anchor: tap.anchor,
            part_hp: Some(tap.event.stack[1]),
            hp_before,
            hp_after,
            frames_since,
            tap_r3,
            tap_sp,
            tap_sp_hi,
            hunter_slot: None,
        });
    }
    (events, unmatched)
}

fn tap_monster<'a>(monsters: &'a [LiveMonster], tap: &ResolvedTap) -> Option<&'a LiveMonster> {
    let hp_addr = tap.event.monster.wrapping_add(crate::model::HP_FROM_OBJECT);
    monsters.iter().find(|monster| {
        monster.key.struct_addr == hp_addr || monster.key.struct_addr == tap.event.monster
    })
}

fn plugin_events(frame: u32, monsters: &[LiveMonster], hits: &[PluginHit]) -> Vec<DamageEvent> {
    hits.iter()
        .filter(|hit| hit.amount > 0)
        .map(|hit| {
            let monster = monsters
                .iter()
                .find(|monster| monster.key.struct_addr == hit.hp_addr);
            let (hp_before, hp_after, frames_since) = monster
                .map(|monster| sample_span(monster, frame))
                .unwrap_or((None, None, None));
            DamageEvent {
                seq: 0,
                guest_frame: frame,
                monster: hit.hp_addr,
                amount: hit.amount,
                lr: 0,
                kind: DamageKind::Hit,
                source: EventSource::Plugin,
                confidence: DamageConfidence::Exact,
                key: monster.map(|monster| monster.key),
                anchor: Anchor::Unknown,
                part_hp: None,
                hp_before,
                hp_after,
                frames_since,
                tap_r3: None,
                tap_sp: None,
                tap_sp_hi: None,
                hunter_slot: None,
            }
        })
        .collect()
}

fn residuals(
    frame: u32,
    monsters: &[LiveMonster],
    exact: &[(u32, u32)],
    poison_tick: Option<u32>,
) -> (Vec<DamageEvent>, Vec<DmgDrop>) {
    let mut events = Vec::new();
    let mut drops = Vec::new();
    for monster in monsters {
        let explained = exact
            .iter()
            .find(|(addr, _)| *addr == monster.key.struct_addr)
            .map(|(_, amount)| *amount)
            .unwrap_or(0);
        apply_loss(
            frame,
            monster,
            poison_tick,
            explained,
            &mut events,
            &mut drops,
        );
    }
    (events, drops)
}

fn sums_tap(taps: &[ResolvedTap], monsters: &[LiveMonster]) -> Vec<(u32, u32)> {
    let mut sums = Vec::new();
    for tap in taps {
        if tap.event.damage() <= 0 {
            continue;
        }
        let Some(monster) = tap_monster(monsters, tap) else {
            continue;
        };
        add_sum(
            &mut sums,
            monster.key.struct_addr,
            tap.event.damage() as u32,
        );
    }
    sums
}

fn sums_plugin(hits: &[PluginHit]) -> Vec<(u32, u32)> {
    let mut sums = Vec::new();
    for hit in hits {
        add_sum(&mut sums, hit.hp_addr, hit.amount);
    }
    sums
}

fn add_sum(sums: &mut Vec<(u32, u32)>, addr: u32, amount: u32) {
    if let Some((_, total)) = sums.iter_mut().find(|(key, _)| *key == addr) {
        *total = total.saturating_add(amount);
    } else {
        sums.push((addr, amount));
    }
}

fn hp_loss(monster: &LiveMonster, frame: u32) -> Option<(u32, DamageConfidence)> {
    if monster.fresh {
        return None;
    }
    let prev = monster.prev_hp?;
    if monster.hp >= prev {
        return None;
    }
    let lost = prev - monster.hp;
    if lost == 0 {
        return None;
    }
    let confidence = match monster.prev_frame {
        Some(prev_frame) if frame.wrapping_sub(prev_frame) > 1 => {
            DamageConfidence::AggregatedHpDelta
        }
        _ => DamageConfidence::HpDelta,
    };
    Some((lost, confidence))
}

/// HP lost since the previous sample, or a drop when that loss cannot fit in the bar.
fn checked_loss(
    monster: &LiveMonster,
    frame: u32,
) -> Result<Option<(u32, DamageConfidence)>, DmgDrop> {
    let Some((lost, confidence)) = hp_loss(monster, frame) else {
        return Ok(None);
    };
    let prev = monster.prev_hp.unwrap_or(0);
    if exceeds_pool(lost, prev, monster.key.max_hp) {
        return Err(DmgDrop {
            hp_addr: monster.key.struct_addr,
            amount: lost,
            prev_hp: prev,
            max_hp: monster.key.max_hp,
        });
    }
    Ok(Some((lost, confidence)))
}

fn apply_loss(
    frame: u32,
    monster: &LiveMonster,
    poison_tick: Option<u32>,
    explained: u32,
    events: &mut Vec<DamageEvent>,
    drops: &mut Vec<DmgDrop>,
) {
    let (lost, confidence) = match checked_loss(monster, frame) {
        Ok(Some(loss)) => loss,
        Ok(None) => return,
        Err(drop) => {
            drops.push(drop);
            return;
        }
    };
    if lost <= explained {
        return;
    }
    let amount = lost - explained;
    let prev = monster.prev_hp.unwrap_or(0);
    if exceeds_pool(amount, prev, monster.key.max_hp) {
        drops.push(DmgDrop {
            hp_addr: monster.key.struct_addr,
            amount,
            prev_hp: prev,
            max_hp: monster.key.max_hp,
        });
        return;
    }
    let kind = if explained == 0 {
        kind_for(monster, amount, poison_tick)
    } else {
        DamageKind::Unknown
    };
    events.push(passive_like(monster, frame, amount, kind, confidence));
}

/// A delta larger than the previous bar, or larger than max HP, is not a hit.
fn exceeds_pool(amount: u32, prev_hp: u32, max_hp: u32) -> bool {
    amount > prev_hp || amount > max_hp
}

fn kind_for(monster: &LiveMonster, amount: u32, poison_tick: Option<u32>) -> DamageKind {
    if monster.poisoned && poison_tick == Some(amount) {
        DamageKind::Poison
    } else {
        DamageKind::Hit
    }
}

/// `lr` at the tap names the caller. An unknown return address stays a normal hit.
/// Poison is only the status caller while the matched monster is already poisoned;
/// the same caller is generic status otherwise.
fn kind_from_caller(lr: u32, poisoned: bool) -> DamageKind {
    match lr {
        crate::tap::CALLER_STATUS if poisoned => DamageKind::Poison,
        crate::tap::CALLER_STATUS => DamageKind::Status,
        crate::tap::CALLER_MOUNT_TOPPLE => DamageKind::Topple,
        _ => DamageKind::Hit,
    }
}

fn passive_like(
    monster: &LiveMonster,
    frame: u32,
    amount: u32,
    kind: DamageKind,
    confidence: DamageConfidence,
) -> DamageEvent {
    let (hp_before, hp_after, frames_since) = sample_span(monster, frame);
    DamageEvent {
        seq: 0,
        guest_frame: frame,
        monster: monster.key.struct_addr,
        amount,
        lr: 0,
        kind,
        source: EventSource::Passive,
        confidence,
        key: Some(monster.key),
        anchor: Anchor::Unknown,
        part_hp: None,
        hp_before,
        hp_after,
        frames_since,
        tap_r3: None,
        tap_sp: None,
        tap_sp_hi: None,
        hunter_slot: None,
    }
}

fn sample_span(monster: &LiveMonster, frame: u32) -> (Option<u32>, Option<u32>, Option<u32>) {
    (
        monster.prev_hp,
        Some(monster.hp),
        monster.prev_frame.map(|prev| frame.wrapping_sub(prev)),
    )
}

fn tap_words(event: &TapEvent) -> (Option<u32>, Option<[u32; 5]>, Option<[u32; 11]>) {
    (
        Some(event.r3),
        Some(event.stack),
        (event.sp_len == 16).then_some(event.stack_hi),
    )
}

pub fn overkill(tap_sum: u32, hp_start: u32, hp_end: u32) -> Option<u32> {
    let lost = hp_start.checked_sub(hp_end)?;
    if hp_end == 0 {
        tap_sum.checked_sub(lost)
    } else if tap_sum == lost {
        Some(0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(seq: u32, r1: i32, monster: u32) -> TapEvent {
        TapEvent {
            seq,
            r1,
            monster,
            r3: 0,
            lr: 0x008B_A260,
            stack: [0; 5],
            stack_hi: [0; 11],
            sp_len: 5,
        }
    }

    #[test]
    fn drops_duplicates_and_non_positive_deltas() {
        let events = events_from_tap(&[
            hit(1, -8, 0x1000),
            hit(1, -8, 0x1000),
            hit(2, 4, 0x1000),
            hit(3, -2, 0x1000),
        ]);
        assert_eq!(events.len(), 2);
        assert_eq!(tap_sum_for(&events, 0x1000), 10);
        assert!(events
            .iter()
            .all(|event| event.source == EventSource::Tap
                && event.confidence == DamageConfidence::Exact));
    }

    #[test]
    fn overkill_is_only_the_point_past_zero() {
        assert_eq!(overkill(775, 774, 0), Some(1));
        assert_eq!(overkill(10, 20, 10), Some(0));
        assert_eq!(overkill(9, 20, 10), None);
    }

    fn live(hp: u32, prev: Option<u32>, max_hp: u32) -> LiveMonster {
        LiveMonster {
            key: crate::model::MonsterKey {
                struct_addr: 0x3000_0360,
                species: 1,
                max_hp,
                generation: 1,
            },
            hp,
            prev_hp: prev,
            prev_frame: Some(1),
            fresh: false,
            poisoned: false,
        }
    }

    #[test]
    fn a_normal_hp_delta_still_emits() {
        let composed = compose(
            Scene::InQuest,
            2,
            &[live(760, Some(774), 774)],
            &[],
            None,
            None,
        );
        assert_eq!(composed.events.len(), 1);
        assert_eq!(composed.events[0].amount, 14);
        assert!(composed.drops.is_empty());
    }

    #[test]
    fn an_hp_delta_larger_than_max_hp_is_dropped() {
        let composed = compose(
            Scene::InQuest,
            2,
            &[live(700, Some(496_093_848), 774)],
            &[],
            None,
            None,
        );
        assert!(composed.events.is_empty());
        assert_eq!(composed.drops.len(), 1);
        assert_eq!(composed.drops[0].amount, 496_093_848 - 700);
        assert_eq!(composed.drops[0].max_hp, 774);
        assert_eq!(
            composed.drops[0].diag_line(),
            "dmg_drop addr=0x30000360 amount=496093148 prev_hp=496093848 max_hp=774"
        );
    }

    #[test]
    fn a_tap_for_an_unlisted_object_does_not_cover_the_hp_loss() {
        let mut event = hit(1, -14, 0x1111_0000);
        event.lr = 0x008B_A260;
        let tap = ResolvedTap {
            event,
            anchor: Anchor::Unknown,
        };
        let composed = compose(
            Scene::InQuest,
            2,
            &[live(760, Some(774), 774)],
            &[tap],
            None,
            None,
        );
        assert_eq!(composed.events.len(), 1);
        assert_eq!(composed.events[0].source, EventSource::Passive);
        assert_eq!(composed.events[0].amount, 14);
        assert_eq!(composed.unmatched.len(), 1);
        assert_eq!(composed.unmatched[0].object, 0x1111_0000);
        assert_eq!(composed.unmatched[0].lr, 0x008B_A260);
    }

    #[test]
    fn tap_caller_sets_the_damage_kind() {
        use crate::tap::{CALLER_HIT, CALLER_MOUNT_TOPPLE, CALLER_STATUS};

        let cases = [
            (CALLER_HIT, false, DamageKind::Hit),
            (0x0011_2233, false, DamageKind::Hit),
            (CALLER_MOUNT_TOPPLE, false, DamageKind::Topple),
            (CALLER_MOUNT_TOPPLE, true, DamageKind::Topple),
            (CALLER_STATUS, false, DamageKind::Status),
            (CALLER_STATUS, true, DamageKind::Poison),
            (CALLER_HIT, true, DamageKind::Hit),
        ];
        for (index, (lr, poisoned, kind)) in cases.into_iter().enumerate() {
            let mut monster = live(769, Some(774), 774);
            monster.poisoned = poisoned;
            let mut event = hit(index as u32 + 1, -5, monster.key.struct_addr);
            event.lr = lr;
            let composed = compose(
                Scene::InQuest,
                2,
                &[monster],
                &[ResolvedTap {
                    event,
                    anchor: Anchor::Unknown,
                }],
                None,
                None,
            );
            let tap = composed
                .events
                .iter()
                .find(|event| event.source == EventSource::Tap)
                .expect("tap event");
            assert_eq!(tap.kind, kind, "lr={lr:#010X} poisoned={poisoned}");
            assert_eq!(composed.events.len(), 1);
        }
    }
}
