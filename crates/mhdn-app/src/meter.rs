//! Per-monster hunt damage meter (logic only; wired in a later phase).

#![allow(
    dead_code,
    reason = "Phase 4 model only; run.rs and corner UI connect in a later phase."
)]

use mhdn_game::{Attacker, AttackerFilter, MonsterKey};

/// Minimum elapsed time when computing DPS so one hit does not divide by zero.
pub const MIN_DPS_WINDOW_MS: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterHit {
    pub key: MonsterKey,
    pub amount: u32,
    pub attacker: Attacker,
    /// Monotonic milliseconds from the caller (same clock for the whole quest).
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerMode {
    CurrentMonster,
    WholeQuest,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonsterMeterStats {
    pub key: MonsterKey,
    pub damage: u32,
    pub hits: u32,
    pub dps: f32,
    pub pct_of_max_hp: f32,
}

#[derive(Debug, Default)]
pub struct Meter {
    monsters: Vec<MonsterEntry>,
}

#[derive(Debug, Clone)]
struct MonsterEntry {
    key: MonsterKey,
    damage: [u32; Attacker::ALL.len()],
    hits: [u32; Attacker::ALL.len()],
    first_hit_ms: u64,
    last_hit_ms: u64,
    first_hit_at: [Option<u64>; Attacker::ALL.len()],
    last_hit_at: [Option<u64>; Attacker::ALL.len()],
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, hit: MeterHit) {
        let idx = self.entry_index(hit.key);
        let entry = &mut self.monsters[idx];
        let a = hit.attacker.index();
        entry.damage[a] = entry.damage[a].saturating_add(hit.amount);
        entry.hits[a] = entry.hits[a].saturating_add(1);
        if entry.first_hit_at[a].is_none() {
            entry.first_hit_at[a] = Some(hit.at_ms);
        }
        entry.last_hit_at[a] = Some(hit.at_ms);
        if entry.first_hit_ms > hit.at_ms {
            entry.first_hit_ms = hit.at_ms;
        }
        if entry.last_hit_ms < hit.at_ms {
            entry.last_hit_ms = hit.at_ms;
        }
    }

    /// Clears all quest damage. Call only when a new quest starts — area transitions within the same quest must not reset.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Monster most recently hit by an attacker allowed in `filter`.
    pub fn current(&self, filter: AttackerFilter, now_ms: u64) -> Option<MonsterMeterStats> {
        let key = self.most_recent_filtered_key(filter)?;
        self.stats_for_key(key, filter, now_ms)
    }

    /// All monsters with filtered damage, highest damage first.
    pub fn per_monster(&self, filter: AttackerFilter, now_ms: u64) -> Vec<MonsterMeterStats> {
        let mut rows: Vec<MonsterMeterStats> = self
            .monsters
            .iter()
            .filter_map(|entry| {
                let damage = filtered_sum(&entry.damage, filter);
                if damage == 0 {
                    return None;
                }
                let (first, last) = filtered_time_bounds(entry, filter);
                Some(MonsterMeterStats {
                    key: entry.key,
                    damage,
                    hits: filtered_hits(&entry.hits, filter),
                    dps: monster_dps(damage, first, last, now_ms),
                    pct_of_max_hp: pct_of_max_hp(damage, entry.key.max_hp),
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            b.damage
                .cmp(&a.damage)
                .then_with(|| a.key.species.cmp(&b.key.species))
        });
        rows
    }

    pub fn quest_total(&self, filter: AttackerFilter) -> u32 {
        self.monsters
            .iter()
            .map(|entry| filtered_sum(&entry.damage, filter))
            .sum()
    }

    pub fn session_dps(&self, filter: AttackerFilter, now_ms: u64) -> f32 {
        let total = self.quest_total(filter);
        if total == 0 {
            return 0.0;
        }
        let (Some(first), Some(last)) = self.session_bounds(filter) else {
            return 0.0;
        };
        dps_from_window(total, first, last, now_ms)
    }

    fn entry_index(&mut self, key: MonsterKey) -> usize {
        if let Some(idx) = self.monsters.iter().position(|entry| entry.key == key) {
            return idx;
        }
        self.monsters.push(MonsterEntry {
            key,
            damage: [0; Attacker::ALL.len()],
            hits: [0; Attacker::ALL.len()],
            first_hit_ms: u64::MAX,
            last_hit_ms: 0,
            first_hit_at: [None; Attacker::ALL.len()],
            last_hit_at: [None; Attacker::ALL.len()],
        });
        self.monsters.len() - 1
    }

    fn most_recent_filtered_key(&self, filter: AttackerFilter) -> Option<MonsterKey> {
        self.monsters
            .iter()
            .flat_map(|entry| {
                Attacker::ALL.iter().filter_map(|attacker| {
                    if !filter.allows(*attacker) {
                        return None;
                    }
                    let at = entry.last_hit_at[attacker.index()]?;
                    Some((entry.key, at))
                })
            })
            .max_by_key(|(_, at_ms)| *at_ms)
            .map(|(key, _)| key)
    }

    fn stats_for_key(
        &self,
        key: MonsterKey,
        filter: AttackerFilter,
        now_ms: u64,
    ) -> Option<MonsterMeterStats> {
        let entry = self.monsters.iter().find(|entry| entry.key == key)?;
        let damage = filtered_sum(&entry.damage, filter);
        if damage == 0 {
            return None;
        }
        let (first, last) = filtered_time_bounds(entry, filter);
        Some(MonsterMeterStats {
            key: entry.key,
            damage,
            hits: filtered_hits(&entry.hits, filter),
            dps: monster_dps(damage, first, last, now_ms),
            pct_of_max_hp: pct_of_max_hp(damage, entry.key.max_hp),
        })
    }

    fn session_bounds(&self, filter: AttackerFilter) -> (Option<u64>, Option<u64>) {
        let mut first = None;
        let mut last = None;
        for entry in &self.monsters {
            if filtered_sum(&entry.damage, filter) == 0 {
                continue;
            }
            let (entry_first, entry_last) = filtered_time_bounds(entry, filter);
            if let Some(entry_first) = entry_first {
                first = Some(first.map_or(entry_first, |f: u64| f.min(entry_first)));
            }
            if let Some(entry_last) = entry_last {
                last = Some(last.map_or(entry_last, |l: u64| l.max(entry_last)));
            }
        }
        (first, last)
    }
}

/// Corner overlay text in English. Empty when there is nothing to show.
pub fn corner_text(mode: CornerMode, meter: &Meter, filter: AttackerFilter, now_ms: u64) -> String {
    match mode {
        CornerMode::Session => session_corner_text(meter, filter, now_ms),
        CornerMode::CurrentMonster => current_monster_corner_text(meter, filter, now_ms),
        CornerMode::WholeQuest => whole_quest_corner_text(meter, filter, now_ms),
    }
}

fn session_corner_text(meter: &Meter, filter: AttackerFilter, now_ms: u64) -> String {
    let total = meter.quest_total(filter);
    if total == 0 {
        return String::new();
    }
    let dps = meter.session_dps(filter, now_ms);
    format!("DMG {total}  DPS {dps:.0}")
}

fn current_monster_corner_text(meter: &Meter, filter: AttackerFilter, now_ms: u64) -> String {
    let Some(stats) = meter.current(filter, now_ms) else {
        return String::new();
    };
    format!(
        "Monster {}\nDMG {}  DPS {:.0}\n{:.1}% HP",
        stats.key.species, stats.damage, stats.dps, stats.pct_of_max_hp
    )
}

fn whole_quest_corner_text(meter: &Meter, filter: AttackerFilter, now_ms: u64) -> String {
    let rows = meter.per_monster(filter, now_ms);
    if rows.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| {
            format!(
                "Monster {}: DMG {}  DPS {:.0}  {:.1}% HP",
                row.key.species, row.damage, row.dps, row.pct_of_max_hp
            )
        })
        .collect();
    let total = meter.quest_total(filter);
    lines.push(format!("Total DMG {total}"));
    lines.join("\n")
}

fn filtered_sum(damage: &[u32; Attacker::ALL.len()], filter: AttackerFilter) -> u32 {
    Attacker::ALL
        .iter()
        .filter(|attacker| filter.allows(**attacker))
        .map(|attacker| damage[attacker.index()])
        .sum()
}

fn filtered_time_bounds(
    entry: &MonsterEntry,
    filter: AttackerFilter,
) -> (Option<u64>, Option<u64>) {
    let mut first = None;
    let mut last = None;
    for attacker in Attacker::ALL {
        if !filter.allows(attacker) {
            continue;
        }
        let idx = attacker.index();
        if let Some(at) = entry.first_hit_at[idx] {
            first = Some(first.map_or(at, |f: u64| f.min(at)));
        }
        if let Some(at) = entry.last_hit_at[idx] {
            last = Some(last.map_or(at, |l: u64| l.max(at)));
        }
    }
    (first, last)
}

fn filtered_hits(hits: &[u32; Attacker::ALL.len()], filter: AttackerFilter) -> u32 {
    Attacker::ALL
        .iter()
        .filter(|attacker| filter.allows(**attacker))
        .map(|attacker| hits[attacker.index()])
        .sum()
}

fn pct_of_max_hp(damage: u32, max_hp: u32) -> f32 {
    if max_hp == 0 {
        return 0.0;
    }
    (damage as f32 / max_hp as f32) * 100.0
}

fn dps_window_ms(first_ms: u64, last_ms: u64, now_ms: u64) -> u64 {
    let end = now_ms.max(last_ms);
    let span = end.saturating_sub(first_ms);
    span.max(MIN_DPS_WINDOW_MS)
}

fn dps_from_window(damage: u32, first_ms: u64, last_ms: u64, now_ms: u64) -> f32 {
    let window_s = dps_window_ms(first_ms, last_ms, now_ms) as f32 / 1000.0;
    damage as f32 / window_s
}

fn monster_dps(damage: u32, first_ms: Option<u64>, last_ms: Option<u64>, now_ms: u64) -> f32 {
    let (Some(first), Some(last)) = (first_ms, last_ms) else {
        return 0.0;
    };
    dps_from_window(damage, first, last, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_game::AttackerFilter;

    fn key(addr: u32, species: u16, max_hp: u32, generation: u32) -> MonsterKey {
        MonsterKey {
            struct_addr: addr,
            species,
            max_hp,
            generation,
        }
    }

    fn hit(
        addr: u32,
        species: u16,
        max_hp: u32,
        generation: u32,
        amount: u32,
        attacker: Attacker,
        at_ms: u64,
    ) -> MeterHit {
        MeterHit {
            key: key(addr, species, max_hp, generation),
            amount,
            attacker,
            at_ms,
        }
    }

    #[test]
    fn two_monsters_alternate_and_current_follows_last_hit() {
        let mut meter = Meter::new();
        let a = key(0x1000, 7, 10_000, 1);
        let b = key(0x2000, 42, 8_000, 1);
        meter.add(hit(0x1000, 7, 10_000, 1, 100, Attacker::You, 0));
        meter.add(hit(0x2000, 42, 8_000, 1, 50, Attacker::You, 100));
        meter.add(hit(0x1000, 7, 10_000, 1, 25, Attacker::You, 200));
        let filter = AttackerFilter::default();
        assert_eq!(meter.current(filter, 500).unwrap().key, a);
        meter.add(hit(0x2000, 42, 8_000, 1, 10, Attacker::You, 300));
        assert_eq!(meter.current(filter, 500).unwrap().key, b);
        let rows = meter.per_monster(filter, 500);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, a);
        assert_eq!(rows[0].damage, 125);
        assert_eq!(rows[1].key, b);
        assert_eq!(rows[1].damage, 60);
    }

    #[test]
    fn same_slot_and_generation_after_leave_keeps_one_entry() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 200, Attacker::You, 1_000));
        meter.add(hit(0x1000, 7, 5_000, 1, 300, Attacker::You, 60_000));
        let filter = AttackerFilter::default();
        assert_eq!(meter.per_monster(filter, 60_000).len(), 1);
        assert_eq!(meter.quest_total(filter), 500);
    }

    #[test]
    fn new_generation_in_same_slot_is_separate() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 100, Attacker::You, 0));
        meter.add(hit(0x1000, 7, 5_000, 2, 400, Attacker::You, 100));
        let filter = AttackerFilter::default();
        assert_eq!(meter.per_monster(filter, 200).len(), 2);
        assert_eq!(meter.quest_total(filter), 500);
    }

    #[test]
    fn excluding_felyne_drops_its_damage_from_totals_dps_and_pct() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 1_000, 1, 200, Attacker::You, 0));
        meter.add(hit(0x1000, 7, 1_000, 1, 300, Attacker::YourFelyne, 500));
        let filter = AttackerFilter::default().with(Attacker::YourFelyne, false);
        let stats = meter.current(filter, 0).unwrap();
        assert_eq!(stats.damage, 200);
        assert_eq!(stats.hits, 1);
        assert!((stats.pct_of_max_hp - 20.0).abs() < 0.01);
        assert!((stats.dps - 200.0).abs() < 0.01);
        assert_eq!(meter.quest_total(filter), 200);
    }

    #[test]
    fn pct_of_max_hp_is_correct_and_not_above_one_hundred_for_solo_damage() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 800, 1, 400, Attacker::You, 0));
        let filter = AttackerFilter::default();
        let pct = meter.current(filter, 1_000).unwrap().pct_of_max_hp;
        assert!((pct - 50.0).abs() < 0.01);
        assert!(pct <= 100.0);
        meter.add(hit(0x1000, 7, 800, 1, 400, Attacker::You, 500));
        let pct = meter.current(filter, 2_000).unwrap().pct_of_max_hp;
        assert!((pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn single_hit_dps_uses_minimum_window() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 500, Attacker::You, 10_000));
        let dps = meter
            .current(AttackerFilter::default(), 10_000)
            .unwrap()
            .dps;
        assert!((dps - 500.0).abs() < 0.01);
    }

    #[test]
    fn dps_across_a_pause_extends_with_now_ms() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 1_000, Attacker::You, 0));
        meter.add(hit(0x1000, 7, 5_000, 1, 1_000, Attacker::You, 1_000));
        let active = meter.current(AttackerFilter::default(), 1_000).unwrap().dps;
        assert!((active - 2_000.0).abs() < 0.01);
        let after_pause = meter
            .current(AttackerFilter::default(), 11_000)
            .unwrap()
            .dps;
        assert!(after_pause < active);
        assert!((after_pause - 2000.0 / 11.0).abs() < 0.01);
    }

    #[test]
    fn reset_clears_all_state() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 100, Attacker::You, 0));
        meter.reset();
        assert_eq!(meter.quest_total(AttackerFilter::default()), 0);
        assert!(meter.current(AttackerFilter::default(), 0).is_none());
        assert!(meter.per_monster(AttackerFilter::default(), 0).is_empty());
        assert_eq!(
            corner_text(CornerMode::Session, &meter, AttackerFilter::default(), 0),
            ""
        );
    }

    #[test]
    fn corner_text_session_matches_recount_format() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 40, Attacker::You, 0));
        meter.add(hit(0x1000, 7, 5_000, 1, 60, Attacker::You, 2_000));
        let text = corner_text(
            CornerMode::Session,
            &meter,
            AttackerFilter::default(),
            2_000,
        );
        assert_eq!(text, "DMG 100  DPS 50");
    }

    #[test]
    fn corner_text_current_monster_lines() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 1_000, 1, 250, Attacker::You, 0));
        let text = corner_text(
            CornerMode::CurrentMonster,
            &meter,
            AttackerFilter::default(),
            1_000,
        );
        assert_eq!(text, "Monster 7\nDMG 250  DPS 250\n25.0% HP");
    }

    #[test]
    fn corner_text_whole_quest_lists_monsters_and_total() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 1_000, 1, 100, Attacker::You, 0));
        meter.add(hit(0x2000, 42, 2_000, 1, 200, Attacker::You, 500));
        let text = corner_text(
            CornerMode::WholeQuest,
            &meter,
            AttackerFilter::default(),
            2_000,
        );
        assert_eq!(
            text,
            "Monster 42: DMG 200  DPS 133  10.0% HP\nMonster 7: DMG 100  DPS 50  10.0% HP\nTotal DMG 300"
        );
    }

    #[test]
    fn max_hp_zero_yields_zero_pct_not_nan() {
        assert_eq!(pct_of_max_hp(100, 0), 0.0);
        assert!(!pct_of_max_hp(100, 0).is_nan());
    }
}
