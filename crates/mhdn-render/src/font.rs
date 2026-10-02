//! Damage and HUD text laid out from the glyph atlas.

#![forbid(unsafe_code)]

use crate::atlas::atlas;
use crate::draw::Quad;

/// Debug HUD em. `scale` 2 is about 16 px, close to the old 5×7 bitmap.
const HUD_EM: f32 = 8.0;

pub fn text_quads(text: &str, x: f32, y: f32, scale: f32, color: [f32; 4]) -> Vec<Quad> {
    glyph_quads(text, x, y, (HUD_EM * scale).max(8.0), color)
}

pub fn glyph_quads(text: &str, x: f32, y: f32, px: f32, color: [f32; 4]) -> Vec<Quad> {
    atlas().layout(text, x, y, px, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_is_one_textured_quad() {
        let quads = text_quads("I", 0.0, 0.0, 1.0, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(quads.len(), 1);
        assert!(quads[0].uv[2] > quads[0].uv[0]);
    }

    #[test]
    fn a_space_advances_without_pixels_and_the_next_glyph_follows() {
        let quads = text_quads(" I", 0.0, 0.0, 2.0, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(quads.len(), 1);
        assert!(quads[0].x > 0.0);
    }
}
