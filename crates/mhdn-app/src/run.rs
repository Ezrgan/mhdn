//! Event loop. Polls Azahar's window at 30 Hz and draws the debug HUD on demand.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use mhdn_game::{EventQueue, Latest, Profile, Scene, Snapshot};
use mhdn_platform::{
    apply_click_through, begin_latency_critical, frontmost_pid, join_active_space,
    overlay_event_loop, overlay_parked, report_startup_failure, set_overlay_parked, system_tracker,
    MenuStatus, OverlayHost, OverlayUserEvent, SurfaceUpdate, TrackedWindow, WindowTracker,
};
use mhdn_proj::{
    parse_layout_settings, project, resolve, Camera, EdgeMode, LayoutOption, LayoutSettings,
    LayoutWatcher, Projected, ScreenRect,
};
use mhdn_render::{FrameClock, Paint, PaintWatch, Quad, RenderError, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::calibrate::{command_from_key, handles, CalibCommand, Calibrator};
use crate::config::{config_path, layout_name, OverlayConfig, SnapshotDelay};
use crate::diag::{self, Diag};
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
    diag::init();
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
    /// Falls back to drawing inline when the window never gets a redraw event.
    paint: PaintWatch,
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
    diag: Diag,
    log_path: String,
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
        diag::line(&format!(
            "start build={} os={} arch={} exe={} profile_version={} profile_loaded={}",
            diag::BUILD,
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|err| format!("unknown ({err})")),
            version,
            profile.is_some(),
        ));
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
            paint: PaintWatch::new(cfg!(windows)),
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
            diag: Diag::new(),
            log_path: diag::path_text(),
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
        let unparking = self.parked;
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
        // Back where it belongs before it is shown again, so the one-pixel parked
        // window never flashes over the game.
        if unparking {
            if let Err(err) = set_overlay_parked(&window, false) {
                eprintln!("mhdn: {err}");
            }
            self.clock.request();
        }
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
        if let Err(err) = set_overlay_parked(window, true) {
            eprintln!("mhdn: {err}");
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
        self.diag.note_guest_frame(snapshot.guest_frame);
        self.delay.push(snapshot);
        self.diag.snapshot();
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
        for note in self.events.drain_notes() {
            diag::line(&note);
        }
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
            let dropped = self.events.drain();
            self.diag.events(dropped.len());
            log_damage(&mut self.diag, &dropped);
            if let Some(first) = dropped.first() {
                if self.diag.first_event() {
                    diag::line(&format!(
                        "first damage event amount={} discarded=unsupported build",
                        first.amount
                    ));
                }
            }
            self.combat.clear();
        } else {
            let events = self.events.drain();
            self.diag.events(events.len());
            log_damage(&mut self.diag, &events);
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
            for update in &ingested.meter {
                diag::line(&diag::format_meter(
                    update.addr,
                    update.total,
                    update.poison,
                    update.topple,
                ));
            }
            if !ingested.gated {
                self.diag.spawned(ingested.anchors.iter().flatten().count());
            }
            if let Some(first) = events.first() {
                if self.diag.first_event() {
                    let top = self.calibrated_top(self.window_size());
                    let camera = newest
                        .as_ref()
                        .and_then(|snapshot| snapshot.camera.as_ref())
                        .and_then(crate::hud::camera_from);
                    let world = ingested.anchors.first().copied().flatten();
                    let screen = world
                        .map(|world| screen_label(world, camera.as_ref(), top))
                        .unwrap_or_else(|| "-".to_string());
                    let on_screen = world.zip(camera.as_ref()).is_some_and(|(world, camera)| {
                        matches!(
                            project(Vec3::from_array(world), camera, top, EdgeMode::Hide),
                            Projected::Visible { .. }
                        )
                    });
                    diag::line(&format!(
                        "first damage event amount={} source={:?} scene={scene:?} gated={} spawned={} on_screen={on_screen} screen={screen}",
                        first.amount,
                        first.source,
                        ingested.gated,
                        world.is_some(),
                    ));
                }
            }
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

    /// Logs each fact once, and again only when it changes.
    fn observe(&mut self) {
        let azahar = match self.tracked {
            Some(window) => {
                let insets = self.config.insets_for(self.layout.show_status_bar);
                format!(
                    "found x={:.0} y={:.0} w={:.0} h={:.0} scale={} fullscreen={} onscreen={} insets=title:{} status:{} left:{} right:{}",
                    window.content_rect.x,
                    window.content_rect.y,
                    window.content_rect.width,
                    window.content_rect.height,
                    window.scale,
                    window.is_fullscreen,
                    window.onscreen,
                    insets.title_bar,
                    insets.status_bar,
                    insets.left,
                    insets.right,
                )
            }
            None => "lost".to_string(),
        };
        self.diag.changed("azahar", azahar);
        if let Some(window) = &self.window {
            let size = window.inner_size();
            let overlay = match window.outer_position() {
                Ok(pos) => format!(
                    "x={} y={} w={} h={} scale={}",
                    pos.x,
                    pos.y,
                    size.width,
                    size.height,
                    window.scale_factor()
                ),
                Err(err) => format!(
                    "x=? y=? w={} h={} scale={} ({err})",
                    size.width,
                    size.height,
                    window.scale_factor()
                ),
            };
            self.diag.changed("overlay", overlay);
        }
        self.diag.changed("parked", self.parked.to_string());
        self.diag.changed("behind", self.behind.to_string());
        let settings_focused = self
            .settings
            .as_ref()
            .is_some_and(SettingsWindow::is_focused);
        self.diag
            .changed("settings_focused", settings_focused.to_string());
        let fresh = Instant::now().saturating_duration_since(self.last_snapshot_at) < STALE_AFTER;
        let latest = self.delay.latest().filter(|_| fresh);
        let scene = latest
            .map(|snapshot| snapshot.scene)
            .unwrap_or(Scene::Disconnected);
        let monsters = latest
            .map(|snapshot| snapshot.monsters.as_slice())
            .unwrap_or(&[]);
        if let Some(text) = self.diag.scene_change(scene, monsters) {
            diag::line(&text);
        }
        let tap = if self.session.is_none() {
            "no session"
        } else if self.events.tap_installed() {
            "installed"
        } else if self.events.tap_blocked() {
            "blocked"
        } else {
            "not installed"
        };
        self.diag.changed("tap", tap.to_string());
        self.diag.changed(
            "fingerprint",
            match self.events.supported() {
                Some(true) => "match",
                Some(false) => "mismatch",
                None => "unread",
            }
            .to_string(),
        );
        self.diag.changed(
            "layout",
            format!(
                "{} status_bar={}",
                self.layout_key, self.layout.show_status_bar
            ),
        );
        self.diag.changed("status", self.status_text.clone());
        self.diag.changed(
            "paint",
            if self.paint.stalled() {
                "inline (no redraw event arrived)"
            } else {
                "event"
            }
            .to_string(),
        );
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
            if self.combat.alive() {
                self.diag.projected(
                    camera
                        .as_ref()
                        .map(|camera| self.combat.draw_stats(camera, top)),
                );
            }
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
        let present = renderer.take_present_stats();
        self.paint.drawn();
        self.diag.redraw();
        self.diag
            .present(present.errors, present.skipped, present.first_error);
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
            log_path: &self.log_path,
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
        // No redirection bitmap: the DirectComposition swapchain is the window's only
        // content. With the redirection surface in place DWM composites an opaque
        // black bitmap behind it and the game disappears under a black sheet. The flag
        // can only be set when the window is created.
        #[cfg(windows)]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes.with_no_redirection_bitmap(true)
        };
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                diag::line(&format!("window error {err}"));
                report_startup_failure(&format!("mhdn: {err}"));
                event_loop.exit();
                return;
            }
        };
        if let Err(err) = apply_click_through(&window, !self.calibrator.active) {
            diag::line(&format!("click-through error {err}"));
            eprintln!("mhdn: {err}");
        }
        window.set_visible(true);
        if self.calibrator.active {
            window.focus_window();
        }
        match Renderer::new(Arc::clone(&window)) {
            Ok(renderer) => {
                diag::line(&format!(
                    "gpu adapter={} surface={}",
                    renderer.adapter_name(),
                    renderer.surface_name()
                ));
                self.renderer = Some(renderer);
            }
            Err(err) => {
                diag::line(&format!("gpu error {err}"));
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
                diag::line(&format!("status item error {err}"));
                eprintln!("mhdn: status item: {err}");
                None
            }
        };
        self.window = Some(window);
        self.clock.request();
        match SettingsWindow::new(event_loop) {
            Ok(settings) => self.settings = Some(settings),
            Err(err) => {
                diag::line(&format!("settings window error {err}"));
                eprintln!("mhdn: settings window: {err}");
            }
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
        self.diag.wake();
        self.reload_layout();
        self.follow();
        let pumping = self.pumping();
        if pumping {
            self.pump();
        }
        self.take_snapshot();
        self.refresh_combat();
        self.refresh_status();
        self.observe();
        self.diag.tick(
            Instant::now(),
            &self.meter,
            self.combat.alive_count(),
            self.pumped,
            self.events.tap_installed(),
            self.events.lost_tap(),
        );
        if self.trace.as_ref().is_some_and(Trace::due) {
            sample_rate(&mut self.rate, &self.meter, Instant::now());
            let context = self.trace_context();
            if let Some(trace) = self.trace.as_mut() {
                trace.summary(&context);
            }
        }
        if self.clock.take() || pumping {
            match self.paint.frame() {
                Paint::Request => {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Paint::Now => self.draw(),
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
            Ok(profile) => {
                diag::line(&format!("profile loaded from file {}", path.display()));
                return Some(profile);
            }
            Err(err) => {
                diag::line(&format!("profile error {}: {err}", path.display()));
                eprintln!("mhdn: profile {}: {err}", path.display());
                return None;
            }
        }
    }
    if env_override.is_some() {
        diag::line(&format!("profile error {}: not found", path.display()));
        eprintln!("mhdn: profile {}: not found", path.display());
        return None;
    }
    match Profile::from_toml_str(include_str!("../../../profiles/mhxx-jp-v1.4-es.toml")) {
        Ok(profile) => {
            diag::line("profile loaded from embedded copy");
            Some(profile)
        }
        Err(err) => {
            diag::line(&format!("embedded profile error {err}"));
            eprintln!("mhdn: embedded profile: {err}");
            None
        }
    }
}

fn log_damage(diag: &mut Diag, events: &[mhdn_game::DamageEvent]) {
    for event in events {
        diag.note_damage(event.source, event.amount);
        diag::line(&diag::format_dmg(event));
    }
}
