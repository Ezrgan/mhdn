//! The 2026-09-28 boss kill, read back from the tap ring while the fight ran.

use mhdn_game::{events_from_tap, overkill, tap_sum_for, TapEvent};

const BOSS: u32 = 0x3006_5558;

#[test]
fn boss_kill_sums_to_one_point_of_overkill() {
    let text = include_str!("fixtures/tap-boss-2026-09-28.txt");
    let mut raw = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let seq: u32 = parts.next().unwrap().parse().unwrap();
        let _damage: i32 = parts.next().unwrap().parse().unwrap();
        let r1: i32 = parts.next().unwrap().parse().unwrap();
        let monster = u32::from_str_radix(parts.next().unwrap(), 16).unwrap();
        let lr = u32::from_str_radix(parts.next().unwrap(), 16).unwrap();
        raw.push(TapEvent {
            seq,
            r1,
            monster,
            r3: 0,
            lr,
            stack: [0; 5],
        });
    }

    let events = events_from_tap(&raw);
    assert_eq!(events.len(), 126);
    let boss_hits: Vec<_> = events
        .iter()
        .filter(|event| event.monster == BOSS)
        .collect();
    assert_eq!(boss_hits.len(), 111);
    let sum = tap_sum_for(&events, BOSS);
    assert_eq!(sum, 775);
    assert_eq!(overkill(sum, 774, 0), Some(1));
    assert_eq!(boss_hits.last().unwrap().amount, 6);
    assert_eq!(tap_sum_for(&events, 0x3008_BD08), 63);
    assert_eq!(tap_sum_for(&events, 0x3009_74A8), 61);
}
