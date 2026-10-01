//! Event loop. Polls Azahar's window at 30 Hz and draws the debug HUD on demand.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mhdn_game::{Latest, Profile, Snapshot};
use mhdn_platform::{
    apply_click_through, system_tracker, Insets, OverlayHost, SurfaceUpdate, TrackedWindow,
    WindowTracker,
};
use mhdn_proj::{parse_layout_settings, resolve, LayoutOption, LayoutSettings, ScreenRect};
use mhdn_render::{FrameClock, Quad, RenderError, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::hud::{build_hud, scene_label, HudStats};
use crate::session::{sample_rate, RateWindow, RpcMeter, Session};

const POLL: Duration = Duration::from_millis(33);

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = OverlayApp::new();
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
    snapshots: Arc<Latest<Snapshot>>,
    meter: Arc<RpcMeter>,
    rate: RateWindow,
    clock: FrameClock,
    last_frame: u32,
    pending: Option<Snapshot>,
    parked: bool,
    _session: Option<Session>,
}

impl OverlayApp {
    fn new() -> Self {
        let layout = load_layout();
        let tracker = system_tracker(Insets::chrome(layout.show_status_bar));
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
            snapshots,
            meter,
            rate: RateWindow::new(Instant::now()),
            clock: FrameClock::default(),
            last_frame: 0,
            pending: None,
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
        if snapshot.guest_frame != self.last_frame {
            self.last_frame = snapshot.guest_frame;
            self.clock.request();
        }
        self.pending = Some(snapshot);
    }

    fn quads(&mut self, size: winit::dpi::PhysicalSize<u32>) -> Vec<Quad> {
        let top = top_screen(&self.layout, size.width, size.height);
        sample_rate(&mut self.rate, &self.meter, Instant::now());
        let snapshot = self.pending.as_ref();
        let stats = HudStats {
            scene: snapshot
                .map(|snapshot| scene_label(snapshot.scene))
                .unwrap_or("WAITING"),
            guest_frame: snapshot
                .map(|snapshot| snapshot.guest_frame)
                .unwrap_or(self.last_frame),
            requests_per_sec: self.rate.requests_per_sec,
            rpc_latency_ms: self.rate.latency_ms,
            rpc_up: self.meter.is_up(),
        };
        build_hud(snapshot, top, &stats)
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
        if let Err(err) = apply_click_through(&window, true) {
            eprintln!("mhdn: {err}");
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
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
