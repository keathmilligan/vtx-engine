//! Shared types for vtx-engine audio capture, processing, and transcription.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};

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

/// Audio sample chunk delivered via the broadcast event channel.
///
/// Used by both [`EngineEvent::AudioData`] (processed) and
/// [`EngineEvent::RawAudioData`] (raw) variants.  The `sample_offset` field
/// provides sample-accurate timing: `sample_offset / sample_rate` gives the
/// chunk timestamp in seconds relative to the start of the capture session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingAudioData {
    /// Mono audio samples (f32, -1.0 to 1.0).
    pub samples: Vec<f32>,
    /// Sample rate in Hz (e.g. 48000).
    pub sample_rate: u32,
    /// Cumulative number of samples emitted for this stream since the capture
    /// session began, not counting the samples in this event.  The first event
    /// of a session has `sample_offset = 0`.
    pub sample_offset: u64,
}

// =============================================================================
// Hotkey Types
// =============================================================================

/// Platform-independent key codes for push-to-talk hotkey configuration.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    // === Modifier Keys ===
    RightAlt,
    LeftAlt,
    RightControl,
    LeftControl,
    #[default]
    RightShift,
    LeftShift,
    CapsLock,
    LeftMeta,
    RightMeta,

    // === Function Keys ===
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,

    // === Letter Keys ===
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,

    // === Digit Keys ===
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,

    // === Navigation Keys ===
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,

    // === Special Keys ===
    Escape,
    Tab,
    Space,
    Enter,
    Backspace,
    PrintScreen,
    ScrollLock,
    Pause,

    // === Punctuation / Symbol Keys ===
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Backquote,
    Comma,
    Period,
    Slash,

    // === Numpad Keys ===
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadMultiply,
    NumpadAdd,
    NumpadSubtract,
    NumpadDecimal,
    NumpadDivide,
    NumLock,
}

impl KeyCode {
    /// Get a human-readable display name for the key.
    pub fn display_name(&self) -> &'static str {
        match self {
            KeyCode::RightAlt => "Right Alt",
            KeyCode::LeftAlt => "Left Alt",
            KeyCode::RightControl => "Right Ctrl",
            KeyCode::LeftControl => "Left Ctrl",
            KeyCode::RightShift => "Right Shift",
            KeyCode::LeftShift => "Left Shift",
            KeyCode::CapsLock => "Caps Lock",
            KeyCode::LeftMeta => "Left Win",
            KeyCode::RightMeta => "Right Win",
            KeyCode::F1 => "F1",
            KeyCode::F2 => "F2",
            KeyCode::F3 => "F3",
            KeyCode::F4 => "F4",
            KeyCode::F5 => "F5",
            KeyCode::F6 => "F6",
            KeyCode::F7 => "F7",
            KeyCode::F8 => "F8",
            KeyCode::F9 => "F9",
            KeyCode::F10 => "F10",
            KeyCode::F11 => "F11",
            KeyCode::F12 => "F12",
            KeyCode::F13 => "F13",
            KeyCode::F14 => "F14",
            KeyCode::F15 => "F15",
            KeyCode::F16 => "F16",
            KeyCode::F17 => "F17",
            KeyCode::F18 => "F18",
            KeyCode::F19 => "F19",
            KeyCode::F20 => "F20",
            KeyCode::F21 => "F21",
            KeyCode::F22 => "F22",
            KeyCode::F23 => "F23",
            KeyCode::F24 => "F24",
            KeyCode::KeyA => "A",
            KeyCode::KeyB => "B",
            KeyCode::KeyC => "C",
            KeyCode::KeyD => "D",
            KeyCode::KeyE => "E",
            KeyCode::KeyF => "F",
            KeyCode::KeyG => "G",
            KeyCode::KeyH => "H",
            KeyCode::KeyI => "I",
            KeyCode::KeyJ => "J",
            KeyCode::KeyK => "K",
            KeyCode::KeyL => "L",
            KeyCode::KeyM => "M",
            KeyCode::KeyN => "N",
            KeyCode::KeyO => "O",
            KeyCode::KeyP => "P",
            KeyCode::KeyQ => "Q",
            KeyCode::KeyR => "R",
            KeyCode::KeyS => "S",
            KeyCode::KeyT => "T",
            KeyCode::KeyU => "U",
            KeyCode::KeyV => "V",
            KeyCode::KeyW => "W",
            KeyCode::KeyX => "X",
            KeyCode::KeyY => "Y",
            KeyCode::KeyZ => "Z",
            KeyCode::Digit0 => "0",
            KeyCode::Digit1 => "1",
            KeyCode::Digit2 => "2",
            KeyCode::Digit3 => "3",
            KeyCode::Digit4 => "4",
            KeyCode::Digit5 => "5",
            KeyCode::Digit6 => "6",
            KeyCode::Digit7 => "7",
            KeyCode::Digit8 => "8",
            KeyCode::Digit9 => "9",
            KeyCode::ArrowUp => "Up",
            KeyCode::ArrowDown => "Down",
            KeyCode::ArrowLeft => "Left",
            KeyCode::ArrowRight => "Right",
            KeyCode::Home => "Home",
            KeyCode::End => "End",
            KeyCode::PageUp => "Page Up",
            KeyCode::PageDown => "Page Down",
            KeyCode::Insert => "Insert",
            KeyCode::Delete => "Delete",
            KeyCode::Escape => "Esc",
            KeyCode::Tab => "Tab",
            KeyCode::Space => "Space",
            KeyCode::Enter => "Enter",
            KeyCode::Backspace => "Backspace",
            KeyCode::PrintScreen => "Print Screen",
            KeyCode::ScrollLock => "Scroll Lock",
            KeyCode::Pause => "Pause",
            KeyCode::Minus => "-",
            KeyCode::Equal => "=",
            KeyCode::BracketLeft => "[",
            KeyCode::BracketRight => "]",
            KeyCode::Backslash => "\\",
            KeyCode::Semicolon => ";",
            KeyCode::Quote => "'",
            KeyCode::Backquote => "`",
            KeyCode::Comma => ",",
            KeyCode::Period => ".",
            KeyCode::Slash => "/",
            KeyCode::Numpad0 => "Num 0",
            KeyCode::Numpad1 => "Num 1",
            KeyCode::Numpad2 => "Num 2",
            KeyCode::Numpad3 => "Num 3",
            KeyCode::Numpad4 => "Num 4",
            KeyCode::Numpad5 => "Num 5",
            KeyCode::Numpad6 => "Num 6",
            KeyCode::Numpad7 => "Num 7",
            KeyCode::Numpad8 => "Num 8",
            KeyCode::Numpad9 => "Num 9",
            KeyCode::NumpadMultiply => "Num *",
            KeyCode::NumpadAdd => "Num +",
            KeyCode::NumpadSubtract => "Num -",
            KeyCode::NumpadDecimal => "Num .",
            KeyCode::NumpadDivide => "Num /",
            KeyCode::NumLock => "Num Lock",
        }
    }

    /// Whether this key is a modifier key.
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            KeyCode::LeftControl
                | KeyCode::RightControl
                | KeyCode::LeftAlt
                | KeyCode::RightAlt
                | KeyCode::LeftShift
                | KeyCode::RightShift
                | KeyCode::LeftMeta
                | KeyCode::RightMeta
        )
    }
}

/// A set of keys that must all be held simultaneously to trigger a hotkey action.
///
/// Order of keys does not matter for equality — two combinations with the same
/// keys in different order are considered equal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyCombination {
    /// One or more keys that must be held together.
    pub keys: Vec<KeyCode>,
}

impl HotkeyCombination {
    /// Create a new combination from a list of keys. Duplicates are removed and
    /// keys are sorted for consistent representation.
    pub fn new(keys: Vec<KeyCode>) -> Self {
        let mut unique: Vec<KeyCode> = keys
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        unique.sort_by_key(|k| format!("{:?}", k));
        Self { keys: unique }
    }

    /// Create a single-key combination.
    pub fn single(key: KeyCode) -> Self {
        Self { keys: vec![key] }
    }

    /// Display the combination in human-readable format.
    /// Modifiers are listed first, then other keys, joined by " + ".
    pub fn display(&self) -> String {
        let mut modifiers: Vec<&KeyCode> = Vec::new();
        let mut others: Vec<&KeyCode> = Vec::new();
        for k in &self.keys {
            if k.is_modifier() {
                modifiers.push(k);
            } else {
                others.push(k);
            }
        }
        modifiers.sort_by_key(|k| format!("{:?}", k));
        others.sort_by_key(|k| format!("{:?}", k));
        let all: Vec<&str> = modifiers
            .iter()
            .chain(others.iter())
            .map(|k| k.display_name())
            .collect();
        all.join(" + ")
    }

    /// Check whether all keys in this combination are in the given pressed set.
    pub fn is_subset_of(&self, pressed: &HashSet<KeyCode>) -> bool {
        self.keys.iter().all(|k| pressed.contains(k))
    }
}

impl PartialEq for HotkeyCombination {
    fn eq(&self, other: &Self) -> bool {
        let a: HashSet<_> = self.keys.iter().collect();
        let b: HashSet<_> = other.keys.iter().collect();
        a == b
    }
}

impl Eq for HotkeyCombination {}

impl Hash for HotkeyCombination {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut sorted: Vec<_> = self.keys.clone();
        sorted.sort_by_key(|k| format!("{:?}", k));
        sorted.hash(state);
    }
}

impl fmt::Display for HotkeyCombination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

impl Default for HotkeyCombination {
    fn default() -> Self {
        Self::single(KeyCode::default())
    }
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
    /// Spectrogram columns (RGB color values, one per completed FFT window)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spectrogram: Vec<SpectrogramColumn>,
    /// Speech detection metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_metrics: Option<SpeechMetrics>,
    /// Sample rate of the audio source in Hz (e.g. 48000).
    /// Allows the frontend to compute the true time span of the spectrogram.
    pub sample_rate: u32,
    /// Duration of the audio chunk that produced this frame, in milliseconds.
    /// Used by the frontend to correctly scale speech-activity time labels.
    pub frame_interval_ms: f32,
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
// Whisper Model Types
// =============================================================================

/// Whisper model variant to use for transcription.
///
/// Variants are listed roughly in order of size (smallest to largest).
/// En-suffixed variants are English-only and are generally faster for
/// English-language audio.
///
/// Sizes from <https://huggingface.co/ggerganov/whisper.cpp>.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperModel {
    /// ~75 MiB — fastest, English-only
    TinyEn,
    /// ~75 MiB — fastest, multilingual
    Tiny,
    /// ~142 MiB — fast, English-only
    #[default]
    BaseEn,
    /// ~142 MiB — fast, multilingual
    Base,
    /// ~466 MiB — good balance, English-only
    SmallEn,
    /// ~466 MiB — good balance, multilingual
    Small,
    /// ~1.5 GiB — high accuracy, English-only
    MediumEn,
    /// ~1.5 GiB — high accuracy, multilingual
    Medium,
    /// ~1.5 GiB — fast large model, multilingual (distilled from large-v3)
    LargeV3Turbo,
    /// ~2.9 GiB — best accuracy, multilingual
    LargeV3,
}

impl WhisperModel {
    /// Return the canonical config/API identifier for this model.
    pub fn config_key(self) -> &'static str {
        match self {
            WhisperModel::TinyEn => "tiny_en",
            WhisperModel::Tiny => "tiny",
            WhisperModel::BaseEn => "base_en",
            WhisperModel::Base => "base",
            WhisperModel::SmallEn => "small_en",
            WhisperModel::Small => "small",
            WhisperModel::MediumEn => "medium_en",
            WhisperModel::Medium => "medium",
            WhisperModel::LargeV3Turbo => "large_v3_turbo",
            WhisperModel::LargeV3 => "large_v3",
        }
    }

    /// Parse a model identifier from config, UI, slug, or filename form.
    pub fn parse_identifier(value: &str) -> Option<Self> {
        let normalized = value.trim().trim_matches('"');
        let normalized = normalized.strip_prefix("ggml-").unwrap_or(normalized);
        let normalized = normalized.strip_suffix(".bin").unwrap_or(normalized);
        let normalized = normalized.to_ascii_lowercase();

        match normalized.as_str() {
            "tiny_en" | "tiny.en" => Some(WhisperModel::TinyEn),
            "tiny" => Some(WhisperModel::Tiny),
            "base_en" | "base.en" => Some(WhisperModel::BaseEn),
            "base" => Some(WhisperModel::Base),
            "small_en" | "small.en" => Some(WhisperModel::SmallEn),
            "small" => Some(WhisperModel::Small),
            "medium_en" | "medium.en" => Some(WhisperModel::MediumEn),
            "medium" => Some(WhisperModel::Medium),
            "large_v3_turbo" | "large-v3-turbo" => Some(WhisperModel::LargeV3Turbo),
            "large_v3" | "large-v3" => Some(WhisperModel::LargeV3),
            _ => None,
        }
    }

    /// Return the canonical whisper.cpp filename slug for this model.
    ///
    /// The file name on disk is `ggml-{slug}.bin`.
    pub fn slug(self) -> &'static str {
        match self {
            WhisperModel::TinyEn => "tiny.en",
            WhisperModel::Tiny => "tiny",
            WhisperModel::BaseEn => "base.en",
            WhisperModel::Base => "base",
            WhisperModel::SmallEn => "small.en",
            WhisperModel::Small => "small",
            WhisperModel::MediumEn => "medium.en",
            WhisperModel::Medium => "medium",
            WhisperModel::LargeV3Turbo => "large-v3-turbo",
            WhisperModel::LargeV3 => "large-v3",
        }
    }

    /// Return the Hugging Face download URL for this model.
    pub fn download_url(self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
            self.slug()
        )
    }

    /// Return the approximate model file size in megabytes (MiB).
    ///
    /// Values from <https://huggingface.co/ggerganov/whisper.cpp>.
    pub fn size_mb(self) -> u32 {
        match self {
            WhisperModel::TinyEn => 75,
            WhisperModel::Tiny => 75,
            WhisperModel::BaseEn => 142,
            WhisperModel::Base => 142,
            WhisperModel::SmallEn => 466,
            WhisperModel::Small => 466,
            WhisperModel::MediumEn => 1536,
            WhisperModel::Medium => 1536,
            WhisperModel::LargeV3Turbo => 1536,
            WhisperModel::LargeV3 => 2970,
        }
    }

    /// Return a human-readable display name for this model.
    pub fn display_name(self) -> &'static str {
        match self {
            WhisperModel::TinyEn => "Tiny En",
            WhisperModel::Tiny => "Tiny",
            WhisperModel::BaseEn => "Base En",
            WhisperModel::Base => "Base",
            WhisperModel::SmallEn => "Small En",
            WhisperModel::Small => "Small",
            WhisperModel::MediumEn => "Medium En",
            WhisperModel::Medium => "Medium",
            WhisperModel::LargeV3Turbo => "Large V3 Turbo",
            WhisperModel::LargeV3 => "Large V3",
        }
    }

    /// Return all variants in ascending order of model size.
    pub fn all_in_size_order() -> &'static [WhisperModel] {
        &[
            WhisperModel::TinyEn,
            WhisperModel::Tiny,
            WhisperModel::BaseEn,
            WhisperModel::Base,
            WhisperModel::SmallEn,
            WhisperModel::Small,
            WhisperModel::MediumEn,
            WhisperModel::Medium,
            WhisperModel::LargeV3Turbo,
            WhisperModel::LargeV3,
        ]
    }
}

// =============================================================================
// Transcription Profile
// =============================================================================

/// Preset configuration profile for the audio engine.
///
/// Use [`EngineBuilder::with_profile`](https://docs.rs/vtx-engine) to apply
/// a profile before calling individual setters.
///
/// - `Dictation` — short-burst real-time microphone dictation (FlowSTT-style).
/// - `Transcription` — long-form post-capture transcription (OmniRec-style).
/// - `Custom` — no presets applied; all `EngineConfig` fields stay at `Default`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProfile {
    /// Short-burst VAD-driven dictation.
    ///
    /// Presets: `segment_max_duration_ms = 4_000`, `word_break_segmentation_enabled = true`,
    /// `segment_word_break_grace_ms = 750`, `model = WhisperModel::BaseEn`.
    #[default]
    Dictation,
    /// Long-form timestamped transcription.
    ///
    /// Presets: `segment_max_duration_ms = 15_000`, `word_break_segmentation_enabled = false`,
    /// `model = WhisperModel::MediumEn`.
    Transcription,
    /// No presets — all `EngineConfig` fields remain at their `Default` values.
    Custom,
}

// =============================================================================
// Transcription Types
// =============================================================================

/// A single timestamped segment produced by stream or file transcription.
///
/// Emitted as `EngineEvent::TranscriptionSegment` during `transcribe_audio_stream`
/// and `transcribe_audio_file` sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// Unique segment identifier (UUID v4 formatted string)
    pub id: String,
    /// Transcribed text for this segment
    pub text: String,
    /// Milliseconds from session start to the beginning of this segment
    pub timestamp_offset_ms: u64,
    /// Duration of this audio segment in milliseconds
    pub duration_ms: u64,
    /// Path to the saved audio file for this segment, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
}

/// Transcription result for a speech segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Unique history entry ID (populated when stored in history)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Transcribed text
    pub text: String,
    /// ISO 8601 UTC timestamp of when transcription completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Duration of the audio segment in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Path to the saved audio file (if saved)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
    /// Milliseconds from session/recording start to this segment.
    ///
    /// `None` for real-time live-capture dictation sessions.
    /// `Some(ms)` for file-based transcription via `transcribe_audio_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_offset_ms: Option<u64>,
}

/// A single entry in the transcription history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unique identifier (UUID v4)
    pub id: String,
    /// Transcribed text
    pub text: String,
    /// ISO 8601 UTC timestamp of when transcription occurred
    pub timestamp: String,
    /// Path to the cached WAV file, if it still exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wav_path: Option<String>,
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
    /// A single timestamped transcription segment from `transcribe_audio_stream`
    /// or `transcribe_audio_file`. NOT emitted during live-capture dictation.
    TranscriptionSegment(TranscriptionSegment),
    /// Manual recording started (via `start_recording`)
    RecordingStarted,
    /// Manual recording stopped (via `stop_recording`)
    RecordingStopped {
        /// Duration in milliseconds
        duration_ms: u64,
    },
    /// File playback through the engine pipeline has completed (or was cancelled).
    PlaybackComplete,
    /// Current AGC gain changed (emitted at most every 100 ms when AGC is enabled).
    ///
    /// The `f32` value is the instantaneous AGC gain in dB at the time of emission.
    AgcGainChanged(f32),
    /// Processed audio data (post-gain, post-AGC).  Emitted for every audio
    /// chunk when audio streaming is enabled via
    /// [`EngineBuilder::with_audio_streaming`](crate::EngineBuilder::with_audio_streaming).
    AudioData(StreamingAudioData),
    /// Raw audio data (post-mono-conversion, pre-gain, pre-AGC).  Emitted for
    /// every audio chunk when raw audio streaming is enabled via
    /// [`EngineBuilder::with_raw_audio_streaming`](crate::EngineBuilder::with_raw_audio_streaming).
    RawAudioData(StreamingAudioData),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_audio_data_fields() {
        let data = StreamingAudioData {
            samples: vec![0.1, -0.5, 1.0],
            sample_rate: 48000,
            sample_offset: 96000,
        };
        assert_eq!(data.samples.len(), 3);
        assert_eq!(data.sample_rate, 48000);
        assert_eq!(data.sample_offset, 96000);
    }

    #[test]
    fn streaming_audio_data_clone() {
        let data = StreamingAudioData {
            samples: vec![0.25, -0.75],
            sample_rate: 48000,
            sample_offset: 0,
        };
        let cloned = data.clone();
        assert_eq!(cloned.samples, data.samples);
        assert_eq!(cloned.sample_rate, data.sample_rate);
        assert_eq!(cloned.sample_offset, data.sample_offset);
    }

    #[test]
    fn streaming_audio_data_serialization_round_trip() {
        let data = StreamingAudioData {
            samples: vec![0.0, 0.5, -0.5, 1.0, -1.0],
            sample_rate: 48000,
            sample_offset: 480000,
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let deserialized: StreamingAudioData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.samples, data.samples);
        assert_eq!(deserialized.sample_rate, data.sample_rate);
        assert_eq!(deserialized.sample_offset, data.sample_offset);
    }

    #[test]
    fn engine_event_audio_data_variant_matches() {
        let event = EngineEvent::AudioData(StreamingAudioData {
            samples: vec![0.1],
            sample_rate: 48000,
            sample_offset: 0,
        });
        match &event {
            EngineEvent::AudioData(data) => {
                assert_eq!(data.samples, vec![0.1]);
                assert_eq!(data.sample_rate, 48000);
                assert_eq!(data.sample_offset, 0);
            }
            _ => panic!("expected AudioData variant"),
        }
    }

    #[test]
    fn engine_event_raw_audio_data_variant_matches() {
        let event = EngineEvent::RawAudioData(StreamingAudioData {
            samples: vec![-0.3, 0.7],
            sample_rate: 48000,
            sample_offset: 960,
        });
        match &event {
            EngineEvent::RawAudioData(data) => {
                assert_eq!(data.samples, vec![-0.3, 0.7]);
                assert_eq!(data.sample_rate, 48000);
                assert_eq!(data.sample_offset, 960);
            }
            _ => panic!("expected RawAudioData variant"),
        }
    }

    #[test]
    fn engine_event_audio_data_serialization() {
        let event = EngineEvent::AudioData(StreamingAudioData {
            samples: vec![0.5],
            sample_rate: 48000,
            sample_offset: 0,
        });
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"event\":\"audio_data\""));
        assert!(json.contains("\"sample_rate\":48000"));
        assert!(json.contains("\"sample_offset\":0"));
    }

    #[test]
    fn engine_event_raw_audio_data_serialization() {
        let event = EngineEvent::RawAudioData(StreamingAudioData {
            samples: vec![-0.25],
            sample_rate: 48000,
            sample_offset: 480,
        });
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"event\":\"raw_audio_data\""));
        assert!(json.contains("\"sample_rate\":48000"));
        assert!(json.contains("\"sample_offset\":480"));
    }

    #[test]
    fn streaming_audio_data_timestamp_computation() {
        // Verify the documented formula: timestamp_seconds = sample_offset / sample_rate
        let data = StreamingAudioData {
            samples: vec![0.0; 480],
            sample_rate: 48000,
            sample_offset: 480000,
        };
        let timestamp_seconds = data.sample_offset as f64 / data.sample_rate as f64;
        assert!(
            (timestamp_seconds - 10.0).abs() < 1e-9,
            "expected 10.0s, got {}",
            timestamp_seconds
        );
    }

    #[test]
    fn streaming_audio_data_first_chunk_offset_zero() {
        let data = StreamingAudioData {
            samples: vec![0.0; 480],
            sample_rate: 48000,
            sample_offset: 0,
        };
        let timestamp_seconds = data.sample_offset as f64 / data.sample_rate as f64;
        assert_eq!(timestamp_seconds, 0.0);
    }

    #[test]
    fn streaming_audio_data_offset_increment() {
        // Simulate two consecutive chunks and verify offset semantics
        let chunk1 = StreamingAudioData {
            samples: vec![0.0; 480],
            sample_rate: 48000,
            sample_offset: 0,
        };
        let next_offset = chunk1.sample_offset + chunk1.samples.len() as u64;
        let chunk2 = StreamingAudioData {
            samples: vec![0.0; 480],
            sample_rate: 48000,
            sample_offset: next_offset,
        };
        assert_eq!(chunk2.sample_offset, 480);
        // Third chunk
        let next_offset2 = chunk2.sample_offset + chunk2.samples.len() as u64;
        assert_eq!(next_offset2, 960);
    }
}
