//! The settings window: a normal desktop window next to the click-through overlay.
//!
//! It edits the overlay's `StyleConfig` in place, so the overlay picks every change up on
//! its next frame. The window never owns the session. It only reports Start and Stop.

#![forbid(unsafe_code)]

use std::num::NonZeroU32;
use std::sync::Arc;

use egui::{Color32, RichText, ViewportId};
use egui_wgpu::{RendererOptions, WgpuConfiguration};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::config::{NumberAnchor, StyleConfig};
use crate::settings::{
    Category, CornerSettings, NumberSettings, CORNER_PT_RANGE, DEFAULT_NUMBER_PX, NUMBER_PX_RANGE,
};

/// Game and Azahar facts, copied from `docs/SETUP_AZAHAR.md` and `docs/GUIA.md`.
pub const GAME_REQUIREMENTS: [&str; 3] = [
    "Monster Hunter XX Japan",
    "Update v1.4 installed",
    "Title ID 0004000000197100",
];

/// The Azahar log filter, exactly as the docs give it.
pub const LOG_FILTER: &str = "*:Info RPC_Server:Warning";

/// Numbered steps, in the order the docs give them.
pub const SETUP_STEPS: [&str; 5] = [
    "Open Azahar settings.",
    "Enable \"Enable RPC server\".",
    "Set the log filter exactly to the line below. Without it, RPC logging can cause high CPU usage and stutter.",
    "Turn stereoscopic 3D off.",
    "Restart Azahar after changing these settings, then start MHXX and load your save.",
];

/// What the user asked the dashboard to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
}

/// What the dashboard shows. The overlay owns the truth.
#[derive(Debug, Clone, Copy)]
pub struct Dashboard<'a> {
    pub status: &'a str,
    pub running: bool,
    /// False when no game profile loaded, so there is nothing to start.
    pub can_start: bool,
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// A control changed this frame.
    pub changed: bool,
    pub action: Option<Action>,
    /// The edits settled (no mouse button held), so the config file should be written.
    pub save: bool,
}

pub enum WindowFlow {
    Nothing,
    Redraw,
    CloseRequested,
}

pub struct SettingsWindow {
    window: Arc<Window>,
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    unsaved: bool,
}

impl SettingsWindow {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let attributes = WindowAttributes::default()
            .with_title("mhdn")
            .with_decorations(true)
            .with_transparent(false)
            .with_resizable(true)
            .with_inner_size(LogicalSize::new(520.0, 700.0))
            .with_min_inner_size(LogicalSize::new(420.0, 420.0))
            .with_visible(true);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|err| err.to_string())?,
        );
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(
            ctx.clone(),
            ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(
            ctx.clone(),
            WgpuConfiguration::default(),
            false,
            RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(ViewportId::ROOT, Some(Arc::clone(&window))))
            .map_err(|err| err.to_string())?;
        window.focus_window();
        Ok(Self {
            window,
            ctx,
            state,
            painter,
            unsaved: false,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Closing the window only puts it away. The overlay and the process keep running.
    pub fn put_away(&self) {
        #[cfg(target_os = "macos")]
        self.window.set_visible(false);
        #[cfg(not(target_os = "macos"))]
        self.window.set_minimized(true);
    }

    /// True while the window is visible and is the key window (`isKeyWindow` on macOS).
    /// A hidden or minimized window is never key.
    pub fn is_focused(&self) -> bool {
        self.window.is_visible().unwrap_or(true)
            && !self.window.is_minimized().unwrap_or(false)
            && self.window.has_focus()
    }

    pub fn show(&self) {
        #[cfg(target_os = "macos")]
        self.window.set_visible(true);
        #[cfg(not(target_os = "macos"))]
        self.window.set_minimized(false);
        self.window.focus_window();
    }

    pub fn take_unsaved(&mut self) -> bool {
        std::mem::take(&mut self.unsaved)
    }

    pub fn on_event(&mut self, event: &WindowEvent) -> WindowFlow {
        match event {
            WindowEvent::CloseRequested => return WindowFlow::CloseRequested,
            WindowEvent::RedrawRequested => return WindowFlow::Redraw,
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    self.painter
                        .on_window_resized(ViewportId::ROOT, width, height);
                }
            }
            _ => {}
        }
        let response = self.state.on_window_event(&self.window, event);
        if response.repaint || matches!(event, WindowEvent::Resized(_)) {
            WindowFlow::Redraw
        } else {
            WindowFlow::Nothing
        }
    }

    /// Runs one frame. Edits land in `style` right away.
    pub fn draw(&mut self, style: &mut StyleConfig, dashboard: Dashboard<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        let input = self.state.take_egui_input(&self.window);
        let output = self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    outcome.action = dashboard_section(ui, dashboard);
                    ui.add_space(8.0);
                    outcome.changed |= numbers_section(ui, style);
                    ui.add_space(8.0);
                    outcome.changed |= corner_section(ui, style);
                    ui.add_space(8.0);
                    setup_section(ui);
                });
            });
        });
        self.state
            .handle_platform_output(&self.window, output.platform_output);
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let mut textures = output.textures_delta;
        self.painter.paint_and_update_textures(
            ViewportId::ROOT,
            output.pixels_per_point,
            [0.0, 0.0, 0.0, 1.0],
            &primitives,
            &mut textures,
            Vec::new(),
            &self.window,
        );
        let again = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .is_some_and(|viewport| viewport.repaint_delay.is_zero());
        if again {
            self.window.request_redraw();
        }
        if outcome.changed {
            self.unsaved = true;
        }
        let held = self.ctx.input(|input| input.pointer.any_down());
        outcome.save = self.unsaved && !held;
        outcome
    }
}

fn dashboard_section(ui: &mut egui::Ui, dashboard: Dashboard<'_>) -> Option<Action> {
    let mut action = None;
    ui.heading("Dashboard");
    ui.label(RichText::new(dashboard.status).strong());
    ui.add_space(4.0);
    if dashboard.running {
        if ui
            .add_sized([120.0, 30.0], egui::Button::new("Stop"))
            .clicked()
        {
            action = Some(Action::Stop);
        }
        ui.label("Running. Damage numbers follow the game.");
    } else {
        let start = ui.add_enabled(
            dashboard.can_start,
            egui::Button::new("Start").min_size(egui::vec2(120.0, 30.0)),
        );
        if start.clicked() {
            action = Some(Action::Start);
        }
        if dashboard.can_start {
            ui.label("Stopped. Numbers are hidden and nothing is read from the game.");
        } else {
            ui.label("No game profile is loaded, so there is nothing to start.");
        }
    }
    action
}

fn numbers_section(ui: &mut egui::Ui, style: &mut StyleConfig) -> bool {
    let mut changed = false;
    ui.separator();
    ui.heading("Numbers");
    changed |= ui
        .checkbox(&mut style.show_numbers, "Show damage numbers")
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(&mut style.number_px, NUMBER_PX_RANGE.0..=NUMBER_PX_RANGE.1)
                .text("Text size"),
        )
        .changed();
    ui.horizontal(|ui| {
        ui.label("Anchor");
        changed |= ui
            .radio_value(&mut style.anchor, NumberAnchor::Hunter, "Hunter")
            .changed();
        changed |= ui
            .radio_value(&mut style.anchor, NumberAnchor::Monster, "Monster")
            .changed();
    });
    ui.add_space(4.0);
    ui.label("Categories the game tap tells apart:");
    egui::Grid::new("categories").num_columns(3).show(ui, |ui| {
        ui.label(RichText::new("Show").weak());
        ui.label(RichText::new("Color").weak());
        ui.label(RichText::new("Category").weak());
        ui.end_row();
        for category in Category::ALL {
            let entry = style.numbers.get_mut(category);
            changed |= ui.checkbox(&mut entry.show, "").changed();
            changed |= ui.color_edit_button_rgb(&mut entry.rgb).changed();
            ui.label(category.label());
            ui.end_row();
        }
    });
    ui.add_space(2.0);
    ui.label(
        RichText::new(
            "Small, medium and large hits are ranked against your last 50 hits. \
             The tap does not flag critical hits.",
        )
        .weak(),
    );
    if ui.button("Reset numbers").clicked() {
        style.numbers = NumberSettings::default();
        style.number_px = DEFAULT_NUMBER_PX;
        style.anchor = NumberAnchor::default();
        style.show_numbers = true;
        changed = true;
    }
    changed
}

fn corner_section(ui: &mut egui::Ui, style: &mut StyleConfig) -> bool {
    let mut changed = false;
    ui.separator();
    ui.heading("Corner");
    changed |= ui
        .checkbox(&mut style.show_recount, "Show the recount")
        .changed();
    changed |= ui
        .checkbox(&mut style.corner.show_total, "Show total damage")
        .changed();
    changed |= ui
        .checkbox(&mut style.corner.show_dps, "Show DPS")
        .changed();
    changed |= ui
        .add(
            egui::Slider::new(
                &mut style.corner.size_pt,
                CORNER_PT_RANGE.0..=CORNER_PT_RANGE.1,
            )
            .text("Size"),
        )
        .changed();
    if ui.button("Reset corner").clicked() {
        style.corner = CornerSettings::default();
        style.show_recount = true;
        changed = true;
    }
    changed
}

fn setup_section(ui: &mut egui::Ui) {
    ui.separator();
    ui.heading("Setup");
    ui.label("You need:");
    for requirement in GAME_REQUIREMENTS {
        ui.label(format!("  \u{2022} {requirement}"));
    }
    ui.add_space(4.0);
    ui.label("In Azahar:");
    for (index, step) in SETUP_STEPS.iter().enumerate() {
        ui.label(format!("{}. {step}", index + 1));
        if index == 2 {
            ui.label(
                RichText::new(LOG_FILTER)
                    .monospace()
                    .color(Color32::LIGHT_GREEN),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setup_checklist_carries_the_real_facts_and_no_spanish() {
        let text = GAME_REQUIREMENTS.join(" ") + &SETUP_STEPS.join(" ");
        for needle in ["0004000000197100", "v1.4", "RPC server", "Restart Azahar"] {
            assert!(text.contains(needle), "missing {needle}");
        }
        assert_eq!(LOG_FILTER, "*:Info RPC_Server:Warning");
        assert!(!text.to_lowercase().contains("reinicia"));
    }
}
