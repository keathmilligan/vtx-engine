//! macOS audio backend using CoreAudio and ScreenCaptureKit.

mod coreaudio;
pub mod screencapturekit;

use super::AudioBackend;
use std::sync::{Arc, Mutex, OnceLock};
use vtx_common::RecordingMode;

/// Global backend instance
static BACKEND: OnceLock<Box<dyn AudioBackend>> = OnceLock::new();

/// Initialize the macOS audio backend.
pub fn init() -> Result<(), String> {
    tracing::info!("Initializing macOS CoreAudio audio backend");

    // Create shared state for AEC and recording mode
    let aec_enabled = Arc::new(Mutex::new(false));
    let recording_mode = Arc::new(Mutex::new(RecordingMode::default()));

    let backend = coreaudio::create_backend(aec_enabled, recording_mode)?;

    BACKEND
        .set(backend)
        .map_err(|_| "Backend already initialized".to_string())?;

    tracing::info!("macOS CoreAudio audio backend initialized");
    Ok(())
}

/// Get the macOS audio backend.
pub fn get_backend() -> Option<&'static dyn AudioBackend> {
    BACKEND.get().map(|b| b.as_ref())
}
