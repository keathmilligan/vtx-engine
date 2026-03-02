//! Platform-agnostic audio backend trait.

pub use vtx_common::{AudioData, AudioDevice, RecordingMode};

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
}
