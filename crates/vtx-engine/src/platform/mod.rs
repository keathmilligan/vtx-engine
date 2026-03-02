//! Platform-specific audio backends.
//!
//! Provides audio capture functionality through platform-native APIs:
//! - Windows: WASAPI
//! - macOS: CoreAudio + ScreenCaptureKit
//! - Linux: PipeWire

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

pub(crate) mod backend;

pub use backend::AudioBackend;

/// Initialize the platform-specific audio backend.
pub fn init_audio_backend() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        linux::init()
    }

    #[cfg(target_os = "windows")]
    {
        windows::init()
    }

    #[cfg(target_os = "macos")]
    {
        macos::init()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Err("Unsupported platform".to_string())
    }
}

/// Get the current audio backend.
pub fn get_backend() -> Option<&'static dyn AudioBackend> {
    #[cfg(target_os = "linux")]
    {
        linux::get_backend()
    }

    #[cfg(target_os = "windows")]
    {
        windows::get_backend()
    }

    #[cfg(target_os = "macos")]
    {
        macos::get_backend()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        None
    }
}
