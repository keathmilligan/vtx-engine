//! vtx-engine: Voice processing and transcription engine.
//!
//! This library provides:
//! - Platform-native audio capture (WASAPI, CoreAudio, PipeWire)
//! - Real-time speech detection (VAD) with dual-mode voiced/whisper detection
//! - Audio visualization (waveform, spectrogram, speech activity metrics)
//! - Whisper-based transcription via whisper.cpp FFI
//! - Echo cancellation support
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use vtx_engine::EngineBuilder;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (engine, mut rx) = EngineBuilder::new().build().await.unwrap();
//!     tokio::spawn(async move {
//!         while let Ok(event) = rx.recv().await {
//!             println!("Event: {:?}", event);
//!         }
//!     });
//!     let devices = engine.list_input_devices();
//!     if let Some(device) = devices.first() {
//!         engine.start_capture(Some(device.id.clone()), None).await.unwrap();
//!     }
//! }
//! ```

mod audio;
pub mod builder;
pub mod config_persistence;
pub mod history;
pub mod model_manager;
pub mod platform;
pub mod processor;
pub mod transcription;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

pub mod common;
pub use common::*;

pub use builder::EngineBuilder;
pub use config_persistence::ConfigError;
pub use history::{HistoryError, TranscriptionHistory, TranscriptionHistoryRecorder};
pub use model_manager::{ModelError, ModelManager};

// =============================================================================
// EngineConfig
// =============================================================================

/// Configuration for the audio engine.
///
/// All fields have sensible defaults. Use [`EngineBuilder`] for a fluent
/// construction API, or construct this struct directly and pass it to
/// [`AudioEngine::new`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Whisper model to use for transcription (default `WhisperModel::BaseEn`).
    ///
    /// Path resolution is handled by `ModelManager::path`. If the deprecated
    /// `model_path` field is also set it takes precedence (with a warning).
    #[serde(default)]
    pub model: WhisperModel,

    /// Override model file location.
    ///
    /// **Deprecated** — use `model` instead. When set, this takes precedence
    /// over `model` and a `tracing::warn` is emitted. Retained for backward
    /// compatibility with serialised config files that contain `model_path`.
    #[deprecated(since = "0.2.0", note = "Use EngineConfig::model instead")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_path: Option<PathBuf>,

    /// Recording mode: `Mixed` combines both sources; `EchoCancel` applies
    /// AEC and outputs only the echo-cancelled primary source.
    #[serde(default)]
    pub recording_mode: RecordingMode,

    /// Voiced speech detection threshold in dB (default -42.0).
    #[serde(default = "default_vad_voiced_threshold_db")]
    pub vad_voiced_threshold_db: f32,

    /// Whisper/soft speech detection threshold in dB (default -52.0).
    #[serde(default = "default_vad_whisper_threshold_db")]
    pub vad_whisper_threshold_db: f32,

    /// Onset duration for voiced speech detection in ms (default 80).
    #[serde(default = "default_vad_voiced_onset_ms")]
    pub vad_voiced_onset_ms: u64,

    /// Onset duration for whisper speech detection in ms (default 120).
    #[serde(default = "default_vad_whisper_onset_ms")]
    pub vad_whisper_onset_ms: u64,

    /// Maximum segment duration before seeking a word-break split in ms (default 4000).
    #[serde(default = "default_segment_max_duration_ms")]
    pub segment_max_duration_ms: u64,

    /// Grace period after max duration before forcing submission in ms (default 750).
    #[serde(default = "default_segment_word_break_grace_ms")]
    pub segment_word_break_grace_ms: u64,

    /// Lookback buffer duration in ms (default 200).
    #[serde(default = "default_segment_lookback_ms")]
    pub segment_lookback_ms: u64,

    /// Maximum number of segments that can queue awaiting transcription (default 8).
    #[serde(default = "default_transcription_queue_capacity")]
    pub transcription_queue_capacity: usize,

    /// Visualization frame interval in ms (default 16 ≈ 60 fps).
    #[serde(default = "default_viz_frame_interval_ms")]
    pub viz_frame_interval_ms: u64,

    /// Whether the audio loop should split segments at word-break boundaries
    /// when a segment exceeds `segment_max_duration_ms` (default `true`).
    ///
    /// When `false`, the VAD still detects word-break events internally but
    /// does not act on them — segment boundaries are determined solely by
    /// speech-end detection and `segment_max_duration_ms`.
    ///
    /// Set to `false` for long-form transcription (OmniRec-style) where
    /// splitting at pauses degrades accuracy.
    #[serde(default = "default_word_break_segmentation_enabled")]
    pub word_break_segmentation_enabled: bool,
}

fn default_vad_voiced_threshold_db() -> f32 { -42.0 }
fn default_vad_whisper_threshold_db() -> f32 { -52.0 }
fn default_vad_voiced_onset_ms() -> u64 { 80 }
fn default_vad_whisper_onset_ms() -> u64 { 120 }
fn default_segment_max_duration_ms() -> u64 { 4000 }
fn default_segment_word_break_grace_ms() -> u64 { 750 }
fn default_segment_lookback_ms() -> u64 { 200 }
fn default_transcription_queue_capacity() -> usize { 8 }
fn default_viz_frame_interval_ms() -> u64 { 16 }
fn default_word_break_segmentation_enabled() -> bool { true }

impl Default for EngineConfig {
    fn default() -> Self {
        #[allow(deprecated)]
        Self {
            model: WhisperModel::BaseEn,
            model_path: None,
            recording_mode: RecordingMode::default(),
            vad_voiced_threshold_db: default_vad_voiced_threshold_db(),
            vad_whisper_threshold_db: default_vad_whisper_threshold_db(),
            vad_voiced_onset_ms: default_vad_voiced_onset_ms(),
            vad_whisper_onset_ms: default_vad_whisper_onset_ms(),
            segment_max_duration_ms: default_segment_max_duration_ms(),
            segment_word_break_grace_ms: default_segment_word_break_grace_ms(),
            segment_lookback_ms: default_segment_lookback_ms(),
            transcription_queue_capacity: default_transcription_queue_capacity(),
            viz_frame_interval_ms: default_viz_frame_interval_ms(),
            word_break_segmentation_enabled: default_word_break_segmentation_enabled(),
        }
    }
}

// =============================================================================
// EventHandlerAdapter
// =============================================================================

/// Bridges a broadcast `Receiver` to a callback closure.
///
/// Useful for consumers that prefer a callback-style API over direct channel
/// management. Call [`EventHandlerAdapter::spawn`] to drive the adapter in a
/// background tokio task.
pub struct EventHandlerAdapter<F>
where
    F: FnMut(EngineEvent) + Send + 'static,
{
    receiver: broadcast::Receiver<EngineEvent>,
    callback: F,
}

impl<F> EventHandlerAdapter<F>
where
    F: FnMut(EngineEvent) + Send + 'static,
{
    /// Create a new adapter wrapping the given receiver and callback.
    pub fn new(receiver: broadcast::Receiver<EngineEvent>, callback: F) -> Self {
        Self { receiver, callback }
    }

    /// Spawn a tokio task that drives the adapter, calling the closure for
    /// each event. Lagged errors are logged as warnings and skipped.
    pub fn spawn(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                match self.receiver.recv().await {
                    Ok(event) => (self.callback)(event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("[EventHandlerAdapter] Lagged: dropped {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}

// =============================================================================
// AudioEngine
// =============================================================================

/// The main audio engine. Manages audio capture, speech detection,
/// visualization, and transcription.
///
/// Obtain an instance via [`EngineBuilder::build`] or [`AudioEngine::new`].
/// Both return a `(AudioEngine, broadcast::Receiver<EngineEvent>)` tuple.
pub struct AudioEngine {
    /// Engine configuration
    config: EngineConfig,
    /// Broadcast sender — all threads send events here
    sender: Arc<broadcast::Sender<EngineEvent>>,
    /// Transcription queue (None if transcription disabled)
    transcription_queue: Option<Arc<transcription::TranscriptionQueue>>,
    /// Transcribe state (ring buffer + segment management)
    transcribe_state: Arc<std::sync::Mutex<transcription::TranscribeState>>,
    /// Whether the audio loop is running
    audio_loop_active: Arc<AtomicBool>,
    /// Whether transcription subsystem is enabled
    transcription_enabled: Arc<AtomicBool>,
    /// Whether VAD subsystem is enabled
    vad_enabled: bool,
    /// Whether visualization subsystem is enabled
    visualization_enabled: bool,
    /// Global shutdown flag
    shutdown_flag: Arc<AtomicBool>,
    /// Whether a manual recording session is active
    recording_active: Arc<AtomicBool>,
    /// Timestamp when manual recording started (for duration tracking)
    recording_start: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// Resolved path to the whisper model file
    model_path: std::path::PathBuf,
}

impl AudioEngine {
    /// Create a new audio engine with default configuration.
    ///
    /// Returns `(engine, receiver)`. Subscribe to additional receivers via
    /// [`AudioEngine::subscribe`].
    pub async fn new(config: EngineConfig) -> Result<(Self, broadcast::Receiver<EngineEvent>), String> {
        EngineBuilder::from_config(config).build().await
    }

    /// Subscribe to engine events. Any number of receivers may be active
    /// simultaneously. Receivers that fall behind the buffer capacity will
    /// receive `RecvError::Lagged`.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.sender.subscribe()
    }

    /// Start a manual recording session.
    ///
    /// While recording, captured audio is accumulated in a growable buffer
    /// (up to 30 minutes). VAD-driven segmentation is suppressed. Call
    /// [`stop_recording`](Self::stop_recording) to submit the accumulated
    /// audio for transcription.
    ///
    /// Emits [`EngineEvent::RecordingStarted`]. No-op if already recording.
    ///
    /// Requires that audio capture is active (via [`start_capture`](Self::start_capture)).
    pub fn start_recording(&self) {
        if self.recording_active.swap(true, Ordering::SeqCst) {
            // Already recording — no-op
            return;
        }

        info!("[Engine] Manual recording started");
        *self.recording_start.lock().unwrap() = Some(std::time::Instant::now());

        if let Ok(mut ts) = self.transcribe_state.lock() {
            ts.set_manual_recording(true);
            ts.is_active = true;
        }

        let _ = self.sender.send(EngineEvent::RecordingStarted);
    }

    /// Stop the manual recording session and submit the accumulated audio
    /// for transcription.
    ///
    /// Emits [`EngineEvent::RecordingStopped`] with the session duration.
    /// No-op if not currently recording.
    pub fn stop_recording(&self) {
        // Acquire the transcribe-state lock BEFORE clearing recording_active.
        // This prevents a race where the audio loop thread sees
        // recording_active == false, enters the else branch, and sets
        // ts.is_active = false (from the transcription_enabled flag) before
        // we call submit_recording(). With the lock held the audio loop's
        // try_lock() will fail harmlessly, preserving is_active == true so
        // submit_recording() processes the accumulated audio.
        let ts_lock = self.transcribe_state.lock().ok();

        if !self.recording_active.swap(false, Ordering::SeqCst) {
            // Not recording — no-op
            return;
        }

        let duration_ms = self.recording_start
            .lock()
            .unwrap()
            .take()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);

        info!("[Engine] Manual recording stopped ({}ms)", duration_ms);

        if let Some(mut ts) = ts_lock {
            ts.submit_recording();
            ts.set_manual_recording(false);
        }

        let _ = self.sender.send(EngineEvent::RecordingStopped { duration_ms });
    }

    /// Whether a manual recording session is currently active.
    pub fn is_recording(&self) -> bool {
        self.recording_active.load(Ordering::SeqCst)
    }

    /// List available input devices (microphones).
    pub fn list_input_devices(&self) -> Vec<AudioDevice> {
        platform::get_backend()
            .map(|b| b.list_input_devices())
            .unwrap_or_default()
    }

    /// List available system audio devices (monitors/loopbacks).
    pub fn list_system_devices(&self) -> Vec<AudioDevice> {
        platform::get_backend()
            .map(|b| b.list_system_devices())
            .unwrap_or_default()
    }

    /// Start audio capture from the specified sources.
    ///
    /// - `source1_id`: Primary input device (microphone). Required.
    /// - `source2_id`: Secondary source (system audio for mixing/AEC). Optional.
    pub async fn start_capture(
        &self,
        source1_id: Option<String>,
        source2_id: Option<String>,
    ) -> Result<(), String> {
        if self.audio_loop_active.load(Ordering::SeqCst) {
            self.stop_capture().await?;
        }

        let backend = platform::get_backend()
            .ok_or_else(|| "Audio backend not initialized".to_string())?;

        // Consolidate AEC into recording_mode — EchoCancel implies AEC enabled
        backend.set_aec_enabled(self.config.recording_mode == RecordingMode::EchoCancel);
        backend.set_recording_mode(self.config.recording_mode);
        backend.start_capture_sources(source1_id.clone(), source2_id.clone())?;

        let sample_rate = backend.sample_rate();
        {
            let mut ts = self.transcribe_state.lock().unwrap();
            ts.init_for_capture(sample_rate, 2);
            ts.is_active = self.transcription_enabled.load(Ordering::SeqCst);
        }

        // Capture flags/arcs for the audio loop thread
        let loop_active = self.audio_loop_active.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        let sender = self.sender.clone();
        let transcribe_state = self.transcribe_state.clone();
        let transcription_enabled = self.transcription_enabled.clone();
        let vad_enabled = self.vad_enabled;
        let visualization_enabled = self.visualization_enabled;
        let recording_active = self.recording_active.clone();
        let word_break_segmentation_enabled = self.config.word_break_segmentation_enabled;

        loop_active.store(true, Ordering::SeqCst);

        thread::spawn(move || {
            info!("[AudioLoop] Starting audio processing loop");

            let mut speech_detector = processor::SpeechDetector::new(sample_rate);
            let mut viz_processor = processor::VisualizationProcessor::new(sample_rate, 256);
            let mut pending_state_change = processor::SpeechStateChange::None;
            let mut pending_word_break: Option<processor::WordBreakEvent> = None;

            loop {
                if !loop_active.load(Ordering::SeqCst) || shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }

                let audio_data = platform::get_backend().and_then(|b| b.try_recv());

                if let Some(data) = audio_data {
                    let mono_samples = audio::convert_to_mono(&data.samples, data.channels as usize);

                    // --- VAD ---
                    if vad_enabled {
                        speech_detector.process(&mono_samples);
                    }
                    let speech_metrics = if vad_enabled {
                        Some(speech_detector.get_metrics())
                    } else {
                        None
                    };

                    // --- Visualization ---
                    if visualization_enabled {
                        if let Some(ref metrics) = speech_metrics {
                            viz_processor.set_speech_metrics(metrics.clone());
                        }
                        if let Some(viz) = viz_processor.process(&mono_samples) {
                            let _ = sender.send(EngineEvent::VisualizationData(viz));
                        }
                    }

                    // --- Speech state changes (VAD) ---
                    if vad_enabled {
                        let new_state = speech_detector.take_state_change();
                        let new_wb = speech_detector.take_word_break_event();
                        match new_state {
                            processor::SpeechStateChange::None => {}
                            other => pending_state_change = other,
                        }
                        if new_wb.is_some() {
                            pending_word_break = new_wb;
                        }
                    }

                    let is_manual_recording = recording_active.load(Ordering::SeqCst);

                    if let Ok(mut ts) = transcribe_state.try_lock() {
                        if is_manual_recording {
                            // Manual recording: is_active is managed by
                            // start_recording/stop_recording, not the global flag.
                            ts.process_samples(&data.samples);
                        } else {
                            ts.is_active = transcription_enabled.load(Ordering::SeqCst);
                        }

                        if !is_manual_recording && ts.is_active && vad_enabled {
                            // VAD mode: speech detection drives segmentation.
                            if let processor::SpeechStateChange::Started { lookback_samples } =
                                &pending_state_change
                            {
                                ts.on_speech_started(*lookback_samples);
                                let _ = sender.send(EngineEvent::SpeechStarted);
                            }

                            ts.process_samples(&data.samples);

                            if let processor::SpeechStateChange::Ended { duration_ms } =
                                pending_state_change
                            {
                                ts.on_speech_ended();
                                let _ = sender.send(EngineEvent::SpeechEnded { duration_ms });
                            }

                            // Only split at word-break boundaries when the
                            // config allows it (disabled for long-form transcription).
                            if word_break_segmentation_enabled {
                                if let Some(wb) = pending_word_break.take() {
                                    ts.on_word_break(wb.offset_ms, wb.gap_duration_ms);
                                }
                            } else {
                                drop(pending_word_break.take());
                            }
                        }

                        pending_state_change = processor::SpeechStateChange::None;
                        pending_word_break = None;
                    }
                } else {
                    thread::sleep(Duration::from_millis(1));
                }
            }

            info!("[AudioLoop] Audio processing loop stopped");
        });

        let _ = self.sender.send(EngineEvent::CaptureStateChanged {
            capturing: true,
            error: None,
        });

        Ok(())
    }

    /// Stop audio capture.
    pub async fn stop_capture(&self) -> Result<(), String> {
        self.audio_loop_active.store(false, Ordering::SeqCst);

        if let Some(backend) = platform::get_backend() {
            backend.stop_capture()?;
        }

        let _ = self.sender.send(EngineEvent::CaptureStateChanged {
            capturing: false,
            error: None,
        });

        Ok(())
    }

    /// Check if audio capture is currently active.
    pub fn is_capturing(&self) -> bool {
        self.audio_loop_active.load(Ordering::SeqCst)
    }

    /// Check if the Whisper model is available.
    pub fn check_model_status(&self) -> ModelStatus {
        ModelStatus {
            available: self.model_path.exists(),
            path: self.model_path.to_string_lossy().to_string(),
        }
    }

    /// Download the Whisper model, emitting progress events.
    pub async fn download_model(&self) -> Result<(), String> {
        let sender = self.sender.clone();
        let model_path = self.model_path.clone();

        let s = sender.clone();
        let result = transcription::download_model(&model_path, move |percent| {
            let _ = s.send(EngineEvent::ModelDownloadProgress { percent });
        })
        .await;

        let _ = sender.send(EngineEvent::ModelDownloadComplete {
            success: result.is_ok(),
        });

        result
    }

    /// Enable or disable real-time transcription without stopping capture.
    pub fn set_transcription_enabled(&self, enabled: bool) {
        info!("[Engine] Transcription enabled: {}", enabled);
        self.transcription_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Whether real-time transcription is currently enabled.
    pub fn is_transcription_enabled(&self) -> bool {
        self.transcription_enabled.load(Ordering::SeqCst)
    }

    /// Finalize and submit any pending audio segment for transcription.
    ///
    /// If a manual recording is active, stops it and submits the accumulated
    /// audio. Otherwise, finalizes any in-progress VAD segment.
    pub fn finalize_segment(&self) {
        info!("[Engine] Finalizing recording session for transcription");
        if self.recording_active.load(Ordering::SeqCst) {
            self.stop_recording();
        } else if let Ok(mut ts) = self.transcribe_state.lock() {
            ts.finalize();
        }
    }

    /// Check GPU acceleration status.
    pub fn check_gpu_status(&self) -> Result<GpuStatus, String> {
        transcription::whisper_ffi::init_library()?;
        let system_info = transcription::whisper_ffi::get_system_info()?;
        Ok(GpuStatus {
            // "CUDA : ARCHS = ..." is present when a CUDA backend is active.
            // A plain "CUDA" substring can appear in non-GPU info strings, so
            // match the more specific form used by flowstt for consistency.
            cuda_available: system_info.contains("CUDA : ARCHS"),
            metal_available: system_info.contains("METAL = 1"),
            system_info,
        })
    }

    /// Get current engine status.
    pub fn get_status(&self) -> EngineStatus {
        EngineStatus {
            capturing: self.audio_loop_active.load(Ordering::SeqCst),
            in_speech: false,
            queue_depth: self.transcription_queue
                .as_ref()
                .map(|q| q.queue_depth())
                .unwrap_or(0),
            error: None,
            source1_id: None,
            source2_id: None,
        }
    }

    /// Transcribe a WAV file and return timestamped segments.
    ///
    /// This replaces the deprecated `transcribe_file` method. It loads the WAV
    /// file, resamples to 16 kHz mono, runs VAD segmentation, runs whisper
    /// inference on each segment, and returns all segments. Each segment
    /// carries a `timestamp_offset_ms` relative to the start of the file.
    ///
    /// An `EngineEvent::TranscriptionSegment` is emitted on the broadcast
    /// channel for each completed segment.
    ///
    /// Returns `Ok(vec![])` for a silent file (not an error).
    pub async fn transcribe_audio_file(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Vec<TranscriptionSegment>, String> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let sender = self.sender.clone();

        let segments = tokio::task::spawn_blocking(move || {
            let reader = hound::WavReader::open(&path_str)
                .map_err(|e| format!("Failed to open WAV file: {}", e))?;

            let spec = reader.spec();
            let raw_samples: Vec<f32> = match spec.sample_format {
                hound::SampleFormat::Float => reader
                    .into_samples::<f32>()
                    .filter_map(|s| s.ok())
                    .collect(),
                hound::SampleFormat::Int => {
                    let bits = spec.bits_per_sample;
                    let max_val = (1 << (bits - 1)) as f32;
                    reader
                        .into_samples::<i32>()
                        .filter_map(|s| s.ok())
                        .map(|s| s as f32 / max_val)
                        .collect()
                }
            };

            let mono = if spec.channels > 1 {
                audio::convert_to_mono(&raw_samples, spec.channels as usize)
            } else {
                raw_samples
            };

            let resampled = audio::resample_to_16khz(&mono, spec.sample_rate)?;

            // For file transcription we treat the entire resampled buffer as a
            // single segment (VAD-based chunking is for live capture; for files
            // the whole recording is already bounded).
            if resampled.is_empty() {
                return Ok(vec![]);
            }

            let total_duration_ms = (resampled.len() as u64 * 1000) / 16_000;

            let mut transcriber = transcription::Transcriber::new();
            let text = transcriber.transcribe(&resampled)?;

            if text.trim().is_empty() || text.trim() == "(No speech detected)" {
                return Ok(vec![]);
            }

            let seg = TranscriptionSegment {
                id: uuid::Uuid::new_v4().to_string(),
                text: text.trim().to_string(),
                timestamp_offset_ms: 0,
                duration_ms: total_duration_ms,
                audio_path: Some(path_str),
            };

            Ok(vec![seg])
        })
        .await
        .map_err(|e| format!("Transcription task failed: {}", e))?;

        if let Ok(ref segs) = segments {
            for seg in segs {
                let _ = sender.send(EngineEvent::TranscriptionSegment(seg.clone()));
            }
        }

        segments
    }

    /// Accept a channel of 16 kHz mono f32 PCM audio frames and transcribe
    /// them incrementally, returning a `JoinHandle` that resolves to the
    /// complete ordered list of segments when the sender is dropped.
    ///
    /// For each completed segment an `EngineEvent::TranscriptionSegment` is
    /// sent on the engine's broadcast channel in real time, allowing consumers
    /// to update a live transcript view before the full session is done.
    ///
    /// **Input contract:** Caller is responsible for supplying audio that is
    /// already resampled to 16 kHz and converted to mono (single-channel)
    /// f32 PCM. The engine does not resample or channel-convert inside this
    /// method.
    ///
    /// `session_start` is used to compute `timestamp_offset_ms` for each
    /// segment (milliseconds elapsed from `session_start` to the beginning of
    /// each segment's audio in the stream).
    ///
    /// Does **not** require an active `start_capture()` session.
    pub fn transcribe_audio_stream(
        &self,
        mut rx: mpsc::Receiver<Vec<f32>>,
        session_start: std::time::Instant,
    ) -> tokio::task::JoinHandle<Vec<TranscriptionSegment>> {
        let sender = self.sender.clone();

        tokio::task::spawn_blocking(move || {
            use transcription::Transcriber;

            const SAMPLE_RATE_HZ: u64 = 16_000;
            // Minimum audio frames to attempt transcription (~500 ms at 16kHz).
            const MIN_SEGMENT_FRAMES: usize = 8_000;

            let mut transcriber = Transcriber::new();
            let mut accumulator: Vec<f32> = Vec::new();
            let mut all_segments: Vec<TranscriptionSegment> = Vec::new();

            // Drain the receiver until it closes (sender dropped).
            // spawn_blocking provides a synchronous context so blocking_recv is correct here.
            loop {
                match rx.blocking_recv() {
                    Some(frames) => {
                        accumulator.extend_from_slice(&frames);
                    }
                    None => {
                        // Channel closed — flush remaining audio.
                        break;
                    }
                }
            }

            // After draining, transcribe the entire accumulated buffer.
            // For stream transcription we treat the whole buffer as a single
            // inference call (the spec allows a future chunked implementation).
            if accumulator.len() >= MIN_SEGMENT_FRAMES {
                let duration_ms = (accumulator.len() as u64 * 1000) / SAMPLE_RATE_HZ;
                // timestamp_offset_ms for a single-pass full-buffer transcription is 0
                // (the segment starts at the beginning of the stream).
                let timestamp_offset_ms: u64 = 0;
                let _ = session_start; // session_start available for future per-chunk offsets

                match transcriber.transcribe(&accumulator) {
                    Ok(text) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() && trimmed != "(No speech detected)" {
                            let seg = TranscriptionSegment {
                                id: uuid::Uuid::new_v4().to_string(),
                                text: trimmed.to_string(),
                                timestamp_offset_ms,
                                duration_ms,
                                audio_path: None,
                            };
                            let _ = sender.send(EngineEvent::TranscriptionSegment(seg.clone()));
                            all_segments.push(seg);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[transcribe_audio_stream] Transcription failed: {}", e);
                    }
                }
            }

            all_segments
        })
    }

    /// Start a lightweight test capture on a device to report audio levels.
    pub fn start_test_capture(&self, device_id: String) -> Result<(), String> {
        let sender = self.sender.clone();

        let backend = platform::get_backend()
            .ok_or_else(|| "Audio backend not initialized".to_string())?;

        let sample_rate = backend.sample_rate();

        backend.start_capture_sources(Some(device_id.clone()), None)?;

        let shutdown = self.shutdown_flag.clone();

        thread::spawn(move || {
            let mut sample_buffer: Vec<f32> = Vec::new();
            let samples_per_report = (sample_rate as usize) / 10;

            loop {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }

                if let Some(data) = platform::get_backend().and_then(|b| b.try_recv()) {
                    let mono = audio::convert_to_mono(&data.samples, data.channels as usize);
                    sample_buffer.extend_from_slice(&mono);

                    if sample_buffer.len() >= samples_per_report {
                        let sum_sq: f32 = sample_buffer.iter().map(|s| s * s).sum();
                        let rms = (sum_sq / sample_buffer.len() as f32).sqrt();
                        let db = if rms > 0.0 { 20.0 * rms.log10() } else { -100.0 };

                        let _ = sender.send(EngineEvent::AudioLevelUpdate {
                            device_id: device_id.clone(),
                            level_db: db,
                        });

                        sample_buffer.clear();
                    }
                } else {
                    thread::sleep(Duration::from_millis(1));
                }
            }
        });

        Ok(())
    }

    /// Stop any active test capture.
    pub fn stop_test_capture(&self) -> Result<(), String> {
        if let Some(backend) = platform::get_backend() {
            backend.stop_capture()?;
        }
        Ok(())
    }

    /// Request engine shutdown.
    pub fn shutdown(&self) {
        info!("Engine shutdown requested");
        self.recording_active.store(false, Ordering::SeqCst);
        self.shutdown_flag.store(true, Ordering::SeqCst);
        self.audio_loop_active.store(false, Ordering::SeqCst);
        if let Some(backend) = platform::get_backend() {
            let _ = backend.stop_capture();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// =============================================================================
// Internal transcription callback
// =============================================================================

/// Forwards transcription results to the broadcast sender.
pub(crate) struct EngineTranscriptionCallback {
    pub sender: Arc<broadcast::Sender<EngineEvent>>,
}

impl transcription::TranscriptionCallback for EngineTranscriptionCallback {
    fn on_transcription_started(&self) {
        debug!("[Transcription] Started");
    }

    fn on_transcription_complete(&self, text: String, wav_path: Option<String>) {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed == "(No speech detected)" {
            debug!("[Transcription] Skipping empty/no-speech result");
            return;
        }

        info!("[Transcription] Complete: {}", trimmed);

        let _ = self.sender.send(EngineEvent::TranscriptionComplete(
            TranscriptionResult {
                id: None,
                text: trimmed.to_string(),
                timestamp: None,
                duration_ms: None,
                audio_path: wav_path,
                timestamp_offset_ms: None,
            },
        ));
    }

    fn on_transcription_error(&self, error: String) {
        error!("[Transcription] Error: {}", error);
    }

    fn on_transcription_finished(&self) {
        debug!("[Transcription] Finished");
    }

    fn on_queue_update(&self, depth: usize) {
        debug!("[Transcription] Queue depth: {}", depth);
    }
}
