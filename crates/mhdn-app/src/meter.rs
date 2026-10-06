//! Per-monster hunt damage. The corner and the diag log read this.
//!
//! Attacker attribution stays [`Attacker::Unknown`] until a later phase. Player
//! totals count only [`Attacker::You`] and are never filled with that unknown damage.
//!
//! Filter queries and the quest corner modes are covered by tests. The overlay
//! draws [`Meter::screen_lines`] today; phase 5 switches modes from Settings.

#![cfg_attr(not(test), allow(dead_code))]

use mhdn_game::{Attacker, AttackerFilter, DamageKind, MonsterKey, LARGE_MIN_HP};

/// Minimum elapsed time when computing DPS so one hit does not divide by zero.
pub const MIN_DPS_WINDOW_MS: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterHit {
    pub key: MonsterKey,
    pub amount: u32,
    pub attacker: Attacker,
    /// Read from the event. This module does not classify hits.
    pub kind: DamageKind,
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
    /// Player damage only (`Attacker::You` / max HP), capped at 100. Unknown damage stays at 0.
    pub pct_of_max_hp: f32,
}

/// Everything one monster has received, independent of the attacker filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonsterRow {
    pub key: MonsterKey,
    pub total: u32,
    pub poison: u32,
    pub topple: u32,
    pub player: u32,
    pub player_pct: f32,
    pub dps: f32,
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
    poison: u32,
    topple: u32,
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
        match hit.kind {
            DamageKind::Poison => {
                entry.poison = entry.poison.saturating_add(hit.amount);
            }
            DamageKind::Topple => {
                entry.topple = entry.topple.saturating_add(hit.amount);
            }
            DamageKind::Hit | DamageKind::Status | DamageKind::Unknown => {}
        }
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
                    pct_of_max_hp: player_pct(
                        entry.damage[Attacker::You.index()],
                        entry.key.max_hp,
                    ),
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

    /// All damage recorded against every monster. This is not the player's total.
    pub fn received_total(&self) -> u32 {
        self.monsters.iter().map(MonsterEntry::received).sum()
    }

    /// Damage attributed to [`Attacker::You`] only. Unknown hits do not add to this.
    pub fn player_total(&self) -> u32 {
        self.monsters
            .iter()
            .map(|entry| entry.damage[Attacker::You.index()])
            .sum()
    }

    /// Monsters that have received damage, highest total first.
    pub fn rows(&self, now_ms: u64) -> Vec<MonsterRow> {
        let mut rows: Vec<MonsterRow> = self
            .monsters
            .iter()
            .filter_map(|entry| entry.row(now_ms))
            .collect();
        rows.sort_by(|a, b| {
            b.total
                .cmp(&a.total)
                .then_with(|| a.key.species.cmp(&b.key.species))
                .then_with(|| a.key.struct_addr.cmp(&b.key.struct_addr))
        });
        rows
    }

    pub fn row(&self, key: MonsterKey, now_ms: u64) -> Option<MonsterRow> {
        self.monsters
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.row(now_ms))
    }

    /// Corner text. Index 0 is the line nearest the bottom corner.
    ///
    /// Only large monsters (`max_hp >= LARGE_MIN_HP`) are drawn, one name line each,
    /// plus a poison/topple line when those amounts are non-zero. The highest total
    /// sits on the corner; the next large monster is above it. Small monsters stay
    /// in the quest total used as the percentage denominator and do not add a row.
    ///
    /// The big percentage is this monster's received damage over every monster in
    /// the quest. Poison and topple percentages are over that monster's own total.
    /// The atlas has no lowercase. `DERRIBO` is [`DamageKind::Topple`] (the fixed
    /// 100/150 mount hit). There is no YOU line: unknown damage is not the player's.
    pub fn screen_lines(&self, show_total: bool, _show_dps: bool, now_ms: u64) -> Vec<String> {
        if !show_total {
            return Vec::new();
        }
        let rows = self.rows(now_ms);
        let quest_total = self.received_total();
        let large: Vec<&MonsterRow> = rows
            .iter()
            .filter(|row| row.key.max_hp >= LARGE_MIN_HP)
            .collect();
        let totals: Vec<u32> = large.iter().map(|row| row.total).collect();
        let percents = quest_percents(&totals, quest_total);
        let mut lines = Vec::new();
        for (row, pct) in large.into_iter().zip(percents) {
            if let Some(detail) = detail_line(row.poison, row.topple, row.total) {
                lines.push(detail);
            }
            lines.push(name_line(species_label(row.key.species), row.total, pct));
        }
        lines
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
            poison: 0,
            topple: 0,
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
            pct_of_max_hp: player_pct(entry.damage[Attacker::You.index()], entry.key.max_hp),
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

impl MonsterEntry {
    fn received(&self) -> u32 {
        self.damage.iter().copied().sum()
    }

    fn row(&self, now_ms: u64) -> Option<MonsterRow> {
        let total = self.received();
        if total == 0 {
            return None;
        }
        let player = self.damage[Attacker::You.index()];
        let first = (self.first_hit_ms != u64::MAX).then_some(self.first_hit_ms);
        let last = first.map(|_| self.last_hit_ms);
        Some(MonsterRow {
            key: self.key,
            total,
            poison: self.poison,
            topple: self.topple,
            player,
            player_pct: player_pct(player, self.key.max_hp),
            dps: monster_dps(total, first, last, now_ms),
        })
    }
}

/// Diag line for one monster. Addresses match the `dmg mon=` field.
pub fn format_meter_line(addr: u32, total: u32, poison: u32, topple: u32) -> String {
    format!("meter mon=0x{addr:08X} total={total} poison={poison} topple={topple}")
}

/// Names already measured in `docs/RE_NOTES.md` (the species id is the `em` number).
/// Missing species stay unnamed: the corner draws a gap, never the raw id.
pub fn species_name(species: u16) -> Option<&'static str> {
    match species {
        1 => Some("RATHIAN"),
        14 => Some("VELOCIDROME"),
        30 => Some("BULLDROME"),
        _ => None,
    }
}

fn species_label(species: u16) -> &'static str {
    species_name(species).unwrap_or("-")
}

fn name_line(label: &str, total: u32, pct: u32) -> String {
    format!("{label}  {total}  {pct}%")
}

/// Poison and topple, each as a share of this monster. Zero amounts are omitted.
fn detail_line(poison: u32, topple: u32, total: u32) -> Option<String> {
    let mut parts = Vec::new();
    if poison > 0 {
        parts.push(format!("VENENO {poison}  {}%", share_pct(poison, total)));
    }
    if topple > 0 {
        parts.push(format!("DERRIBO {topple}  {}%", share_pct(topple, total)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("   "))
    }
}

fn share_pct(part: u32, total: u32) -> u32 {
    if part == 0 || total == 0 {
        return 0;
    }
    let rounded = (u64::from(part) * 100 + u64::from(total) / 2) / u64::from(total);
    u32::try_from(rounded).unwrap_or(100).min(100)
}

/// Integer percentages of `quest_total`. When `totals` are the whole quest, they sum to 100.
fn quest_percents(totals: &[u32], quest_total: u32) -> Vec<u32> {
    if quest_total == 0 {
        return vec![0; totals.len()];
    }
    let den = u64::from(quest_total);
    let mut floors = Vec::with_capacity(totals.len());
    let mut remainders = Vec::with_capacity(totals.len());
    let mut sum = 0u32;
    for total in totals {
        let num = u64::from(*total) * 100;
        let floor = u32::try_from(num / den).unwrap_or(100);
        sum = sum.saturating_add(floor);
        floors.push(floor);
        remainders.push(num % den);
    }
    let painted: u32 = totals.iter().copied().sum();
    if painted == quest_total {
        let mut leftover = 100u32.saturating_sub(sum);
        let mut order: Vec<usize> = (0..totals.len()).collect();
        order.sort_by(|&a, &b| remainders[b].cmp(&remainders[a]).then(a.cmp(&b)));
        for idx in order {
            if leftover == 0 {
                break;
            }
            floors[idx] = floors[idx].saturating_add(1);
            leftover -= 1;
        }
    }
    floors
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

/// Share of max HP dealt by the player. Unknown damage contributes nothing, and the result never passes 100.
fn player_pct(player: u32, max_hp: u32) -> f32 {
    if player == 0 || max_hp == 0 {
        return 0.0;
    }
    ((player as f32 / max_hp as f32) * 100.0).min(100.0)
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
            kind: DamageKind::Hit,
            at_ms,
        }
    }

    fn with_kind(mut hit: MeterHit, kind: DamageKind) -> MeterHit {
        hit.kind = kind;
        hit
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
        assert_eq!(player_pct(100, 0), 0.0);
        assert!(!player_pct(100, 0).is_nan());
        assert_eq!(player_pct(0, 800), 0.0);
    }

    #[test]
    fn damage_on_one_monster_does_not_enter_the_other_total() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 5_000, 1, 40, Attacker::Unknown, 0));
        meter.add(with_kind(
            hit(0x1000, 7, 5_000, 1, 5, Attacker::Unknown, 100),
            DamageKind::Poison,
        ));
        meter.add(with_kind(
            hit(0x2000, 42, 8_000, 1, 100, Attacker::Unknown, 200),
            DamageKind::Topple,
        ));
        meter.add(with_kind(
            hit(0x1000, 7, 5_000, 1, 40, Attacker::Unknown, 300),
            DamageKind::Status,
        ));
        let rows = meter.rows(1_000);
        let a = rows
            .iter()
            .find(|row| row.key.struct_addr == 0x1000)
            .unwrap();
        let b = rows
            .iter()
            .find(|row| row.key.struct_addr == 0x2000)
            .unwrap();
        assert_eq!(a.total, 85);
        assert_eq!(a.poison, 5);
        assert_eq!(a.topple, 0);
        assert_eq!(b.total, 100);
        assert_eq!(b.poison, 0);
        assert_eq!(b.topple, 100);
        assert_eq!(meter.received_total(), 185);
        assert_eq!(meter.player_total(), 0);
    }

    #[test]
    fn poison_and_topple_accumulate_on_the_event_monster_not_a_session_bag() {
        let mut meter = Meter::new();
        meter.add(with_kind(
            hit(0x1000, 30, 774, 1, 5, Attacker::Unknown, 0),
            DamageKind::Poison,
        ));
        meter.add(with_kind(
            hit(0x1000, 30, 774, 1, 5, Attacker::Unknown, 1_000),
            DamageKind::Poison,
        ));
        meter.add(with_kind(
            hit(0x2000, 31, 774, 1, 150, Attacker::Unknown, 2_000),
            DamageKind::Topple,
        ));
        let rows = meter.rows(2_000);
        assert_eq!(rows.len(), 2);
        let poisoned = rows
            .iter()
            .find(|row| row.key.struct_addr == 0x1000)
            .unwrap();
        let toppled = rows
            .iter()
            .find(|row| row.key.struct_addr == 0x2000)
            .unwrap();
        assert_eq!(poisoned.poison, 10);
        assert_eq!(poisoned.topple, 0);
        assert_eq!(poisoned.total, 10);
        assert_eq!(toppled.topple, 150);
        assert_eq!(toppled.poison, 0);
        assert_eq!(toppled.total, 150);
        let lines = meter.screen_lines(true, false, 2_000);
        assert!(lines.iter().any(|line| line.contains("VENENO 10")));
        assert!(lines.iter().any(|line| line.contains("DERRIBO 150")));
        assert!(!lines.iter().any(|line| line.contains("YOU")));
        assert!(!lines.iter().any(|line| line.contains("JUMP")));
        assert_eq!(
            format_meter_line(0x1000, poisoned.total, poisoned.poison, poisoned.topple),
            "meter mon=0x00001000 total=10 poison=10 topple=0"
        );
        assert_eq!(species_name(30), Some("BULLDROME"));
    }

    #[test]
    fn unknown_damage_does_not_increment_the_player_total() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 800, 1, 400, Attacker::Unknown, 0));
        meter.add(hit(0x2000, 8, 800, 1, 50, Attacker::Unknown, 10));
        assert_eq!(meter.player_total(), 0);
        assert_eq!(meter.received_total(), 450);
        let rows = meter.rows(1_000);
        assert!(rows
            .iter()
            .all(|row| row.player == 0 && row.player_pct == 0.0));
        let lines = meter.screen_lines(true, true, 1_000);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|line| !line.contains("YOU")));
    }

    #[test]
    fn player_percentage_never_exceeds_one_hundred_and_is_not_the_unknown_total() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 7, 100, 1, 250, Attacker::You, 0));
        meter.add(hit(0x1000, 7, 100, 1, 400, Attacker::Unknown, 10));
        let row = meter.row(key(0x1000, 7, 100, 1), 1_000).unwrap();
        assert_eq!(row.player, 250);
        assert_eq!(row.total, 650);
        assert!(row.player_pct <= 100.0);
        assert!((row.player_pct - 100.0).abs() < 0.01);
        assert_eq!(meter.player_total(), 250);
        let lines = meter.screen_lines(true, false, 1_000);
        assert!(
            lines.is_empty(),
            "a bar under {LARGE_MIN_HP} is not painted"
        );
        assert!(lines.iter().all(|line| !line.contains("YOU")));
    }

    #[test]
    fn two_large_monsters_show_names_heaviest_at_the_corner_and_percents_sum_to_100() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 30, 774, 1, 420, Attacker::Unknown, 0));
        meter.add(hit(0x2000, 1, 2_205, 1, 200, Attacker::Unknown, 100));
        let lines = meter.screen_lines(true, false, 1_000);
        assert_eq!(
            lines,
            vec![
                "BULLDROME  420  68%".to_string(),
                "RATHIAN  200  32%".to_string(),
            ]
        );
        assert_eq!(percent_of(&lines[0]) + percent_of(&lines[1]), 100);
        assert!(lines.iter().all(|line| !line.contains("YOU")));
    }

    #[test]
    fn a_small_monster_with_damage_does_not_add_a_row() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 30, 774, 1, 420, Attacker::Unknown, 0));
        meter.add(hit(0x3000, 4107, 80, 1, 100, Attacker::Unknown, 50));
        let lines = meter.screen_lines(true, false, 1_000);
        assert_eq!(lines, vec!["BULLDROME  420  80%".to_string()]);
        assert!(lines.iter().all(|line| !has_id_token(line, 4107)));
        assert_eq!(meter.received_total(), 520);
    }

    #[test]
    fn zero_poison_and_topple_do_not_draw_those_words() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 30, 774, 1, 420, Attacker::Unknown, 0));
        let lines = meter.screen_lines(true, false, 1_000);
        assert_eq!(lines, vec!["BULLDROME  420  100%".to_string()]);
        assert!(lines.iter().all(|line| !line.contains("VENENO")));
        assert!(lines.iter().all(|line| !line.contains("DERRIBO")));
    }

    #[test]
    fn two_of_the_same_species_do_not_show_the_numeric_id() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 30, 774, 1, 420, Attacker::Unknown, 0));
        meter.add(hit(0x2000, 30, 774, 2, 200, Attacker::Unknown, 100));
        let lines = meter.screen_lines(true, false, 1_000);
        assert_eq!(
            lines,
            vec![
                "BULLDROME  420  68%".to_string(),
                "BULLDROME  200  32%".to_string(),
            ]
        );
        assert!(lines.iter().all(|line| !has_id_token(line, 30)));
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("BULLDROME"))
                .count(),
            2
        );
    }

    #[test]
    fn a_species_without_a_measured_name_leaves_a_gap_instead_of_the_id() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 116, 1_200, 1, 420, Attacker::Unknown, 0));
        meter.add(hit(0x2000, 116, 1_200, 2, 258, Attacker::Unknown, 100));
        assert!(species_name(116).is_none());
        let lines = meter.screen_lines(true, false, 1_000);
        assert_eq!(
            lines,
            vec!["-  420  62%".to_string(), "-  258  38%".to_string()]
        );
        assert!(lines.iter().all(|line| !has_id_token(line, 116)));
        assert_eq!(percent_of(&lines[0]) + percent_of(&lines[1]), 100);
    }

    #[test]
    fn poison_and_topple_percentages_are_of_that_monster_and_zero_is_omitted() {
        let mut meter = Meter::new();
        meter.add(hit(0x1000, 30, 774, 1, 280, Attacker::Unknown, 0));
        meter.add(with_kind(
            hit(0x1000, 30, 774, 1, 40, Attacker::Unknown, 100),
            DamageKind::Poison,
        ));
        meter.add(with_kind(
            hit(0x1000, 30, 774, 1, 100, Attacker::Unknown, 200),
            DamageKind::Topple,
        ));
        let lines = meter.screen_lines(true, false, 1_000);
        assert_eq!(
            lines,
            vec![
                "VENENO 40  10%   DERRIBO 100  24%".to_string(),
                "BULLDROME  420  100%".to_string(),
            ]
        );
        assert!(lines.iter().all(|line| !has_id_token(line, 30)));
    }

    fn has_id_token(line: &str, species: u16) -> bool {
        let id = species.to_string();
        line.split(|ch: char| !ch.is_ascii_digit())
            .any(|token| token == id)
    }

    fn percent_of(line: &str) -> u32 {
        let pct = line.rsplit_once(' ').unwrap().1;
        pct.trim_end_matches('%').parse().unwrap()
    }
}
