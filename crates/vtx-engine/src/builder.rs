//! Fluent builder for constructing an [`AudioEngine`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};

use tokio::sync::broadcast;
use tracing::info;
use crate::{RecordingMode, TranscriptionProfile, WhisperModel};

use crate::{AudioEngine, EngineConfig, EngineTranscriptionCallback};
use crate::transcription;
use crate::transcription::TranscribeStateCallback;

/// Broadcast channel capacity.
const BROADCAST_CAPACITY: usize = 256;

/// [`TranscribeStateCallback`] implementation that stores the path of each
/// saved recording WAV into a shared slot on the engine.
struct LastRecordingPathCallback {
    last_recording_path: Arc<std::sync::Mutex<Option<std::path::PathBuf>>>,
}

impl TranscribeStateCallback for LastRecordingPathCallback {
    fn on_recording_saved(&self, path: String) {
        if let Ok(mut guard) = self.last_recording_path.lock() {
            *guard = Some(std::path::PathBuf::from(path));
        }
    }

    fn on_queue_update(&self, _depth: usize) {}
}

/// Fluent builder for constructing an [`AudioEngine`].
///
/// # Example
///
/// ```rust,no_run
/// use vtx_engine::EngineBuilder;
///
/// #[tokio::main]
/// async fn main() {
///     let (engine, rx) = EngineBuilder::new()
///         .segment_max_duration_ms(10_000)
///         .without_visualization()
///         .build()
///         .await
///         .unwrap();
/// }
/// ```
pub struct EngineBuilder {
    config: EngineConfig,
    app_name: String,
    transcription_enabled: bool,
    visualization_enabled: bool,
    vad_enabled: bool,
}

impl EngineBuilder {
    /// Create a new builder with all subsystems enabled and default configuration.
    pub fn new() -> Self {
        Self {
            config: EngineConfig::default(),
            app_name: "vtx-engine".to_string(),
            transcription_enabled: true,
            visualization_enabled: true,
            vad_enabled: true,
        }
    }

    /// Create a builder pre-populated from an existing [`EngineConfig`].
    pub fn from_config(config: EngineConfig) -> Self {
        Self {
            config,
            app_name: "vtx-engine".to_string(),
            transcription_enabled: true,
            visualization_enabled: true,
            vad_enabled: true,
        }
    }

    /// Set the application name used for resolving data directories
    /// (model cache, config, history).
    ///
    /// Defaults to `"vtx-engine"`. Host applications should set this to
    /// their own name so that model files, config, and history are stored
    /// under the application's own data directory rather than vtx-engine's.
    pub fn app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = name.into();
        self
    }

    // -------------------------------------------------------------------------
    // Config field setters
    // -------------------------------------------------------------------------

    /// Set the Whisper model variant to use for transcription.
    ///
    /// Path resolution is handled by [`ModelManager`](crate::ModelManager).
    pub fn model(mut self, model: WhisperModel) -> Self {
        self.config.model = model;
        self
    }

    /// Override the Whisper model file path.
    ///
    /// **Deprecated** — use [`model`](Self::model) instead. When set, this
    /// takes precedence over the `model` field and a warning is logged.
    #[deprecated(since = "0.2.0", note = "Use EngineBuilder::model instead")]
    pub fn model_path(mut self, path: PathBuf) -> Self {
        #[allow(deprecated)]
        {
            self.config.model_path = Some(path);
        }
        self
    }

    /// Enable or disable word-break segmentation.
    ///
    /// When `false`, the audio loop still detects word-break events internally
    /// but does not split segments at pause boundaries. Segment boundaries are
    /// determined solely by speech-end detection and `segment_max_duration_ms`.
    /// Defaults to `true`.
    pub fn word_break_segmentation_enabled(mut self, enabled: bool) -> Self {
        self.config.word_break_segmentation_enabled = enabled;
        self
    }

    /// Apply a [`TranscriptionProfile`] preset, seeding `EngineConfig` with
    /// the profile's default values.
    ///
    /// This method **overwrites** any previously-set fields covered by the
    /// profile. Call individual setters *after* `with_profile` to override
    /// specific fields.
    ///
    /// `Custom` profile does nothing — all fields remain at their `Default`.
    pub fn with_profile(mut self, profile: TranscriptionProfile) -> Self {
        match profile {
            TranscriptionProfile::Dictation => {
                self.config.vad_voiced_threshold_db = -42.0;
                self.config.vad_whisper_threshold_db = -52.0;
                self.config.vad_voiced_onset_ms = 80;
                self.config.vad_whisper_onset_ms = 120;
                self.config.segment_max_duration_ms = 4_000;
                self.config.segment_word_break_grace_ms = 750;
                self.config.word_break_segmentation_enabled = true;
                self.config.model = WhisperModel::BaseEn;
            }
            TranscriptionProfile::Transcription => {
                self.config.vad_voiced_threshold_db = -42.0;
                self.config.vad_whisper_threshold_db = -52.0;
                self.config.vad_voiced_onset_ms = 80;
                self.config.vad_whisper_onset_ms = 120;
                self.config.segment_max_duration_ms = 15_000;
                self.config.segment_word_break_grace_ms = 0;
                self.config.word_break_segmentation_enabled = false;
                self.config.model = WhisperModel::MediumEn;
            }
            TranscriptionProfile::Custom => {
                // No presets applied; caller supplies all values via setters.
            }
        }
        self
    }

    /// Set recording mode (`Mixed` or `EchoCancel`).
    pub fn recording_mode(mut self, mode: RecordingMode) -> Self {
        self.config.recording_mode = mode;
        self
    }

    /// Set the voiced speech detection threshold in dB.
    pub fn vad_voiced_threshold_db(mut self, db: f32) -> Self {
        self.config.vad_voiced_threshold_db = db;
        self
    }

    /// Set the whisper/soft speech detection threshold in dB.
    pub fn vad_whisper_threshold_db(mut self, db: f32) -> Self {
        self.config.vad_whisper_threshold_db = db;
        self
    }

    /// Set the voiced speech onset duration in ms.
    pub fn vad_voiced_onset_ms(mut self, ms: u64) -> Self {
        self.config.vad_voiced_onset_ms = ms;
        self
    }

    /// Set the whisper speech onset duration in ms.
    pub fn vad_whisper_onset_ms(mut self, ms: u64) -> Self {
        self.config.vad_whisper_onset_ms = ms;
        self
    }

    /// Set maximum segment duration before seeking a word-break split in ms.
    pub fn segment_max_duration_ms(mut self, ms: u64) -> Self {
        self.config.segment_max_duration_ms = ms;
        self
    }

    /// Set grace period after max duration before forcing segment submission in ms.
    pub fn segment_word_break_grace_ms(mut self, ms: u64) -> Self {
        self.config.segment_word_break_grace_ms = ms;
        self
    }

    /// Set lookback buffer duration in ms.
    pub fn segment_lookback_ms(mut self, ms: u64) -> Self {
        self.config.segment_lookback_ms = ms;
        self
    }

    /// Set maximum transcription queue depth.
    pub fn transcription_queue_capacity(mut self, cap: usize) -> Self {
        self.config.transcription_queue_capacity = cap;
        self
    }

    /// Set visualization frame interval in ms.
    pub fn viz_frame_interval_ms(mut self, ms: u64) -> Self {
        self.config.viz_frame_interval_ms = ms;
        self
    }

    // -------------------------------------------------------------------------
    // Subsystem toggles
    // -------------------------------------------------------------------------

    /// Disable the transcription subsystem. No `TranscriptionComplete` events
    /// will be emitted and whisper.cpp will not be loaded.
    pub fn without_transcription(mut self) -> Self {
        self.transcription_enabled = false;
        self
    }

    /// Disable the visualization subsystem. No `VisualizationData` events
    /// will be emitted.
    pub fn without_visualization(mut self) -> Self {
        self.visualization_enabled = false;
        self
    }

    /// Disable the VAD subsystem. No `SpeechStarted`/`SpeechEnded` events
    /// will be emitted from VAD. (Manual recording via `start_recording`/
    /// `stop_recording` still works.)
    pub fn without_vad(mut self) -> Self {
        self.vad_enabled = false;
        self
    }

    // -------------------------------------------------------------------------
    // Build
    // -------------------------------------------------------------------------

    /// Build the [`AudioEngine`].
    ///
    /// Returns `(engine, receiver)` where `receiver` is the first broadcast
    /// receiver. Call [`AudioEngine::subscribe`] to obtain additional receivers.
    pub async fn build(self) -> Result<(AudioEngine, broadcast::Receiver<crate::EngineEvent>), String> {
        info!("Initializing audio backend...");
        crate::platform::init_audio_backend()?;

        let (sender, receiver) = broadcast::channel(BROADCAST_CAPACITY);
        let sender = Arc::new(sender);

        // Resolve the model path: model_path (deprecated) takes precedence, then model enum.
        #[allow(deprecated)]
        let resolved_model_path = if let Some(ref explicit_path) = self.config.model_path {
            tracing::warn!(
                "[EngineBuilder] model_path is deprecated (since 0.2.0). \
                 Use EngineConfig::model instead. Falling back to explicit path: {}",
                explicit_path.display()
            );
            explicit_path.clone()
        } else {
            // Use ModelManager to resolve path from the WhisperModel enum.
            crate::model_manager::ModelManager::new(&self.app_name)
                .path(self.config.model)
        };

        // Optionally initialize transcription worker
        let transcription_queue = if self.transcription_enabled {
            let callback = EngineTranscriptionCallback {
                sender: sender.clone(),
            };
            let queue = Arc::new(transcription::TranscriptionQueue::new());
            queue.set_callback(Arc::new(callback));

            queue.start_worker(resolved_model_path.clone());
            Some(queue)
        } else {
            None
        };

        // TranscribeState needs a queue; use a dummy no-op queue when transcription is off
        let queue_for_state = transcription_queue.clone().unwrap_or_else(|| {
            Arc::new(transcription::TranscriptionQueue::new())
        });

        // Shared slot that the TranscribeStateCallback writes the WAV path into.
        let last_recording_path: Arc<std::sync::Mutex<Option<std::path::PathBuf>>> =
            Arc::new(std::sync::Mutex::new(None));

        let mut ts = transcription::TranscribeState::new(queue_for_state);
        ts.set_callback(Arc::new(LastRecordingPathCallback {
            last_recording_path: last_recording_path.clone(),
        }));
        let transcribe_state = Arc::new(std::sync::Mutex::new(ts));

        let initial_mic_gain_bits = self.config.mic_gain_db.to_bits();
        let initial_agc_config = self.config.agc.clone();
        let engine = AudioEngine {
            config: self.config,
            recording_mode_override: Arc::new(std::sync::Mutex::new(None)),
            sender,
            transcription_queue,
            transcribe_state,
            audio_loop_active: Arc::new(AtomicBool::new(false)),
            transcription_enabled: Arc::new(AtomicBool::new(self.transcription_enabled)),
            vad_enabled: self.vad_enabled,
            visualization_enabled: self.visualization_enabled,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            recording_active: Arc::new(AtomicBool::new(false)),
            recording_start: Arc::new(std::sync::Mutex::new(None)),
            model_path: resolved_model_path,
            last_recording_path,
            playback_tx: Arc::new(std::sync::Mutex::new(None)),
            playback_active: Arc::new(AtomicBool::new(false)),
            mic_gain_db: Arc::new(AtomicU32::new(initial_mic_gain_bits)),
            ptt_mode: Arc::new(AtomicBool::new(true)),
            agc_config: Arc::new(std::sync::Mutex::new(initial_agc_config)),
            render_tx: Arc::new(std::sync::Mutex::new(None)),
        };

        Ok((engine, receiver))
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
