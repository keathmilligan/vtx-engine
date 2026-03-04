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
pub mod platform;
pub mod processor;
pub mod ptt;
pub mod transcription;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use vtx_common::*;

// Re-export common types and new public types
pub use vtx_common;
pub use builder::EngineBuilder;
pub use config_persistence::ConfigError;
pub use history::{HistoryError, TranscriptionHistory, TranscriptionHistoryRecorder};
pub use ptt::PushToTalkController;

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
    /// Override model file location. When `None`, the default platform
    /// data directory is used.
    #[serde(default)]
    pub model_path: Option<PathBuf>,

    /// Recording mode: `Mixed` combines both sources; `EchoCancel` applies
    /// AEC and outputs only the echo-cancelled primary source.
    #[serde(default)]
    pub recording_mode: RecordingMode,

    /// Transcription mode: `Automatic` uses VAD; `PushToTalk` uses
    /// explicit press/release signals via [`PushToTalkController`].
    #[serde(default)]
    pub transcription_mode: TranscriptionMode,

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

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            recording_mode: RecordingMode::default(),
            transcription_mode: TranscriptionMode::default(),
            vad_voiced_threshold_db: default_vad_voiced_threshold_db(),
            vad_whisper_threshold_db: default_vad_whisper_threshold_db(),
            vad_voiced_onset_ms: default_vad_voiced_onset_ms(),
            vad_whisper_onset_ms: default_vad_whisper_onset_ms(),
            segment_max_duration_ms: default_segment_max_duration_ms(),
            segment_word_break_grace_ms: default_segment_word_break_grace_ms(),
            segment_lookback_ms: default_segment_lookback_ms(),
            transcription_queue_capacity: default_transcription_queue_capacity(),
            viz_frame_interval_ms: default_viz_frame_interval_ms(),
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
    /// PTT shared state (shared with PushToTalkController)
    ptt_state: Arc<std::sync::Mutex<ptt::PttState>>,
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

    /// Get a [`PushToTalkController`] for this engine. The controller is
    /// `Clone + Send` and can be moved to any thread or task.
    pub fn ptt_controller(&self) -> PushToTalkController {
        PushToTalkController::new(
            self.ptt_state.clone(),
            self.sender.clone(),
            self.transcribe_state.clone(),
        )
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
        let ptt_state = self.ptt_state.clone();
        let transcription_mode = self.config.transcription_mode;

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

                    // --- Speech state changes (VAD only, not PTT) ---
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

                    if let Ok(mut ts) = transcribe_state.try_lock() {
                        ts.is_active = transcription_enabled.load(Ordering::SeqCst);

                        // In PTT mode the PTT controller drives segment lifecycle;
                        // suppress VAD-based speech events.
                        let use_vad_segments = vad_enabled
                            && transcription_mode != TranscriptionMode::PushToTalk;

                        if ts.is_active && use_vad_segments {
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

                            if let Some(wb) = pending_word_break.take() {
                                ts.on_word_break(wb.offset_ms, wb.gap_duration_ms);
                            }
                        } else if ts.is_active {
                            // PTT mode: still write samples into the ring buffer
                            // but don't act on VAD events.
                            let is_ptt_active = ptt_state
                                .lock()
                                .map(|s| s.is_active)
                                .unwrap_or(false);
                            if is_ptt_active {
                                ts.process_samples(&data.samples);
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
        let transcriber = transcription::Transcriber::new();
        ModelStatus {
            available: transcriber.is_model_available(),
            path: transcriber.get_model_path().to_string_lossy().to_string(),
        }
    }

    /// Download the Whisper model, emitting progress events.
    pub async fn download_model(&self) -> Result<(), String> {
        let sender = self.sender.clone();
        let model_path = transcription::Transcriber::new().get_model_path().clone();

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

    /// Finalize and submit the current recording session for transcription.
    ///
    /// In PTT mode, this submits the audio accumulated since the last
    /// `press()`. Prefer using [`PushToTalkController::release`] instead,
    /// which calls this automatically.
    pub fn finalize_segment(&self) {
        info!("[Engine] Finalizing recording session for transcription");
        if let Ok(mut ts) = self.transcribe_state.lock() {
            if ts.ptt_mode {
                ts.submit_session();
            } else {
                ts.finalize();
            }
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

    /// Transcribe a WAV file and return the result.
    pub async fn transcribe_file(&self, path: String) -> Result<TranscriptionResult, String> {
        let sender = self.sender.clone();

        let result = tokio::task::spawn_blocking(move || {
            let reader = hound::WavReader::open(&path)
                .map_err(|e| format!("Failed to open WAV file: {}", e))?;

            let spec = reader.spec();
            let samples: Vec<f32> = match spec.sample_format {
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
                audio::convert_to_mono(&samples, spec.channels as usize)
            } else {
                samples
            };

            let resampled = audio::resample_to_16khz(&mono, spec.sample_rate)?;

            let mut transcriber = transcription::Transcriber::new();
            let text = transcriber.transcribe(&resampled)?;

            let duration_ms = Some((mono.len() as u64 * 1000) / spec.sample_rate as u64);

            Ok(TranscriptionResult {
                id: None,
                text,
                timestamp: None,
                duration_ms,
                audio_path: Some(path),
            })
        })
        .await
        .map_err(|e| format!("Transcription task failed: {}", e))?;

        if let Ok(ref result) = result {
            let _ = sender.send(EngineEvent::TranscriptionComplete(result.clone()));
        }

        result
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
