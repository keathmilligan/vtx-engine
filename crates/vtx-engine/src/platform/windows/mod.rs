//! Windows audio backend using WASAPI.

mod wasapi;

use super::AudioBackend;
use crate::RecordingMode;
use std::sync::{Arc, Mutex, OnceLock};

/// Global backend instance
static BACKEND: OnceLock<Box<dyn AudioBackend>> = OnceLock::new();

/// Initialize the Windows audio backend.
pub fn init() -> Result<(), String> {
    tracing::info!("Initializing Windows WASAPI audio backend");

    // Create shared state for AEC and recording mode
    let aec_enabled = Arc::new(Mutex::new(false));
    let recording_mode = Arc::new(Mutex::new(RecordingMode::default()));

    let backend = wasapi::create_backend(aec_enabled, recording_mode)?;

    BACKEND
        .set(backend)
        .map_err(|_| "Backend already initialized".to_string())?;

    tracing::info!("Windows WASAPI audio backend initialized");
    Ok(())
}

/// Get the Windows audio backend.
pub fn get_backend() -> Option<&'static dyn AudioBackend> {
    BACKEND.get().map(|b| b.as_ref())
}
