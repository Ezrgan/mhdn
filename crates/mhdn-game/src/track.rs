//! Monster identity across frames. A reused slot or a changed species is a new generation.

use crate::model::{MonsterKey, RawMonster};

#[derive(Debug, Clone)]
struct Known {
    hp_addr: u32,
    slot_ptr: u32,
    species: u16,
    max_hp: u32,
    generation: u32,
    hp: u32,
    frame: u32,
    seen: bool,
    absent: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveMonster {
    pub key: MonsterKey,
    pub hp: u32,
    pub prev_hp: Option<u32>,
    pub prev_frame: Option<u32>,
    pub fresh: bool,
    pub poisoned: bool,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct MonsterTrack {
    known: Vec<Known>,
}

impl MonsterTrack {
    pub(crate) fn clear(&mut self) {
        self.known.clear();
    }

    pub(crate) fn update(&mut self, monsters: &[RawMonster], frame: u32) -> Vec<LiveMonster> {
        for known in &mut self.known {
            known.seen = false;
        }
        let mut live = Vec::with_capacity(monsters.len());
        for raw in monsters {
            live.push(self.touch(raw, frame));
        }
        for known in &mut self.known {
            if !known.seen {
                known.absent = known.absent.saturating_add(1);
            }
        }
        self.known.retain(|known| known.absent < 2);
        live
    }

    fn touch(&mut self, raw: &RawMonster, frame: u32) -> LiveMonster {
        if let Some(index) = self
            .known
            .iter()
            .position(|known| known.hp_addr == raw.hp_addr || known.slot_ptr == raw.slot_ptr)
        {
            let known = &mut self.known[index];
            let identity_changed = known.absent > 0
                || known.species != raw.species
                || known.max_hp != raw.max_hp
                || known.hp_addr != raw.hp_addr;
            // A higher bar (heal, or HP restored after a rejected sample) is a new baseline.
            let hp_rose = !identity_changed && raw.hp > known.hp;
            let rebaseline = identity_changed || hp_rose;
            let prev_hp = (!rebaseline).then_some(known.hp);
            let prev_frame = (!rebaseline).then_some(known.frame);
            if identity_changed {
                known.generation = known.generation.saturating_add(1);
            }
            known.hp_addr = raw.hp_addr;
            known.slot_ptr = raw.slot_ptr;
            known.species = raw.species;
            known.max_hp = raw.max_hp;
            known.hp = raw.hp;
            known.frame = frame;
            known.absent = 0;
            known.seen = true;
            LiveMonster {
                key: key_of(known),
                hp: raw.hp,
                prev_hp,
                prev_frame,
                fresh: rebaseline,
                poisoned: raw.poisoned,
            }
        } else {
            self.known.push(Known {
                hp_addr: raw.hp_addr,
                slot_ptr: raw.slot_ptr,
                species: raw.species,
                max_hp: raw.max_hp,
                generation: 1,
                hp: raw.hp,
                frame,
                seen: true,
                absent: 0,
            });
            let known = self.known.last().expect("just pushed");
            LiveMonster {
                key: key_of(known),
                hp: raw.hp,
                prev_hp: None,
                prev_frame: None,
                fresh: true,
                poisoned: raw.poisoned,
            }
        }
    }
}

fn key_of(known: &Known) -> MonsterKey {
    MonsterKey {
        struct_addr: known.hp_addr,
        species: known.species,
        max_hp: known.max_hp,
        generation: known.generation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Vec3;

    fn raw(addr: u32, species: u16, max_hp: u32, hp: u32) -> RawMonster {
        RawMonster {
            hp_addr: addr,
            slot_ptr: addr,
            hp,
            max_hp,
            species,
            pos: Vec3::new(0.0, 0.0, 0.0),
            visible: true,
            large: max_hp >= 400,
            poisoned: false,
            slot: 0,
        }
    }

    #[test]
    fn a_reused_slot_does_not_emit_a_baseline() {
        let mut track = MonsterTrack::default();
        let first = track.update(&[raw(0x1000, 1, 774, 100)], 1);
        assert!(first[0].fresh);
        assert!(first[0].prev_hp.is_none());
        track.update(&[], 2);
        let again = track.update(&[raw(0x1000, 1, 774, 774)], 3);
        assert!(again[0].fresh);
        assert!(again[0].prev_hp.is_none());
        assert_eq!(again[0].key.generation, 2);
    }

    #[test]
    fn species_change_bumps_generation() {
        let mut track = MonsterTrack::default();
        track.update(&[raw(0x1000, 1, 774, 10)], 1);
        let next = track.update(&[raw(0x1000, 2, 200, 200)], 2);
        assert!(next[0].fresh);
        assert!(next[0].prev_hp.is_none());
        assert_eq!(next[0].key.generation, 2);
        assert_eq!(next[0].key.species, 2);
    }

    #[test]
    fn an_hp_increase_rebaselines_without_bumping_generation() {
        let mut track = MonsterTrack::default();
        track.update(&[raw(0x1000, 1, 774, 100)], 1);
        let up = track.update(&[raw(0x1000, 1, 774, 774)], 2);
        assert!(up[0].fresh);
        assert!(up[0].prev_hp.is_none());
        assert_eq!(up[0].key.generation, 1);
        assert_eq!(up[0].hp, 774);
        let down = track.update(&[raw(0x1000, 1, 774, 760)], 3);
        assert!(!down[0].fresh);
        assert_eq!(down[0].prev_hp, Some(774));
    }
}
