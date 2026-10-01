//! GPU text and debug drawing for the overlay.
//!
//! Quad colors are premultiplied. The surface clears to transparent and blends
//! with `src + dst * (1 - src.a)`.

#![forbid(unsafe_code)]

mod draw;
mod error;
mod font;
mod gpu;
mod schedule;

pub use draw::{cross, stroke_rect, Quad, QUADS_PER_BATCH};
pub use error::RenderError;
pub use font::{text_quads, ADVANCE, GLYPH_H, GLYPH_W};
pub use gpu::Renderer;
pub use schedule::{
    pick_format, premul, select_alpha_mode, FrameClock, POWER_PREFERENCE, PRESENT_MODE,
};

pub const SHADER: &str = include_str!("shader.wgsl");
