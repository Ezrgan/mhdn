//! Drag the top-screen corners, or nudge the whole rect with the arrow keys.

#![forbid(unsafe_code)]

use mhdn_proj::ScreenRect;
use mhdn_render::{premul, Quad};
use winit::keyboard::{Key, NamedKey};

use crate::config::{apply_calibration, LayoutCalibration};

const HIT_PX: f32 = 12.0;
const NUDGE_PX: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalibCommand {
    Toggle,
    Save,
    Nudge(f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    corner: Corner,
    last_x: f32,
    last_y: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Calibrator {
    pub active: bool,
    pub calibration: LayoutCalibration,
    drag: Option<Drag>,
}

impl Calibrator {
    pub fn apply(&self, rect: ScreenRect) -> ScreenRect {
        apply_calibration(rect, self.calibration)
    }

    pub fn pointer_down(&mut self, rect: ScreenRect, x: f32, y: f32) {
        let Some(corner) = hit_corner(rect, x, y) else {
            return;
        };
        self.drag = Some(Drag {
            corner,
            last_x: x,
            last_y: y,
        });
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        let Some(drag) = self.drag else {
            return;
        };
        let dx = x - drag.last_x;
        let dy = y - drag.last_y;
        drag_corner(&mut self.calibration, drag.corner, dx, dy);
        self.drag = Some(Drag {
            corner: drag.corner,
            last_x: x,
            last_y: y,
        });
    }

    pub fn pointer_up(&mut self) {
        self.drag = None;
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) {
        self.calibration.nudge_x += dx;
        self.calibration.nudge_y += dy;
    }

    pub fn command(&mut self, command: CalibCommand) {
        match command {
            CalibCommand::Toggle => {
                self.active = !self.active;
                self.drag = None;
            }
            CalibCommand::Save => {}
            CalibCommand::Nudge(dx, dy) if self.active => self.nudge(dx, dy),
            CalibCommand::Nudge(_, _) => {}
        }
    }
}

pub fn command_from_key(key: &Key) -> Option<CalibCommand> {
    Some(match key {
        Key::Character(text) if text.eq_ignore_ascii_case("c") => CalibCommand::Toggle,
        Key::Character(text) if text.eq_ignore_ascii_case("s") => CalibCommand::Save,
        Key::Named(NamedKey::Enter) => CalibCommand::Save,
        Key::Named(NamedKey::ArrowLeft) => CalibCommand::Nudge(-NUDGE_PX, 0.0),
        Key::Named(NamedKey::ArrowRight) => CalibCommand::Nudge(NUDGE_PX, 0.0),
        Key::Named(NamedKey::ArrowUp) => CalibCommand::Nudge(0.0, -NUDGE_PX),
        Key::Named(NamedKey::ArrowDown) => CalibCommand::Nudge(0.0, NUDGE_PX),
        _ => return None,
    })
}

pub fn hit_corner(rect: ScreenRect, x: f32, y: f32) -> Option<Corner> {
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    let corners = [
        (Corner::TopLeft, rect.x, rect.y),
        (Corner::TopRight, right, rect.y),
        (Corner::BottomLeft, rect.x, bottom),
        (Corner::BottomRight, right, bottom),
    ];
    corners
        .into_iter()
        .find(|(_, cx, cy)| dist2(x, y, *cx, *cy) <= HIT_PX * HIT_PX)
        .map(|(corner, _, _)| corner)
}

pub fn handles(rect: ScreenRect) -> Vec<Quad> {
    let color = premul([1.0, 0.85, 0.2], 0.95);
    let size = 8.0;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    [rect.x, right, rect.x, right]
        .into_iter()
        .zip([rect.y, rect.y, bottom, bottom])
        .map(|(x, y)| Quad::solid(x - size * 0.5, y - size * 0.5, size, size, color))
        .collect()
}

fn drag_corner(cal: &mut LayoutCalibration, corner: Corner, dx: f32, dy: f32) {
    match corner {
        Corner::TopLeft => {
            cal.left += dx;
            cal.top += dy;
        }
        Corner::TopRight => {
            cal.right -= dx;
            cal.top += dy;
        }
        Corner::BottomLeft => {
            cal.left += dx;
            cal.bottom -= dy;
        }
        Corner::BottomRight => {
            cal.right -= dx;
            cal.bottom -= dy;
        }
    }
}

fn dist2(x: f32, y: f32, cx: f32, cy: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> ScreenRect {
        ScreenRect::new(0.0, 0.0, 400.0, 240.0)
    }

    #[test]
    fn dragging_a_corner_changes_only_that_corner() {
        let mut calibrator = Calibrator::default();
        calibrator.pointer_down(rect(), 1.0, 1.0);
        calibrator.pointer_move(11.0, 6.0);
        calibrator.pointer_up();
        assert_eq!(calibrator.calibration.left, 10.0);
        assert_eq!(calibrator.calibration.top, 5.0);
        assert_eq!(calibrator.calibration.right, 0.0);
        assert_eq!(calibrator.calibration.bottom, 0.0);

        calibrator.pointer_down(calibrator.apply(rect()), 410.0, 240.0);
        calibrator.pointer_move(406.0, 243.0);
        assert_eq!(calibrator.calibration.right, 4.0);
        assert_eq!(calibrator.calibration.bottom, -3.0);
    }

    #[test]
    fn arrows_nudge_one_pixel_while_calibration_is_active() {
        let mut calibrator = Calibrator::default();
        let left = command_from_key(&Key::Named(NamedKey::ArrowLeft)).unwrap();
        calibrator.command(left);
        assert_eq!(calibrator.calibration.nudge_x, 0.0);
        calibrator.command(CalibCommand::Toggle);
        calibrator.command(left);
        calibrator.command(command_from_key(&Key::Named(NamedKey::ArrowDown)).unwrap());
        assert_eq!(calibrator.calibration.nudge_x, -1.0);
        assert_eq!(calibrator.calibration.nudge_y, 1.0);
        assert!(calibrator.active);
    }

    #[test]
    fn letters_map_to_toggle_and_save() {
        assert_eq!(
            command_from_key(&Key::Character("c".into())),
            Some(CalibCommand::Toggle)
        );
        assert_eq!(
            command_from_key(&Key::Character("S".into())),
            Some(CalibCommand::Save)
        );
        assert_eq!(
            command_from_key(&Key::Named(NamedKey::Enter)),
            Some(CalibCommand::Save)
        );
    }
}
