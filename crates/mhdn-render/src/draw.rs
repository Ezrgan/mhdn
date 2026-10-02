//! CPU-side quads. One instanced draw covers at most [`INSTANCE_LIMIT`] of them.

#![forbid(unsafe_code)]

/// One instanced draw covers at most this many quads. 200 damage numbers fit.
pub const INSTANCE_LIMIT: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    /// `u0, v0, u1, v1` into the glyph atlas. A zero span is a solid color.
    pub uv: [f32; 4],
}

impl Quad {
    pub fn solid(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Self {
        Self {
            x,
            y,
            w,
            h,
            color,
            uv: [0.0; 4],
        }
    }
}

pub fn batch_count(quads: usize) -> usize {
    if quads == 0 {
        0
    } else {
        quads.div_ceil(INSTANCE_LIMIT)
    }
}

pub fn cross(x: f32, y: f32, arm: f32, thickness: f32, color: [f32; 4]) -> [Quad; 2] {
    let half = thickness * 0.5;
    [
        Quad::solid(x - arm, y - half, arm * 2.0, thickness, color),
        Quad::solid(x - half, y - arm, thickness, arm * 2.0, color),
    ]
}

pub fn stroke_rect(x: f32, y: f32, w: f32, h: f32, thickness: f32, color: [f32; 4]) -> [Quad; 4] {
    [
        Quad::solid(x, y, w, thickness, color),
        Quad::solid(x, y + h - thickness, w, thickness, color),
        Quad::solid(x, y, thickness, h, color),
        Quad::solid(x + w - thickness, y, thickness, h, color),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cross_is_two_quads_centered_on_the_point() {
        let [horizontal, vertical] = cross(10.0, 20.0, 6.0, 2.0, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(horizontal.x, 4.0);
        assert_eq!(horizontal.y, 19.0);
        assert_eq!(horizontal.w, 12.0);
        assert_eq!(vertical.x, 9.0);
        assert_eq!(vertical.h, 12.0);
        assert_eq!(horizontal.uv, [0.0; 4]);
    }

    #[test]
    fn two_hundred_numbers_are_a_single_draw() {
        assert_eq!(batch_count(0), 0);
        assert_eq!(batch_count(200 * 6), 1);
        assert_eq!(batch_count(INSTANCE_LIMIT + 1), 2);
    }
}
