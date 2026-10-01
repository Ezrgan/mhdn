//! CPU-side quads. One draw covers at most [`QUADS_PER_BATCH`] of them.

#![forbid(unsafe_code)]

/// Uniform batch size. 256 × 32 B plus the screen vector stays under 16 KB.
pub const QUADS_PER_BATCH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
}

pub fn cross(x: f32, y: f32, arm: f32, thickness: f32, color: [f32; 4]) -> [Quad; 2] {
    let half = thickness * 0.5;
    [
        Quad {
            x: x - arm,
            y: y - half,
            w: arm * 2.0,
            h: thickness,
            color,
        },
        Quad {
            x: x - half,
            y: y - arm,
            w: thickness,
            h: arm * 2.0,
            color,
        },
    ]
}

pub fn stroke_rect(x: f32, y: f32, w: f32, h: f32, thickness: f32, color: [f32; 4]) -> [Quad; 4] {
    [
        Quad {
            x,
            y,
            w,
            h: thickness,
            color,
        },
        Quad {
            x,
            y: y + h - thickness,
            w,
            h: thickness,
            color,
        },
        Quad {
            x,
            y,
            w: thickness,
            h,
            color,
        },
        Quad {
            x: x + w - thickness,
            y,
            w: thickness,
            h,
            color,
        },
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
    }
}
