//! Overlay window errors.

#![forbid(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("overlay style is only applied to an AppKit window")]
    NotAppKit,
    #[error("the AppKit view has no window")]
    NoWindow,
    #[error("overlay style must run on the main thread")]
    NotMainThread,
    #[error("failed to read the window handle: {0}")]
    Handle(String),
}
