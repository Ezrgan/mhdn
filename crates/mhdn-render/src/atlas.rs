//! Raster glyph atlas. Two pixel sizes, one quad per character.
//!
//! Barlow Condensed ExtraBold (OFL) is rasterized with fontdue at startup.
//! MSDF replaces this in a later phase.

#![forbid(unsafe_code)]

use std::sync::OnceLock;

use fontdue::Font;

use crate::draw::Quad;

const FONT: &[u8] = include_bytes!("../../../assets/fonts/BarlowCondensed-ExtraBold.ttf");
const ATLAS_W: u32 = 512;
const ATLAS_H: u32 = 1024;
const SIZES: [f32; 2] = [32.0, 64.0];
const CHARSET: &str = " 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ.,!+KM/%:-";

#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    w: f32,
    h: f32,
    xmin: f32,
    ymin: f32,
    advance: f32,
}

struct Face {
    px: f32,
    ascent: f32,
    glyphs: [Glyph; 128],
    present: [bool; 128],
}

pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    faces: [Face; 2],
}

pub fn atlas() -> &'static GlyphAtlas {
    static ATLAS: OnceLock<GlyphAtlas> = OnceLock::new();
    ATLAS.get_or_init(GlyphAtlas::build)
}

impl GlyphAtlas {
    fn build() -> Self {
        let font = Font::from_bytes(
            FONT,
            fontdue::FontSettings {
                collection_index: 0,
                scale: 40.0,
                load_substitutions: false,
            },
        )
        .expect("Barlow Condensed parses");
        let mut pixels = vec![0u8; (ATLAS_W * ATLAS_H) as usize];
        let mut pen_x = 1u32;
        let mut pen_y = 1u32;
        let mut row_h = 0u32;
        let mut faces = Vec::with_capacity(2);
        for px in SIZES {
            let line = font
                .horizontal_line_metrics(px)
                .expect("horizontal metrics");
            let mut glyphs = [Glyph {
                uv: [0.0; 4],
                w: 0.0,
                h: 0.0,
                xmin: 0.0,
                ymin: 0.0,
                advance: px * 0.4,
            }; 128];
            let mut present = [false; 128];
            for ch in CHARSET.chars() {
                let (metrics, bitmap) = font.rasterize(ch, px);
                let w = metrics.width as u32;
                let h = metrics.height as u32;
                if pen_x + w + 1 >= ATLAS_W {
                    pen_x = 1;
                    pen_y += row_h + 1;
                    row_h = 0;
                }
                assert!(pen_y + h < ATLAS_H, "glyph atlas ran out of room at {px}px");
                blit(
                    &mut pixels,
                    pen_x,
                    pen_y,
                    &bitmap,
                    metrics.width,
                    metrics.height,
                );
                let index = ch as usize;
                if index < glyphs.len() {
                    present[index] = true;
                    glyphs[index] = Glyph {
                        uv: [
                            pen_x as f32 / ATLAS_W as f32,
                            pen_y as f32 / ATLAS_H as f32,
                            (pen_x + w) as f32 / ATLAS_W as f32,
                            (pen_y + h) as f32 / ATLAS_H as f32,
                        ],
                        w: w as f32,
                        h: h as f32,
                        xmin: metrics.xmin as f32,
                        ymin: metrics.ymin as f32,
                        advance: metrics.advance_width,
                    };
                }
                pen_x += w + 1;
                row_h = row_h.max(h);
            }
            faces.push(Face {
                px,
                ascent: line.ascent,
                glyphs,
                present,
            });
        }
        let face_b = faces.pop().expect("large face");
        let face_a = faces.pop().expect("small face");
        Self {
            width: ATLAS_W,
            height: ATLAS_H,
            pixels,
            faces: [face_a, face_b],
        }
    }

    pub fn layout(&self, text: &str, x: f32, y: f32, px: f32, color: [f32; 4]) -> Vec<Quad> {
        let face = self.face_for(px);
        let scale = px / face.px;
        let baseline = y + face.ascent * scale;
        let mut cursor = x;
        let mut quads = Vec::new();
        for ch in text.chars() {
            let glyph = face.glyph(ch);
            if glyph.w > 0.0 && glyph.h > 0.0 {
                let w = glyph.w * scale;
                let h = glyph.h * scale;
                quads.push(Quad {
                    x: cursor + glyph.xmin * scale,
                    y: baseline - (glyph.ymin + glyph.h) * scale,
                    w,
                    h,
                    color,
                    uv: glyph.uv,
                });
            }
            cursor += glyph.advance * scale;
        }
        quads
    }

    fn face_for(&self, px: f32) -> &Face {
        if (px - self.faces[1].px).abs() < (px - self.faces[0].px).abs() {
            &self.faces[1]
        } else {
            &self.faces[0]
        }
    }
}

impl Face {
    fn glyph(&self, ch: char) -> Glyph {
        let index = ch as usize;
        if index < self.glyphs.len() && self.present[index] {
            return self.glyphs[index];
        }
        self.glyphs[b' ' as usize]
    }
}

fn blit(pixels: &mut [u8], x: u32, y: u32, bitmap: &[u8], width: usize, height: usize) {
    for row in 0..height {
        let dst = (y as usize + row) * ATLAS_W as usize + x as usize;
        let src = row * width;
        pixels[dst..dst + width].copy_from_slice(&bitmap[src..src + width]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_and_marks_are_in_the_atlas() {
        let atlas = atlas();
        for ch in "0123456789,.!+KM".chars() {
            let glyph = atlas.faces[0].glyph(ch);
            assert!(glyph.w > 0.0, "{ch} has no bitmap");
            assert!(glyph.uv[2] > glyph.uv[0]);
        }
    }

    #[test]
    fn a_number_is_one_quad_per_glyph_and_advances() {
        let quads = atlas().layout("12", 0.0, 0.0, 32.0, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(quads.len(), 2);
        assert!(quads[1].x > quads[0].x);
        assert!(quads.iter().all(|quad| quad.uv[2] > quad.uv[0]));
    }

    #[test]
    fn two_hundred_hits_fit_in_one_draw() {
        let atlas = atlas();
        let mut count = 0;
        let started = std::time::Instant::now();
        for n in 0..200 {
            count += atlas
                .layout(&format!("{n}"), 0.0, 0.0, 36.0, [1.0; 4])
                .len();
        }
        assert!(count <= crate::draw::INSTANCE_LIMIT);
        assert_eq!(crate::draw::batch_count(count), 1);
        assert!(started.elapsed().as_millis() < 50);
    }
}
