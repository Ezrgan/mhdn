//! Particle pool and easing for damage numbers. No graphics dependencies.

#![forbid(unsafe_code)]

mod ease;
mod magnitude;
mod pool;

pub use ease::{pose, Pose, FADE_MS, HOLD_MS, LIFE_MS, POP_MS};
pub use magnitude::{HitKind, HitStyle, MagnitudeWindow, ORANGE, POISON, WHITE, YELLOW};
pub use pool::{scatter_px, Live, Pool, Spawn, POOL, SCATTER_PX};
