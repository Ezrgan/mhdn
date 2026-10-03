//! Event loop. Polls Azahar's window at 30 Hz and draws the debug HUD on demand.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use mhdn_game::{EventQueue, Latest, Profile, Scene, Snapshot};
use mhdn_platform::{
    apply_click_through, begin_latency_critical, frontmost_pid, join_active_space,
    overlay_event_loop, overlay_parked, report_startup_failure, system_tracker, MenuStatus,
    OverlayHost, OverlayUserEvent, SurfaceUpdate, TrackedWindow, WindowTracker,
};
use mhdn_proj::{
    parse_layout_settings, project, resolve, Camera, EdgeMode, LayoutOption, LayoutSettings,
    LayoutWatcher, Projected, ScreenRect,
};
use mhdn_render::{FrameClock, Quad, RenderError, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::calibrate::{command_from_key, handles, CalibCommand, Calibrator};
use crate::config::{config_path, layout_name, OverlayConfig, SnapshotDelay};
use crate::hud::{build_hud, scene_label, HudStats};
use crate::numbers::CombatView;
use crate::session::{sample_rate, RateWindow, RpcMeter, Session, PHASE_WAITING};
use crate::settings_window::{Action, Dashboard, SettingsWindow, WindowFlow};
use crate::status::{link_state, status_title};
use crate::trace::Trace;

/// Shown in the menu bar and the dashboard while the session is dropped.
const STOPPED_TITLE: &str = "mhdn: Stopped";
const POLL: Duration = Duration::from_millis(33);
/// The sampler publishes at 4–60 Hz in every scene, including the in-game pause.
const STALE_AFTER: Duration = Duration::from_secs(2);

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let calibrate = std::env::args().any(|arg| arg == "--calibrate");
    let _latency = begin_latency_critical();
    let event_loop = overlay_event_loop()?;
    let user_events = event_loop.create_proxy();
    let mut app = OverlayApp::new(calibrate, user_events);
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
    /// Another app owns the keyboard, so the overlay is parked even though Azahar is on screen.
    behind: bool,
    /// Times the overlay was reordered onto the active space, e.g. a native fullscreen one.
    rejoins: u32,
    /// Samples taken on this thread rather than the sampler thread.
    pumped: u64,
    events: Arc<EventQueue>,
    combat: CombatView,
    profile: Option<Profile>,
    version: String,
    last_tick: Instant,
    menu: Option<MenuStatus>,
    user_events: EventLoopProxy<OverlayUserEvent>,
    status_text: String,
    trace: Option<Trace>,
    last_snapshot_at: Instant,
    session: Option<Session>,
    settings: Option<SettingsWindow>,
}

impl OverlayApp {
    fn new(calibrate: bool, user_events: EventLoopProxy<OverlayUserEvent>) -> Self {
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
        let events = Arc::new(EventQueue::new(64));
        let profile = load_profile();
        let version = profile
            .as_ref()
            .map(|profile| profile.meta.version.clone())
            .unwrap_or_else(|| "desconocida".to_string());
        let session = profile.clone().map(|profile| {
            Session::start(
                profile,
                Arc::clone(&snapshots),
                Arc::clone(&meter),
                Arc::clone(&events),
            )
        });
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
            behind: false,
            rejoins: 0,
            pumped: 0,
            events,
            combat: CombatView::new(),
            profile,
            version,
            last_tick: Instant::now(),
            menu: None,
            user_events,
            status_text: String::new(),
            trace: Trace::from_env(),
            last_snapshot_at: Instant::now(),
            session,
            settings: None,
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
        let own_pid = i32::try_from(std::process::id()).unwrap_or(-1);
        // Our own pid counts as "in front" for calibration, so it also covers the settings
        // window. That one is a normal-level window and the overlay is at level 1000, so
        // the overlay has to step aside while Settings is the key window.
        let settings_focused = self
            .settings
            .as_ref()
            .is_some_and(SettingsWindow::is_focused);
        self.behind = overlay_parked(
            tracked.owner_pid,
            frontmost_pid(),
            own_pid,
            settings_focused,
            self.calibrator.active,
        );
        if self.behind {
            self.park(&window);
            return;
        }
        if self.parked {
            self.parked = false;
            self.host = OverlayHost::new();
        }
        if let Ok(true) = join_active_space(&window) {
            self.rejoins += 1;
            self.clock.request();
        }
        let update = self
            .host
            .sync(&window, tracked.content_rect, Instant::now());
        if !matches!(update, SurfaceUpdate::Idle) {
            self.clock.request();
        }
    }

    /// Game Mode wakes every thread of a background process about 116 ms late, while
    /// the thread presenting frames keeps vsync. In a fullscreen hunt that thread
    /// samples, and keeps presenting so it stays on schedule.
    fn pumping(&self) -> bool {
        !self.parked
            && self.tracked.is_some_and(|window| window.is_fullscreen)
            && self
                .delay
                .latest()
                .is_some_and(|snapshot| snapshot.scene == Scene::InQuest)
    }

    fn pump(&mut self) {
        let Some(pump) = self.session.as_ref().and_then(Session::pump) else {
            return;
        };
        if pump.pump() {
            self.pumped += 1;
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
        self.last_snapshot_at = Instant::now();
        if let Some(trace) = self.trace.as_mut() {
            trace.snapshot();
        }
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

    fn refresh_combat(&mut self) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let was_alive = self.combat.alive();
        self.combat.tick(dt);
        let fresh = now.saturating_duration_since(self.last_snapshot_at) < STALE_AFTER;
        let newest = self.delay.latest().filter(|_| fresh).cloned();
        let scene = newest
            .as_ref()
            .map(|snapshot| snapshot.scene)
            .unwrap_or(Scene::Disconnected);
        let frame = newest
            .as_ref()
            .map(|snapshot| snapshot.guest_frame)
            .unwrap_or(self.last_frame);
        let reset = self.combat.observe(scene, frame);
        if let Some(reason) = reset {
            self.clock.request();
            if let Some(trace) = self.trace.as_mut() {
                trace.reset(reason, frame);
            }
        }
        if self.events.supported() == Some(false) {
            let _ = self.events.drain();
            self.combat.clear();
        } else {
            let events = self.events.drain();
            let monsters = newest
                .as_ref()
                .map(|snapshot| snapshot.monsters.as_slice())
                .unwrap_or(&[]);
            let profile = self.profile.clone();
            self.combat.set_anchor(self.config.style.anchor);
            self.combat.set_hunter(
                newest
                    .as_ref()
                    .and_then(|snapshot| snapshot.hunter_pos)
                    .map(|pos| [pos.x, pos.y, pos.z]),
            );
            let ingested = self.combat.ingest(&events, monsters, |species, large| {
                profile
                    .as_ref()
                    .map(|profile| profile.species.anchor_for(species, large))
                    .unwrap_or(if large { 150.0 } else { 52.0 })
            });
            if self.trace.is_some() && !events.is_empty() {
                let top = self.calibrated_top(self.window_size());
                let camera = newest
                    .as_ref()
                    .and_then(|snapshot| snapshot.camera.as_ref())
                    .and_then(crate::hud::camera_from);
                if let Some(trace) = self.trace.as_mut() {
                    trace.ingest(&events, &ingested, |world| {
                        screen_label(world, camera.as_ref(), top)
                    });
                }
            }
        }
        if was_alive || self.combat.alive() {
            self.clock.request();
        }
    }

    fn trace_context(&self) -> String {
        let size = self.window_size();
        let top = self.calibrated_top(size);
        let scale = self
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(0.0);
        let tracked = self
            .tracked
            .map(|window| {
                format!(
                    "azahar={:.0},{:.0},{:.0}x{:.0} on={} fs={} wscale={}",
                    window.content_rect.x,
                    window.content_rect.y,
                    window.content_rect.width,
                    window.content_rect.height,
                    u8::from(window.onscreen),
                    u8::from(window.is_fullscreen),
                    window.scale
                )
            })
            .unwrap_or_else(|| "azahar=none".to_string());
        let latest = self
            .delay
            .latest()
            .map(|snapshot| {
                format!(
                    "scene={:?} frame={} cam={} monsters={}",
                    snapshot.scene,
                    snapshot.guest_frame,
                    u8::from(snapshot.camera.is_some()),
                    snapshot.monsters.len()
                )
            })
            .unwrap_or_else(|| "scene=none".to_string());
        format!(
            "{tracked}\tparked={} behind={} rejoins={}\tsize={}x{} scale={scale}\ttop={:.0},{:.0},{:.0}x{:.0}\t{latest}\talive={}\ttotal={}\tphase={}\trpc={:.0}/s lat={:.2}ms\tticks={} pumped={}",
            u8::from(self.parked),
            u8::from(self.behind),
            self.rejoins,
            size.width,
            size.height,
            top.x,
            top.y,
            top.width,
            top.height,
            self.combat.alive_count(),
            self.combat.recount_total(),
            self.meter.phase(),
            self.rate.requests_per_sec,
            self.rate.latency_ms,
            self.session
                .as_ref()
                .and_then(Session::pump)
                .map_or(0, |pump| pump.ticks()),
            self.pumped
        )
    }

    fn refresh_status(&mut self) {
        let onscreen = self.tracked.as_ref().is_some_and(|window| window.onscreen);
        let title = if self.session.is_some() {
            status_title(link_state(self.meter.phase(), onscreen), &self.version)
        } else {
            STOPPED_TITLE.to_string()
        };
        if title == self.status_text {
            return;
        }
        self.status_text = title;
        if let Some(settings) = &self.settings {
            settings.request_redraw();
        }
        if let Some(menu) = &self.menu {
            menu.set_title(&self.status_text);
        }
        if let Some(window) = &self.window {
            window.set_title(&self.status_text);
        }
    }

    fn quads(&mut self, size: PhysicalSize<u32>) -> Vec<Quad> {
        let top = self.calibrated_top(size);
        // Text sizes are in points. The surface is in physical pixels.
        let ui = self
            .window
            .as_ref()
            .map(|window| window.scale_factor() as f32)
            .unwrap_or(1.0)
            .max(1.0);
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
            build_hud(
                snapshot.as_ref(),
                top,
                &stats,
                self.config.style.text_scale * ui,
            )
        } else {
            Vec::new()
        };
        if self.session.is_some() && self.config.style.show_numbers {
            let camera = snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.camera.as_ref().and_then(crate::hud::camera_from));
            if let Some(trace) = self.trace.as_mut() {
                if self.combat.alive() {
                    trace.draw(
                        camera
                            .as_ref()
                            .map(|camera| self.combat.draw_stats(camera, top)),
                    );
                }
            }
            if let Some(camera) = camera {
                quads.extend(self.combat.number_quads(
                    &camera,
                    top,
                    self.config.style.number_size() * ui,
                    &self.config.style.numbers,
                ));
            }
        }
        if self.session.is_some() && self.config.style.show_recount {
            quads.extend(self.combat.recount_quads(
                top,
                self.config.style.corner_size() * ui,
                &self.config.style.corner,
            ));
        }
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

    /// Dropping the session joins its thread, which uninstalls the damage tap.
    fn stop_session(&mut self) {
        if self.session.take().is_none() {
            return;
        }
        let _ = self.events.drain();
        self.combat.clear();
        self.meter.set_up(false);
        self.clock.request();
        self.refresh_status();
    }

    /// A fresh session starts from clean queues, so nothing from before the stop leaks in.
    fn start_session(&mut self) {
        if self.session.is_some() {
            return;
        }
        let Some(profile) = self.profile.clone() else {
            return;
        };
        self.snapshots = Arc::new(Latest::new());
        self.events = Arc::new(EventQueue::new(64));
        self.delay = SnapshotDelay::default();
        self.combat.clear();
        self.meter.set_up(false);
        self.meter.set_phase(PHASE_WAITING);
        self.last_snapshot_at = Instant::now();
        self.session = Some(Session::start(
            profile,
            Arc::clone(&self.snapshots),
            Arc::clone(&self.meter),
            Arc::clone(&self.events),
        ));
        self.clock.request();
        self.refresh_status();
    }

    fn draw_settings(&mut self) {
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        let dashboard = Dashboard {
            status: &self.status_text,
            running: self.session.is_some(),
            can_start: self.profile.is_some(),
        };
        let outcome = settings.draw(&mut self.config.style, dashboard);
        if outcome.changed {
            self.clock.request();
        }
        if outcome.save {
            settings.take_unsaved();
            match self.config.save(&self.config_file) {
                Ok(()) => eprintln!("mhdn: saved {}", self.config_file.display()),
                Err(err) => eprintln!("mhdn: config: {err}"),
            }
        }
        match outcome.action {
            Some(Action::Start) => self.start_session(),
            Some(Action::Stop) => self.stop_session(),
            None => {}
        }
    }

    fn settings_event(&mut self, event: &WindowEvent) {
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        match settings.on_event(event) {
            WindowFlow::Nothing => {}
            WindowFlow::Redraw => settings.request_redraw(),
            WindowFlow::CloseRequested => {
                if settings.take_unsaved() {
                    if let Err(err) = self.config.save(&self.config_file) {
                        eprintln!("mhdn: config: {err}");
                    }
                }
                settings.put_away();
            }
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            self.draw_settings();
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

impl ApplicationHandler<OverlayUserEvent> for OverlayApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("mhdn")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(false);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                report_startup_failure(&format!("mhdn: {err}"));
                event_loop.exit();
                return;
            }
        };
        if let Err(err) = apply_click_through(&window, !self.calibrator.active) {
            eprintln!("mhdn: {err}");
        }
        window.set_visible(true);
        if self.calibrator.active {
            window.focus_window();
        }
        match Renderer::new(Arc::clone(&window)) {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(err) => {
                report_startup_failure(&format!("mhdn: {err}"));
                event_loop.exit();
                return;
            }
        }
        let user_events = self.user_events.clone();
        self.menu = match MenuStatus::install(move |event| {
            let _ = user_events.send_event(event);
        }) {
            Ok(status) => Some(status),
            Err(err) => {
                eprintln!("mhdn: status item: {err}");
                None
            }
        };
        self.window = Some(window);
        self.clock.request();
        match SettingsWindow::new(event_loop) {
            Ok(settings) => self.settings = Some(settings),
            Err(err) => eprintln!("mhdn: settings window: {err}"),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: OverlayUserEvent) {
        match event {
            OverlayUserEvent::Quit => event_loop.exit(),
            OverlayUserEvent::ShowSettings => {
                if let Some(settings) = &self.settings {
                    settings.show();
                    settings.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self
            .settings
            .as_ref()
            .is_some_and(|settings| settings.id() == id)
        {
            self.settings_event(&event);
            return;
        }
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
        let pumping = self.pumping();
        if pumping {
            self.pump();
        }
        self.take_snapshot();
        self.refresh_combat();
        self.refresh_status();
        if self.trace.as_ref().is_some_and(Trace::due) {
            sample_rate(&mut self.rate, &self.meter, Instant::now());
            let context = self.trace_context();
            if let Some(trace) = self.trace.as_mut() {
                trace.summary(&context);
            }
        }
        if self.clock.take() || pumping {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        let wait = if self.combat.alive() || pumping {
            Duration::from_millis(16)
        } else {
            POLL
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
    }
}

fn screen_label(world: [f32; 3], camera: Option<&Camera>, top: ScreenRect) -> String {
    let Some(camera) = camera else {
        return "nocam".to_string();
    };
    match project(Vec3::from_array(world), camera, top, EdgeMode::Hide) {
        Projected::Visible { x, y } => format!("{x:.0},{y:.0}"),
        Projected::OffScreen { x, y } => format!("off({x:.0},{y:.0})"),
        Projected::Behind => "behind".to_string(),
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

const DEFAULT_PROFILE_PATH: &str = "profiles/mhxx-jp-v1.4-es.toml";

fn load_profile() -> Option<Profile> {
    let env_override = std::env::var("MHDN_PROFILE").ok();
    let path = env_override
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PROFILE_PATH));
    if path.is_file() {
        match Profile::load(&path) {
            Ok(profile) => return Some(profile),
            Err(err) => {
                eprintln!("mhdn: profile {}: {err}", path.display());
                return None;
            }
        }
    }
    if env_override.is_some() {
        eprintln!("mhdn: profile {}: not found", path.display());
        return None;
    }
    match Profile::from_toml_str(include_str!("../../../profiles/mhxx-jp-v1.4-es.toml")) {
        Ok(profile) => Some(profile),
        Err(err) => {
            eprintln!("mhdn: embedded profile: {err}");
            None
        }
    }
}
