//! Fluent builder for constructing an [`AudioEngine`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::broadcast;
use tracing::info;
use vtx_common::{RecordingMode, TranscriptionMode};

use crate::{AudioEngine, EngineConfig, EngineTranscriptionCallback};
use crate::ptt::PttState;
use crate::transcription;

/// Broadcast channel capacity.
const BROADCAST_CAPACITY: usize = 256;

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
    transcription_enabled: bool,
    visualization_enabled: bool,
    vad_enabled: bool,
}

impl EngineBuilder {
    /// Create a new builder with all subsystems enabled and default configuration.
    pub fn new() -> Self {
        Self {
            config: EngineConfig::default(),
            transcription_enabled: true,
            visualization_enabled: true,
            vad_enabled: true,
        }
    }

    /// Create a builder pre-populated from an existing [`EngineConfig`].
    pub fn from_config(config: EngineConfig) -> Self {
        Self {
            config,
            transcription_enabled: true,
            visualization_enabled: true,
            vad_enabled: true,
        }
    }

    // -------------------------------------------------------------------------
    // Config field setters
    // -------------------------------------------------------------------------

    /// Override the Whisper model file path.
    pub fn model_path(mut self, path: PathBuf) -> Self {
        self.config.model_path = Some(path);
        self
    }

    /// Set recording mode (`Mixed` or `EchoCancel`).
    pub fn recording_mode(mut self, mode: RecordingMode) -> Self {
        self.config.recording_mode = mode;
        self
    }

    /// Set transcription mode (`Automatic` or `PushToTalk`).
    pub fn transcription_mode(mut self, mode: TranscriptionMode) -> Self {
        self.config.transcription_mode = mode;
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
    /// will be emitted from VAD. (PTT signals still work.)
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
    pub async fn build(self) -> Result<(AudioEngine, broadcast::Receiver<vtx_common::EngineEvent>), String> {
        info!("Initializing audio backend...");
        crate::platform::init_audio_backend()?;

        let (sender, receiver) = broadcast::channel(BROADCAST_CAPACITY);
        let sender = Arc::new(sender);

        // Optionally initialize transcription worker
        let transcription_queue = if self.transcription_enabled {
            let callback = EngineTranscriptionCallback {
                sender: sender.clone(),
            };
            let queue = Arc::new(transcription::TranscriptionQueue::new());
            queue.set_callback(Arc::new(callback));

            let model_path = self.config.model_path.clone().unwrap_or_else(|| {
                transcription::Transcriber::new().get_model_path().clone()
            });
            queue.start_worker(model_path);
            Some(queue)
        } else {
            None
        };

        // TranscribeState needs a queue; use a dummy no-op queue when transcription is off
        let queue_for_state = transcription_queue.clone().unwrap_or_else(|| {
            Arc::new(transcription::TranscriptionQueue::new())
        });

        let transcribe_state = Arc::new(std::sync::Mutex::new(
            transcription::TranscribeState::new(queue_for_state),
        ));

        let ptt_state = Arc::new(std::sync::Mutex::new(PttState { is_active: false }));

        let engine = AudioEngine {
            config: self.config,
            sender,
            transcription_queue,
            transcribe_state,
            audio_loop_active: Arc::new(AtomicBool::new(false)),
            transcription_enabled: Arc::new(AtomicBool::new(self.transcription_enabled)),
            vad_enabled: self.vad_enabled,
            visualization_enabled: self.visualization_enabled,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            ptt_state,
        };

        Ok((engine, receiver))
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
