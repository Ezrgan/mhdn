//! World-to-screen projection and Azahar framebuffer layout.

#![forbid(unsafe_code)]

mod camera;
mod layout;

pub use camera::{
    guess_ndc_rotation, project, project_ndc, Camera, CameraParams, EdgeMode, NdcRotation,
    Projected, ScreenRect, TOP_ASPECT, TOP_HEIGHT, TOP_WIDTH,
};
pub use layout::{
    resolve, AspectRatio, CustomRect, LayoutInput, LayoutOption, Rect, ScreenRects,
    SmallScreenPosition, BOTTOM_HEIGHT, BOTTOM_WIDTH,
};
