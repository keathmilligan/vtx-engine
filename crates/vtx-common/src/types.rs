//! Shared types for vtx-engine audio capture, processing, and transcription.

use serde::{Deserialize, Serialize};

// =============================================================================
// Audio Types
// =============================================================================

/// Audio source type for capture.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourceType {
    /// Microphone or other input device
    #[default]
    Input,
    /// System audio (monitor/loopback)
    System,
    /// Mixed input and system audio
    Mixed,
}

/// Recording mode - determines how multiple audio sources are combined.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    /// Mix both streams together (default behavior)
    #[default]
    Mixed,
    /// Echo cancellation mode - output only echo-cancelled primary source
    EchoCancel,
}

/// Information about an audio device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    /// Unique identifier (WASAPI endpoint ID, PipeWire node ID, CoreAudio UID, etc.)
    pub id: String,
    /// Display name for UI
    pub name: String,
    /// Type of audio source
    #[serde(default)]
    pub source_type: AudioSourceType,
}

/// Raw audio data received from a capture backend.
pub struct AudioData {
    /// Interleaved audio samples (f32, -1.0 to 1.0)
    pub samples: Vec<f32>,
    /// Number of channels (1=mono, 2=stereo)
    pub channels: u16,
    /// Sample rate in Hz
    pub sample_rate: u32,
}

// =============================================================================
// Visualization Types
// =============================================================================

/// A single column of spectrogram data ready for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectrogramColumn {
    /// RGB triplets for each pixel row (height * 3 bytes)
    pub colors: Vec<u8>,
}

/// Visualization data for real-time audio display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationData {
    /// Waveform amplitude values (downsampled for display)
    pub waveform: Vec<f32>,
    /// Spectrogram column (RGB color values, if ready)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectrogram: Option<SpectrogramColumn>,
    /// Speech detection metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_metrics: Option<SpeechMetrics>,
}

/// Speech detection metrics for visualization and analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechMetrics {
    /// RMS amplitude in dB
    pub amplitude_db: f32,
    /// Zero-crossing rate (0.0-0.5)
    pub zcr: f32,
    /// Spectral centroid in Hz
    pub centroid_hz: f32,
    /// Whether speech is currently detected
    pub is_speaking: bool,
    /// Whether voiced onset is pending
    pub voiced_onset_pending: bool,
    /// Whether whisper onset is pending
    pub whisper_onset_pending: bool,
    /// Whether a transient was detected
    pub is_transient: bool,
    /// Whether this is lookback-determined speech
    pub is_lookback_speech: bool,
    /// Whether this is a word break
    pub is_word_break: bool,
}

// =============================================================================
// Transcription Types
// =============================================================================

/// Transcription result for a speech segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Transcribed text
    pub text: String,
    /// Duration of the audio segment in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Path to the saved audio file (if saved)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
}

/// Status of the Whisper model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    /// Whether the model file exists and is available
    pub available: bool,
    /// Path to the model file
    pub path: String,
}

/// GPU acceleration status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuStatus {
    /// Whether CUDA is available
    pub cuda_available: bool,
    /// Whether Metal is available
    pub metal_available: bool,
    /// System info string from whisper.cpp
    pub system_info: String,
}

// =============================================================================
// Engine Status Types
// =============================================================================

/// Current status of the audio engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineStatus {
    /// Whether audio capture is active
    pub capturing: bool,
    /// Whether speech is currently being detected
    pub in_speech: bool,
    /// Number of segments waiting to be transcribed
    pub queue_depth: usize,
    /// Error message if something failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Currently configured primary audio source ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source1_id: Option<String>,
    /// Currently configured secondary audio source ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source2_id: Option<String>,
}

// =============================================================================
// Engine Events
// =============================================================================

/// Events emitted by the engine to consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EngineEvent {
    /// Visualization data update (waveform, spectrogram, speech metrics)
    VisualizationData(VisualizationData),
    /// Transcription result for a completed segment
    TranscriptionComplete(TranscriptionResult),
    /// Speech started (segment recording began)
    SpeechStarted,
    /// Speech ended
    SpeechEnded {
        /// Duration in milliseconds
        duration_ms: u64,
    },
    /// Audio capture state changed
    CaptureStateChanged {
        /// Whether capture is now active
        capturing: bool,
        /// Error message if capture failed
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Model download progress
    ModelDownloadProgress {
        /// Progress percentage (0-100)
        percent: u8,
    },
    /// Model download complete
    ModelDownloadComplete {
        /// Whether download succeeded
        success: bool,
    },
    /// Audio level update (from device test capture)
    AudioLevelUpdate {
        /// Device being tested
        device_id: String,
        /// RMS audio level in dB
        level_db: f32,
    },
}
