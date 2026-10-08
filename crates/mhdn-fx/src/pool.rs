//! Fixed pool of floating damage numbers. Spawning never allocates.

#![forbid(unsafe_code)]

use crate::ease::{pose, Pose, RISE_MAX, RISE_MIN};

pub const POOL: usize = 256;
pub const SCATTER_PX: f32 = 12.0;

#[derive(Debug, Clone, Copy)]
pub struct Spawn {
    pub world: [f32; 3],
    pub rgb: [f32; 3],
    pub mag_scale: f32,
    pub amount: u32,
    pub seed: u32,
    /// Attacker index from the game crate. The pool does not interpret it.
    pub tag: u8,
    /// Residual HP drop, as opposed to a hit-by-hit tap.
    pub grouped: bool,
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    alive: bool,
    age_ms: f32,
    speed: f32,
    scatter: f32,
    world: [f32; 3],
    rgb: [f32; 3],
    mag_scale: f32,
    text: [u8; 8],
    text_len: u8,
    born: u32,
    tag: u8,
    grouped: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            alive: false,
            age_ms: 0.0,
            speed: 0.0,
            scatter: 0.0,
            world: [0.0; 3],
            rgb: [1.0; 3],
            mag_scale: 1.0,
            text: [0; 8],
            text_len: 0,
            born: 0,
            tag: 0,
            grouped: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Live<'a> {
    pub world: [f32; 3],
    pub scatter: f32,
    pub rgb: [f32; 3],
    pub text: &'a str,
    pub pose: Pose,
    pub mag_scale: f32,
    pub tag: u8,
    pub grouped: bool,
}

pub struct Pool {
    slots: [Slot; POOL],
    next_born: u32,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl Pool {
    pub fn new() -> Self {
        Self {
            slots: [Slot::default(); POOL],
            next_born: 1,
        }
    }

    pub fn clear(&mut self) {
        self.slots = [Slot::default(); POOL];
    }

    pub fn alive_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.alive).count()
    }

    pub fn spawn(&mut self, spawn: Spawn) {
        let index = self.free_or_oldest();
        let born = self.next_born;
        self.next_born = self.next_born.wrapping_add(1);
        let mut text = [0u8; 8];
        let text_len = format_u32(spawn.amount, &mut text);
        self.slots[index] = Slot {
            alive: true,
            age_ms: 0.0,
            speed: RISE_MIN + unit(spawn.seed) * (RISE_MAX - RISE_MIN),
            scatter: (unit(spawn.seed.wrapping_mul(3)) * 2.0 - 1.0) * SCATTER_PX,
            world: spawn.world,
            rgb: spawn.rgb,
            mag_scale: spawn.mag_scale,
            text,
            text_len,
            born,
            tag: spawn.tag,
            grouped: spawn.grouped,
        };
    }

    pub fn tick(&mut self, dt_ms: f32) {
        if dt_ms <= 0.0 {
            return;
        }
        for slot in &mut self.slots {
            if !slot.alive {
                continue;
            }
            slot.age_ms += dt_ms;
            if !pose(slot.age_ms, slot.speed).alive {
                slot.alive = false;
            }
        }
    }

    pub fn live(&self) -> impl Iterator<Item = Live<'_>> {
        self.slots.iter().filter(|slot| slot.alive).map(|slot| {
            let text = std::str::from_utf8(&slot.text[..slot.text_len as usize]).unwrap_or("0");
            Live {
                world: slot.world,
                scatter: slot.scatter,
                rgb: slot.rgb,
                text,
                pose: pose(slot.age_ms, slot.speed),
                mag_scale: slot.mag_scale,
                tag: slot.tag,
                grouped: slot.grouped,
            }
        })
    }

    fn free_or_oldest(&self) -> usize {
        if let Some(index) = self.slots.iter().position(|slot| !slot.alive) {
            return index;
        }
        self.slots
            .iter()
            .enumerate()
            .min_by_key(|(_, slot)| slot.born)
            .map(|(index, _)| index)
            .unwrap_or(0)
    }
}

pub fn scatter_px(seed: u32) -> f32 {
    (unit(seed) * 2.0 - 1.0) * SCATTER_PX
}

fn unit(seed: u32) -> f32 {
    let hashed = seed.wrapping_mul(0x9E37_79B1);
    (hashed >> 8) as f32 / (1u32 << 24) as f32
}

fn format_u32(mut value: u32, buf: &mut [u8; 8]) -> u8 {
    if value == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 8];
    let mut len = 0;
    while value > 0 && len < tmp.len() {
        tmp[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for (dst, src) in buf.iter_mut().zip(tmp[..len].iter().rev()) {
        *dst = *src;
    }
    len as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ease::LIFE_MS;

    fn hit(amount: u32, seed: u32) -> Spawn {
        Spawn {
            world: [1.0, 2.0, 3.0],
            rgb: [1.0, 1.0, 1.0],
            mag_scale: 1.0,
            amount,
            seed,
            tag: 0,
            grouped: false,
        }
    }

    #[test]
    fn a_spawn_shows_the_amount_and_stays_inside_the_scatter() {
        let mut pool = Pool::new();
        pool.spawn(hit(128, 7));
        let live: Vec<_> = pool.live().collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].text, "128");
        assert!(live[0].scatter.abs() <= SCATTER_PX);
        assert!((live[0].pose.scale - 1.35).abs() < 0.02);
    }

    #[test]
    fn the_pool_drops_the_oldest_when_it_is_full() {
        let mut pool = Pool::new();
        for n in 0..POOL as u32 {
            pool.spawn(hit(n + 1, n));
        }
        assert_eq!(pool.alive_count(), POOL);
        pool.spawn(hit(999, 1));
        assert_eq!(pool.alive_count(), POOL);
        let texts: Vec<_> = pool.live().map(|live| live.text.to_string()).collect();
        assert!(texts.contains(&"999".to_string()));
        assert!(!texts.contains(&"1".to_string()));
    }

    #[test]
    fn ticking_past_the_lifetime_clears_the_slot() {
        let mut pool = Pool::new();
        pool.spawn(hit(4, 1));
        pool.tick(LIFE_MS);
        assert_eq!(pool.alive_count(), 0);
    }

    #[test]
    fn clear_drops_every_number() {
        let mut pool = Pool::new();
        pool.spawn(hit(4, 1));
        pool.clear();
        assert_eq!(pool.alive_count(), 0);
    }
}
