//! World-to-screen projection and Azahar framebuffer layout.

#![forbid(unsafe_code)]

mod camera;
mod config;
mod interp;
mod layout;

pub use camera::{
    guess_ndc_rotation, project, project_ndc, Camera, CameraParams, EdgeMode, NdcRotation,
    Projected, ScreenRect, TOP_ASPECT, TOP_HEIGHT, TOP_WIDTH,
};
pub use config::{parse_layout_settings, ConfigError, LayoutSettings, LayoutWatcher};
pub use interp::{render_timestamp, CameraBuffer, CameraFrame, DEFAULT_DISPLAY_LATENCY};
pub use layout::{
    resolve, AspectRatio, CustomRect, LayoutInput, LayoutOption, Rect, ScreenRects,
    SmallScreenPosition, BOTTOM_HEIGHT, BOTTOM_WIDTH,
};
