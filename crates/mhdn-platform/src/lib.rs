//! Platform-specific window tracking and overlay surfaces.

mod error;
mod style;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

pub use error::PlatformError;
pub use style::{
    overlay_collection_behavior, CAN_JOIN_ALL_SPACES, FULL_SCREEN_AUXILIARY, IGNORES_CYCLE,
    SCREEN_SAVER_LEVEL, STATIONARY,
};

#[cfg(target_os = "macos")]
pub use macos::{apply_click_through, tint_spike};
#[cfg(not(target_os = "macos"))]
pub use other::apply_click_through;
