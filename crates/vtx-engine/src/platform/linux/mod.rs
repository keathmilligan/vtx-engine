//! Linux audio backend using PipeWire.

mod pipewire;

use super::AudioBackend;
use std::sync::{Arc, Mutex, OnceLock};
use vtx_common::RecordingMode;

/// Global backend instance
static BACKEND: OnceLock<Box<dyn AudioBackend>> = OnceLock::new();

/// Initialize the Linux audio backend.
pub fn init() -> Result<(), String> {
    tracing::info!("Initializing Linux PipeWire audio backend");

    // Create shared state for AEC and recording mode
    let aec_enabled = Arc::new(Mutex::new(false));
    let recording_mode = Arc::new(Mutex::new(RecordingMode::default()));

    let backend = pipewire::create_backend(aec_enabled, recording_mode)?;

    BACKEND
        .set(backend)
        .map_err(|_| "Backend already initialized".to_string())?;

    tracing::info!("Linux PipeWire audio backend initialized");
    Ok(())
}

/// Get the Linux audio backend.
pub fn get_backend() -> Option<&'static dyn AudioBackend> {
    BACKEND.get().map(|b| b.as_ref())
}
