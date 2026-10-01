//! Platform-specific window tracking and overlay surfaces.

mod error;
mod geom;
#[cfg(target_os = "macos")]
mod macos_list;
mod select;
mod style;
mod track;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

pub use error::PlatformError;
pub use geom::{
    content_rect, is_fullscreen, Insets, Rect, DEFAULT_STATUS_BAR_PT, DEFAULT_TITLE_BAR_PT,
};
pub use select::{select_window, HostWindow, TrackQuery, TOP_ASPECT};
pub use style::{
    overlay_collection_behavior, CAN_JOIN_ALL_SPACES, FULL_SCREEN_AUXILIARY, IGNORES_CYCLE,
    SCREEN_SAVER_LEVEL, STATIONARY,
};
pub use track::{
    system_tracker, track_windows, Display, ListTracker, NullTracker, TrackedWindow, WindowTracker,
};

#[cfg(target_os = "macos")]
pub use macos::{apply_click_through, tint_spike};
#[cfg(not(target_os = "macos"))]
pub use other::apply_click_through;
