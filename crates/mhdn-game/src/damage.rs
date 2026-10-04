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
}

/// One plugin hit. `hp_addr` is the monster HP word, the same address as [`MonsterKey::struct_addr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PluginHit {
    pub hp_addr: u32,
    pub amount: u32,
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
            kind: DamageKind::Hit,
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
pub fn compose(
    scene: Scene,
    frame: u32,
    monsters: &[LiveMonster],
    taps: &[ResolvedTap],
    plugin: Option<&[PluginHit]>,
    poison_tick: Option<u32>,
) -> Vec<DamageEvent> {
    if scene != Scene::InQuest {
        return Vec::new();
    }
    if let Some(hits) = plugin {
        let mut events = plugin_events(frame, monsters, hits);
        events.extend(residuals(frame, monsters, &sums_plugin(hits), poison_tick));
        return events;
    }
    let mut events = tap_events(frame, monsters, taps);
    events.extend(residuals(frame, monsters, &sums_tap(taps), poison_tick));
    if taps.is_empty() && plugin.is_none() {
        return passive_only(frame, monsters, poison_tick);
    }
    events
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
) -> Vec<DamageEvent> {
    if !tap_active && plugin.is_none() {
        return if scene == Scene::InQuest {
            passive_only(frame, monsters, poison_tick)
        } else {
            Vec::new()
        };
    }
    compose(scene, frame, monsters, taps, plugin, poison_tick)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedTap {
    pub event: TapEvent,
    pub anchor: Anchor,
}

fn passive_only(
    frame: u32,
    monsters: &[LiveMonster],
    poison_tick: Option<u32>,
) -> Vec<DamageEvent> {
    monsters
        .iter()
        .filter_map(|monster| passive_event(frame, monster, poison_tick))
        .collect()
}

fn passive_event(
    frame: u32,
    monster: &LiveMonster,
    poison_tick: Option<u32>,
) -> Option<DamageEvent> {
    let (amount, confidence) = hp_loss(monster, frame)?;
    Some(passive_like(
        monster,
        frame,
        amount,
        kind_for(monster, amount, poison_tick),
        confidence,
    ))
}

fn tap_events(frame: u32, monsters: &[LiveMonster], taps: &[ResolvedTap]) -> Vec<DamageEvent> {
    taps.iter()
        .filter(|tap| tap.event.damage() > 0)
        .map(|tap| {
            let amount = tap.event.damage() as u32;
            let hp_addr = tap.event.monster.wrapping_add(crate::model::HP_FROM_OBJECT);
            let monster = monsters.iter().find(|monster| {
                monster.key.struct_addr == hp_addr || monster.key.struct_addr == tap.event.monster
            });
            let (hp_before, hp_after, frames_since) = monster
                .map(|monster| sample_span(monster, frame))
                .unwrap_or((None, None, None));
            let (tap_r3, tap_sp, tap_sp_hi) = tap_words(&tap.event);
            DamageEvent {
                seq: tap.event.seq,
                guest_frame: frame,
                monster: tap.event.monster,
                amount,
                lr: tap.event.lr,
                kind: DamageKind::Hit,
                source: EventSource::Tap,
                confidence: DamageConfidence::Exact,
                key: monster.map(|monster| monster.key),
                anchor: tap.anchor,
                part_hp: Some(tap.event.stack[1]),
                hp_before,
                hp_after,
                frames_since,
                tap_r3,
                tap_sp,
                tap_sp_hi,
            }
        })
        .collect()
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
            }
        })
        .collect()
}

fn residuals(
    frame: u32,
    monsters: &[LiveMonster],
    exact: &[(u32, u32)],
    poison_tick: Option<u32>,
) -> Vec<DamageEvent> {
    let mut out = Vec::new();
    for monster in monsters {
        let Some((lost, confidence)) = hp_loss(monster, frame) else {
            continue;
        };
        let explained = exact
            .iter()
            .find(|(addr, _)| *addr == monster.key.struct_addr)
            .map(|(_, amount)| *amount)
            .unwrap_or(0);
        if lost > explained {
            let amount = lost - explained;
            let kind = if explained == 0 {
                kind_for(monster, amount, poison_tick)
            } else {
                DamageKind::Unknown
            };
            out.push(passive_like(monster, frame, amount, kind, confidence));
        }
    }
    out
}

fn sums_tap(taps: &[ResolvedTap]) -> Vec<(u32, u32)> {
    let mut sums = Vec::new();
    for tap in taps {
        if tap.event.damage() <= 0 {
            continue;
        }
        let hp_addr = tap.event.monster.wrapping_add(crate::model::HP_FROM_OBJECT);
        add_sum(&mut sums, hp_addr, tap.event.damage() as u32);
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

fn kind_for(monster: &LiveMonster, amount: u32, poison_tick: Option<u32>) -> DamageKind {
    if monster.poisoned && poison_tick == Some(amount) {
        DamageKind::Poison
    } else {
        DamageKind::Hit
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
}
