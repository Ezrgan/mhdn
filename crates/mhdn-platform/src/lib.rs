//! Platform-specific window tracking and overlay surfaces.

/// User events injected from the menu bar or other platform UI into winit's loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayUserEvent {
    Quit,
    ShowSettings,
}

mod error;
mod follow;
mod geom;
#[cfg(target_os = "macos")]
mod macos_list;
mod select;
mod startup_error;
mod style;
mod track;

#[cfg(any(windows, test))]
mod win_geom;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(windows)]
mod windows;

pub use error::PlatformError;
pub use follow::{
    round_rect, FollowController, OverlayHost, PixelRect, SurfaceUpdate, MIN_SURFACE_INTERVAL,
};
pub use geom::{
    content_rect, is_fullscreen, Insets, Rect, DEFAULT_STATUS_BAR_PT, DEFAULT_TITLE_BAR_PT,
};
pub use select::{select_window, HostWindow, TrackQuery, TOP_ASPECT};
pub use startup_error::{log_dir, report_startup_failure};
pub use style::{
    overlay_collection_behavior, CAN_JOIN_ALL_SPACES, FULL_SCREEN_AUXILIARY, IGNORES_CYCLE,
    SCREEN_SAVER_LEVEL, STATIONARY,
};
pub use track::{
    host_in_front, overlay_parked, system_tracker, track_windows, Display, ListTracker,
    NullTracker, TrackedWindow, WindowTracker,
};

#[cfg(target_os = "macos")]
pub use macos::{
    apply_click_through, begin_latency_critical, frontmost_pid, join_active_space,
    overlay_event_loop, raise_thread_qos, set_overlay_parked, tint_spike, LatencyCritical,
    MenuStatus,
};
#[cfg(not(any(target_os = "macos", windows)))]
pub use other::{
    apply_click_through, begin_latency_critical, frontmost_pid, join_active_space,
    overlay_event_loop, raise_thread_qos, set_overlay_parked, LatencyCritical, MenuStatus,
};
#[cfg(windows)]
pub use windows::{
    apply_click_through, begin_latency_critical, frontmost_pid, join_active_space,
    overlay_event_loop, raise_thread_qos, set_overlay_parked, tint_spike, LatencyCritical,
    MenuStatus,
};
