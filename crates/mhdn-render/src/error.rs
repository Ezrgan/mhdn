//! Renderer errors.

#![forbid(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("no GPU adapter can drive the overlay surface: {0}")]
    NoAdapter(String),
    #[error("failed to create the overlay surface: {0}")]
    Surface(String),
    #[error("failed to create the GPU device: {0}")]
    Device(String),
    #[error("the overlay surface is outdated")]
    Outdated,
}
