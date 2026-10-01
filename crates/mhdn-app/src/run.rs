//! Event loop. Polls Azahar's window at 30 Hz and draws the debug HUD on demand.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mhdn_game::{Latest, Profile, Snapshot};
use mhdn_platform::{
    apply_click_through, system_tracker, OverlayHost, SurfaceUpdate, TrackedWindow, WindowTracker,
};
use mhdn_proj::{
    parse_layout_settings, resolve, LayoutOption, LayoutSettings, LayoutWatcher, ScreenRect,
};
use mhdn_render::{FrameClock, Quad, RenderError, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::calibrate::{command_from_key, handles, CalibCommand, Calibrator};
use crate::config::{config_path, layout_name, OverlayConfig, SnapshotDelay};
use crate::hud::{build_hud, scene_label, HudStats};
use crate::session::{sample_rate, RateWindow, RpcMeter, Session};

const POLL: Duration = Duration::from_millis(33);

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let calibrate = std::env::args().any(|arg| arg == "--calibrate");
    let event_loop = EventLoop::new()?;
    let mut app = OverlayApp::new(calibrate);
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct OverlayApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    tracker: Box<dyn WindowTracker>,
    host: OverlayHost,
    tracked: Option<TrackedWindow>,
    layout: LayoutSettings,
    layout_key: String,
    layout_watcher: Option<LayoutWatcher>,
    config: OverlayConfig,
    config_file: PathBuf,
    snapshots: Arc<Latest<Snapshot>>,
    delay: SnapshotDelay,
    meter: Arc<RpcMeter>,
    rate: RateWindow,
    clock: FrameClock,
    last_frame: u32,
    calibrator: Calibrator,
    cursor: (f32, f32),
    parked: bool,
    _session: Option<Session>,
}

impl OverlayApp {
    fn new(calibrate: bool) -> Self {
        let config_file = config_path();
        let config = OverlayConfig::load(&config_file);
        let layout_watcher = azahar_config().and_then(|path| match LayoutWatcher::open(path) {
            Ok(watcher) => Some(watcher),
            Err(err) => {
                eprintln!("mhdn: layout: {err}");
                None
            }
        });
        let detected = layout_watcher
            .as_ref()
            .map(|watcher| watcher.settings().clone())
            .unwrap_or_else(load_layout);
        let layout = config.effective_layout(detected);
        let layout_key = layout_name(layout.option).to_string();
        let mut calibrator = Calibrator::default();
        calibrator.calibration = config.calibration_for(&layout_key);
        calibrator.active = calibrate;
        let tracker = system_tracker(config.insets_for(layout.show_status_bar));
        let snapshots = Arc::new(Latest::new());
        let meter = Arc::new(RpcMeter::default());
        let session = load_profile()
            .map(|profile| Session::start(profile, Arc::clone(&snapshots), Arc::clone(&meter)));
        Self {
            window: None,
            renderer: None,
            tracker,
            host: OverlayHost::new(),
            tracked: None,
            layout,
            layout_key,
            layout_watcher,
            config,
            config_file,
            snapshots,
            delay: SnapshotDelay::default(),
            meter,
            rate: RateWindow::new(Instant::now()),
            clock: FrameClock::default(),
            last_frame: 0,
            calibrator,
            cursor: (0.0, 0.0),
            parked: false,
            _session: session,
        }
    }

    fn follow(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        self.tracked = self.tracker.poll();
        let Some(tracked) = self.tracked else {
            self.park(&window);
            return;
        };
        if self.parked {
            self.parked = false;
            self.host = OverlayHost::new();
        }
        let update = self
            .host
            .sync(&window, tracked.content_rect, Instant::now());
        if !matches!(update, SurfaceUpdate::Idle) {
            self.clock.request();
        }
    }

    fn park(&mut self, window: &Window) {
        if self.parked {
            return;
        }
        let _ = window.request_inner_size(LogicalSize::new(1.0, 1.0));
        window.set_outer_position(LogicalPosition::new(-8.0, -8.0));
        self.parked = true;
        self.host = OverlayHost::new();
        self.clock.request();
    }

    fn take_snapshot(&mut self) {
        let Some(snapshot) = self.snapshots.take() else {
            return;
        };
        self.delay.push(snapshot);
        if self.config.debug_hud || self.calibrator.active {
            self.clock.request();
        }
    }

    fn reload_layout(&mut self) {
        let Some(watcher) = self.layout_watcher.as_mut() else {
            return;
        };
        match watcher.poll() {
            Ok(false) => {}
            Ok(true) => {
                let detected = watcher.settings().clone();
                self.apply_layout(detected);
            }
            Err(err) => eprintln!("mhdn: layout: {err}"),
        }
    }

    fn apply_layout(&mut self, detected: LayoutSettings) {
        let layout = self.config.effective_layout(detected);
        let key = layout_name(layout.option);
        if key != self.layout_key {
            self.layout_key = key.to_string();
            self.calibrator.calibration = self.config.calibration_for(&self.layout_key);
            self.calibrator.pointer_up();
        }
        self.tracker
            .set_insets(self.config.insets_for(layout.show_status_bar));
        self.layout = layout;
        self.clock.request();
    }

    fn window_size(&self) -> PhysicalSize<u32> {
        self.window
            .as_ref()
            .map(|window| window.inner_size())
            .unwrap_or(PhysicalSize::new(1, 1))
    }

    fn calibrated_top(&self, size: PhysicalSize<u32>) -> ScreenRect {
        self.calibrator
            .apply(top_screen(&self.layout, size.width, size.height))
    }

    fn quads(&mut self, size: PhysicalSize<u32>) -> Vec<Quad> {
        let top = self.calibrated_top(size);
        if !self.config.debug_hud && !self.calibrator.active {
            return Vec::new();
        }
        sample_rate(&mut self.rate, &self.meter, Instant::now());
        let snapshot = self.delay.sample(self.config.latency()).cloned();
        let mut quads = if self.config.debug_hud {
            let stats = HudStats {
                scene: snapshot
                    .as_ref()
                    .map(|snapshot| scene_label(snapshot.scene))
                    .unwrap_or("WAITING"),
                guest_frame: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.guest_frame)
                    .unwrap_or(self.last_frame),
                requests_per_sec: self.rate.requests_per_sec,
                rpc_latency_ms: self.rate.latency_ms,
                rpc_up: self.meter.is_up(),
            };
            if let Some(snapshot) = snapshot.as_ref() {
                self.last_frame = snapshot.guest_frame;
            }
            build_hud(snapshot.as_ref(), top, &stats, self.config.style.text_scale)
        } else {
            Vec::new()
        };
        if self.calibrator.active {
            quads.extend(handles(top));
        }
        quads
    }

    fn draw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        let quads = self.quads(size);
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        if let Err(RenderError::Outdated) = renderer.draw(&quads) {
            renderer.resize(size.width, size.height);
            let _ = renderer.draw(&quads);
        }
    }

    fn set_calibration_active(&mut self, active: bool) {
        self.calibrator.active = active;
        self.calibrator.pointer_up();
        if let Some(window) = &self.window {
            if let Err(err) = apply_click_through(window, !active) {
                eprintln!("mhdn: {err}");
            }
            if active {
                window.focus_window();
            }
        }
        self.clock.request();
    }

    fn save_config(&mut self) {
        let key = self.layout_key.clone();
        self.config
            .set_calibration(key, self.calibrator.calibration);
        match self.config.save(&self.config_file) {
            Ok(()) => eprintln!("mhdn: saved {}", self.config_file.display()),
            Err(err) => eprintln!("mhdn: config: {err}"),
        }
    }

    fn on_key(&mut self, key: &winit::keyboard::Key) {
        let Some(command) = command_from_key(key) else {
            return;
        };
        match command {
            CalibCommand::Toggle => self.set_calibration_active(!self.calibrator.active),
            CalibCommand::Save => self.save_config(),
            CalibCommand::Nudge(dx, dy) => {
                self.calibrator.command(CalibCommand::Nudge(dx, dy));
                self.clock.request();
            }
        }
    }
}

impl ApplicationHandler for OverlayApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("mhdn")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("mhdn: {err}");
                event_loop.exit();
                return;
            }
        };
        if let Err(err) = apply_click_through(&window, !self.calibrator.active) {
            eprintln!("mhdn: {err}");
        }
        if self.calibrator.active {
            window.focus_window();
        }
        match Renderer::new(Arc::clone(&window)) {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(err) => {
                eprintln!("mhdn: {err}");
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        self.clock.request();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                self.clock.request();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                if self.calibrator.active {
                    self.calibrator.pointer_move(self.cursor.0, self.cursor.1);
                    self.clock.request();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left && self.calibrator.active {
                    match state {
                        ElementState::Pressed => {
                            let top = self.calibrated_top(self.window_size());
                            self.calibrator
                                .pointer_down(top, self.cursor.0, self.cursor.1);
                        }
                        ElementState::Released => self.calibrator.pointer_up(),
                    }
                    self.clock.request();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    self.on_key(&event.logical_key);
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.reload_layout();
        self.follow();
        self.take_snapshot();
        if self.clock.take() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + POLL));
    }
}

fn top_screen(layout: &LayoutSettings, width: u32, height: u32) -> ScreenRect {
    let separate = layout.option == LayoutOption::SeparateWindows;
    resolve(&layout.layout(width.max(1), height.max(1), separate))
        .top
        .to_screen()
}

fn load_layout() -> LayoutSettings {
    let Some(path) = azahar_config() else {
        return LayoutSettings::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return LayoutSettings::default();
    };
    parse_layout_settings(&text).unwrap_or_default()
}

fn azahar_config() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        let path =
            PathBuf::from(home).join("Library/Application Support/Azahar/config/qt-config.ini");
        path.is_file().then_some(path)
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        let path = PathBuf::from(appdata)
            .join("Azahar")
            .join("config")
            .join("qt-config.ini");
        path.is_file().then_some(path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn load_profile() -> Option<Profile> {
    let path = std::env::var("MHDN_PROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("profiles/mhxx-jp-v1.4-es.toml"));
    match Profile::load(&path) {
        Ok(profile) => Some(profile),
        Err(err) => {
            eprintln!("mhdn: profile {}: {err}", path.display());
            None
        }
    }
}
