//! Turn tap ring entries into exact damage events.
//!
//! The tap records the signed HP delta before the game clamps the stored HP at
//! zero, so a killing blow can sum to one more point than the bar had left.

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
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageEvent {
    pub seq: u32,
    pub monster: u32,
    pub amount: u32,
    pub lr: u32,
    pub kind: DamageKind,
    pub source: EventSource,
    pub confidence: DamageConfidence,
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
        out.push(DamageEvent {
            seq: event.seq,
            monster: event.monster,
            amount: amount as u32,
            lr: event.lr,
            kind: DamageKind::Hit,
            source: EventSource::Tap,
            confidence: DamageConfidence::Exact,
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
