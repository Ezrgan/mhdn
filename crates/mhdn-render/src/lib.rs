//! GPU text and debug drawing for the overlay.
//!
//! Quad colors are premultiplied. The surface clears to transparent and blends
//! with `src + dst * (1 - src.a)`.

#![forbid(unsafe_code)]

mod atlas;
mod draw;
mod error;
mod font;
mod gpu;
mod schedule;

pub use draw::{batch_count, cross, stroke_rect, Quad, INSTANCE_LIMIT};
pub use error::RenderError;
pub use font::{glyph_quads, text_quads};
pub use gpu::{required_device_limits, PresentStats, Renderer};
pub use schedule::{
    pick_format, premul, select_alpha_mode, FrameClock, POWER_PREFERENCE, PRESENT_MODE,
};

pub const SHADER: &str = include_str!("shader.wgsl");
