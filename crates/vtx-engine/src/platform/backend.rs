//! Platform-agnostic audio backend trait.

use std::sync::mpsc;

pub use crate::{AudioData, AudioDevice, RecordingMode};

/// Platform-agnostic audio backend interface.
///
/// Each platform provides an implementation of this trait that uses
/// the native audio APIs for capture.
pub trait AudioBackend: Send + Sync {
    /// Get the sample rate for this backend.
    fn sample_rate(&self) -> u32;

    /// List available input devices (microphones).
    fn list_input_devices(&self) -> Vec<AudioDevice>;

    /// List available system audio devices (monitors/loopbacks).
    fn list_system_devices(&self) -> Vec<AudioDevice>;

    /// Return the system default audio output device (loopback/render endpoint).
    ///
    /// The default implementation returns the first device from
    /// [`list_system_devices`], which is a best-effort fallback on platforms
    /// that do not override this method.  Platform backends that can resolve
    /// the OS default output endpoint should override this to return the
    /// correct device regardless of enumeration order.
    fn get_default_system_device(&self) -> Option<AudioDevice> {
        self.list_system_devices().into_iter().next()
    }

    /// Start audio capture from the specified sources.
    fn start_capture_sources(
        &self,
        source1_id: Option<String>,
        source2_id: Option<String>,
    ) -> Result<(), String>;

    /// Stop audio capture.
    fn stop_capture(&self) -> Result<(), String>;

    /// Try to receive audio data (non-blocking).
    fn try_recv(&self) -> Option<AudioData>;

    /// Set whether AEC is enabled.
    fn set_aec_enabled(&self, enabled: bool);

    /// Set the recording mode.
    fn set_recording_mode(&self, mode: RecordingMode);

    /// Set the microphone input gain hint in dB.
    ///
    /// Implementations MAY apply this via OS/driver APIs. The default
    /// implementation is a no-op; software gain is applied in the
    /// `AudioEngine` capture loop regardless.
    fn set_gain(&self, _db: f32) {}

    /// Whether this backend can render playback audio to the system output.
    fn supports_render_output(&self) -> bool {
        false
    }

    /// Start audio output rendering on the system default output device.
    ///
    /// Returns a channel sender for pushing mono f32 samples at 48 kHz.
    /// The backend opens a render endpoint and spawns a thread that
    /// converts and writes samples to the device buffer.
    ///
    /// The default implementation returns an error indicating that
    /// render output is not supported on this platform.
    fn start_render(&self) -> Result<mpsc::SyncSender<Vec<f32>>, String> {
        Err("Audio render output is not supported on this platform".to_string())
    }

    /// Stop audio output rendering and release the render endpoint.
    ///
    /// The default implementation is a no-op.
    fn stop_render(&self) -> Result<(), String> {
        Ok(())
    }
}
