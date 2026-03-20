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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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

    /// Software microphone input gain in dB applied to raw PCM before the
    /// VAD and transcription pipeline (default 0.0 dB = no change).
    ///
    /// The gain is applied as a linear multiplier (`10^(db/20)`) and output
    /// samples are clamped to `[-1.0, 1.0]` to prevent clipping.
    /// Recommended range: -20.0 to +20.0 dB.
    #[serde(default = "default_mic_gain_db")]
    pub mic_gain_db: f32,

    /// Automatic Gain Control configuration (default: disabled).
    ///
    /// When enabled, an RMS envelope-follower algorithm continuously adjusts
    /// the gain to maintain a target output level. Operates after `mic_gain_db`.
    #[serde(default)]
    pub agc: AgcConfig,
}

// =============================================================================
// AgcConfig
// =============================================================================

/// Configuration for the Automatic Gain Control (AGC) stage.
///
/// AGC uses a feed-back RMS envelope follower with separate attack and release
/// time constants to maintain a consistent output level regardless of
/// microphone sensitivity or input level variation.
///
/// The AGC stage is inserted after the `mic_gain_db` manual gain stage and
/// before the VAD and visualization stages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgcConfig {
    /// Whether AGC is active (default `false`).
    ///
    /// When `false`, the AGC stage is bypassed entirely with no processing cost.
    #[serde(default)]
    pub enabled: bool,

    /// Target RMS output level in dBFS (default -18.0).
    ///
    /// The AGC attempts to keep the RMS of processed audio near this level.
    /// Recommended range: -30.0 to -6.0 dBFS.
    #[serde(default = "default_agc_target_level_db")]
    pub target_level_db: f32,

    /// Gain reduction time constant in milliseconds (default 10.0).
    ///
    /// Controls how quickly the AGC reduces gain when the input gets louder.
    /// Shorter values respond faster but may cause audible pumping on transients.
    #[serde(default = "default_agc_attack_time_ms")]
    pub attack_time_ms: f32,

    /// Gain increase time constant in milliseconds (default 200.0).
    ///
    /// Controls how quickly the AGC increases gain when the input gets quieter.
    /// Longer values prevent the AGC from amplifying noise between words.
    #[serde(default = "default_agc_release_time_ms")]
    pub release_time_ms: f32,

    /// Minimum allowable AGC gain in dB (default -6.0).
    ///
    /// Prevents the AGC from attenuating the signal excessively.
    #[serde(default = "default_agc_min_gain_db")]
    pub min_gain_db: f32,

    /// Maximum allowable AGC gain in dB (default 30.0).
    ///
    /// Caps the AGC gain to prevent extreme amplification of quiet/silent input.
    #[serde(default = "default_agc_max_gain_db")]
    pub max_gain_db: f32,

    /// Gate threshold in dBFS (default -50.0).
    ///
    /// When the smoothed power estimate falls below this level, the AGC decays
    /// its gain toward unity (1.0) instead of continuing to boost. This prevents
    /// the AGC from amplifying background noise during speech pauses.
    /// Recommended range: -60.0 to -30.0 dBFS.
    #[serde(default = "default_agc_gate_threshold_db")]
    pub gate_threshold_db: f32,
}

fn default_agc_target_level_db() -> f32 { -18.0 }
fn default_agc_attack_time_ms() -> f32 { 10.0 }
fn default_agc_release_time_ms() -> f32 { 200.0 }
fn default_agc_min_gain_db() -> f32 { -6.0 }
fn default_agc_max_gain_db() -> f32 { 30.0 }
fn default_agc_gate_threshold_db() -> f32 { -50.0 }

impl Default for AgcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_level_db: default_agc_target_level_db(),
            attack_time_ms: default_agc_attack_time_ms(),
            release_time_ms: default_agc_release_time_ms(),
            min_gain_db: default_agc_min_gain_db(),
            max_gain_db: default_agc_max_gain_db(),
            gate_threshold_db: default_agc_gate_threshold_db(),
        }
    }
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
fn default_mic_gain_db() -> f32 { 0.0 }

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
            mic_gain_db: default_mic_gain_db(),
            agc: AgcConfig::default(),
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
    /// Runtime recording-mode override.
    ///
    /// When `Some`, this value is used instead of `config.recording_mode` for
    /// every `start_capture` call.  Updated via `set_recording_mode` without
    /// requiring `&mut self`.
    recording_mode_override: Arc<std::sync::Mutex<Option<RecordingMode>>>,
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
    /// Whether processed audio streaming is enabled
    audio_streaming_enabled: bool,
    /// Whether raw audio streaming is enabled
    raw_audio_streaming_enabled: bool,
    /// Global shutdown flag
    shutdown_flag: Arc<AtomicBool>,
    /// Whether a manual recording session is active
    recording_active: Arc<AtomicBool>,
    /// Timestamp when manual recording started (for duration tracking)
    recording_start: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// Resolved path to the whisper model file
    model_path: std::path::PathBuf,
    /// Path of the most recently saved WAV file from a manual recording session
    last_recording_path: Arc<std::sync::Mutex<Option<std::path::PathBuf>>>,
    /// Sender side of the audio injection channel (for file playback).
    /// Sending `AudioData` here feeds samples into the audio loop as if they
    /// came from the hardware backend.
    playback_tx: Arc<std::sync::Mutex<Option<std::sync::mpsc::SyncSender<AudioData>>>>,
    /// Flag set while a file is being played back through the injection channel.
    playback_active: Arc<AtomicBool>,
    /// Mic gain in dB stored as f32 bits in an AtomicU32 for lock-free hot-update.
    mic_gain_db: Arc<AtomicU32>,
    /// Whether PTT (push-to-talk / manual) mode is active.
    /// When `true`, VAD-driven segmentation is suppressed and the user
    /// must manually start/stop recordings. When `false` (auto-transcription
    /// mode), VAD drives segmentation automatically.
    ptt_mode: Arc<AtomicBool>,
    /// AGC configuration shared with the capture thread.
    ///
    /// The capture thread reads this via `try_lock` once per chunk.
    /// The public API writes it via `set_agc_config`.
    agc_config: Arc<std::sync::Mutex<AgcConfig>>,
    /// Sender for pushing processed audio to the render output endpoint
    /// during file playback.  Set by `play_file`, cleared on playback end.
    render_tx: Arc<std::sync::Mutex<Option<std::sync::mpsc::SyncSender<Vec<f32>>>>>,
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
            // In PTT mode: submit the whole accumulated buffer as one transcription segment.
            // In auto-transcription mode: the VAD already handled segmentation in real time;
            // just save the WAV so the session appears as the active document and can be
            // reprocessed, but don't re-queue it for transcription.
            if self.ptt_mode.load(Ordering::SeqCst) {
                ts.submit_recording();
                ts.set_manual_recording(false);
            } else {
                // Clear manual_recording first so that finalize() takes the VAD branch
                // (in_speech check) rather than the PTT branch (submit_recording).
                // This flushes any speech segment that was still in progress when the
                // user stopped — i.e. on_speech_ended() never fired because the VAD
                // hold timer hadn't elapsed yet.
                ts.set_manual_recording(false);
                ts.finalize();
                ts.save_recording_wav();
            }
        }

        let _ = self.sender.send(EngineEvent::RecordingStopped { duration_ms });
    }

    /// Whether a manual recording session is currently active.
    pub fn is_recording(&self) -> bool {
        self.recording_active.load(Ordering::SeqCst)
    }

    /// Return the path of the most recently saved WAV file from a manual
    /// recording session, or `None` if no recording has completed yet.
    pub fn get_last_recording_path(&self) -> Option<std::path::PathBuf> {
        self.last_recording_path.lock().ok()?.clone()
    }

    /// Whether a file playback is currently active.
    pub fn is_playing_back(&self) -> bool {
        self.playback_active.load(Ordering::SeqCst)
    }

    /// Stop an active file playback, if any.
    pub fn stop_playback(&self) {
        self.playback_active.store(false, Ordering::SeqCst);
        // Drop the render sender and stop the render endpoint immediately.
        {
            let _ = self.render_tx.lock().unwrap().take();
        }
        if let Some(backend) = platform::get_backend() {
            let _ = backend.stop_render();
        }
    }

    /// Play a WAV file through the full engine pipeline (visualization + VAD +
    /// transcription), exactly as if the audio were being captured live.
    ///
    /// - When `ptt_mode` is `true` the entire file is submitted as a single
    ///   manual recording segment (equivalent to press→play→release in PTT).
    /// - When `ptt_mode` is `false` the VAD drives automatic segmentation as
    ///   it would during live capture.
    ///
    /// The method returns as soon as the feeder thread is spawned. The caller
    /// can await completion by polling [`is_playing_back`](Self::is_playing_back)
    /// or listening for a `PlaybackComplete` event on the broadcast channel
    /// (emitted when the feeder thread finishes).
    ///
    /// Calling `play_file` while a playback is already in progress cancels the
    /// previous playback first.
    pub fn play_file(
        &self,
        path: impl AsRef<std::path::Path>,
        ptt_mode: bool,
    ) -> Result<(), String> {
        // Cancel any in-progress playback.
        self.stop_playback();

        // Resolve to the raw (unprocessed) WAV when a processed variant is
        // provided, so playback always reprocesses from the original recording.
        let resolved = crate::audio::resolve_raw_wav_path(path.as_ref());
        let path_str = resolved.to_string_lossy().to_string();

        // Read the WAV file and decode to interleaved f32 samples.
        let reader = hound::WavReader::open(&path_str)
            .map_err(|e| format!("Failed to open WAV file: {}", e))?;
        let spec = reader.spec();
        let wav_sample_rate = spec.sample_rate;
        let wav_channels = spec.channels;

        let raw_samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .filter_map(|s| s.ok())
                .collect(),
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                let max_val = (1u32 << (bits - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / max_val)
                    .collect()
            }
        };

        if raw_samples.is_empty() {
            return Ok(());
        }

        // Ensure the audio loop is running. If not, start one without a hardware
        // backend, using the WAV's own sample rate.
        let loop_was_running = self.audio_loop_active.load(Ordering::SeqCst);
        if !loop_was_running {
            let (inject_tx, inject_rx) = std::sync::mpsc::sync_channel::<AudioData>(2);
            *self.playback_tx.lock().unwrap() = Some(inject_tx);
            {
                let mut ts = self.transcribe_state.lock().unwrap();
                ts.init_for_capture(wav_sample_rate, wav_channels);
                ts.is_active = self.transcription_enabled.load(Ordering::SeqCst);
            }
            self.start_audio_loop(wav_sample_rate, inject_rx);
        }

        // Grab the injection sender.
        let tx = self.playback_tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Playback channel not available".to_string())?;

        self.playback_active.store(true, Ordering::SeqCst);

        // Extract the original recording stem from the source file so that
        // playback reprocessing overwrites the existing processed WAV instead
        // of creating a new timestamped file.
        let source_stem = crate::audio::extract_recording_stem(path.as_ref());
        {
            let mut ts = self.transcribe_state.lock().unwrap();
            ts.set_playback_source_stem(source_stem);
        }

        // Start the audio render endpoint so processed samples are played
        // through the speakers during playback.
        if let Some(backend) = platform::get_backend() {
            match backend.start_render() {
                Ok(render_sender) => {
                    *self.render_tx.lock().unwrap() = Some(render_sender);
                }
                Err(e) => {
                    warn!("[Playback] Could not start render output: {}", e);
                    // Non-fatal: playback still works for visualization and transcription.
                }
            }
        }

        // In PTT mode, start a manual recording session so the whole file is
        // submitted as one segment when playback ends.
        if ptt_mode {
            self.start_recording();
        }

        let playback_active = self.playback_active.clone();
        let recording_active = self.recording_active.clone();
        let transcribe_state_arc = self.transcribe_state.clone();
        let render_tx_arc = self.render_tx.clone();
        let sender = self.sender.clone();
        let samples_per_chunk = (wav_sample_rate as usize / 100) * wav_channels as usize; // 10ms chunks
        let chunk_duration = Duration::from_millis(10);

        thread::spawn(move || {
            info!("[Playback] Starting file playback: {}", path_str);

            for chunk in raw_samples.chunks(samples_per_chunk.max(1)) {
                if !playback_active.load(Ordering::SeqCst) {
                    info!("[Playback] Playback cancelled");
                    break;
                }

                let data = AudioData {
                    samples: chunk.to_vec(),
                    channels: wav_channels,
                    sample_rate: wav_sample_rate,
                };

                // send() blocks when the channel is full, providing real-time pacing.
                if tx.send(data).is_err() {
                    // Receiver dropped (loop stopped) — abort.
                    break;
                }

                thread::sleep(chunk_duration);
            }

            playback_active.store(false, Ordering::SeqCst);

            // Drop the render sender to signal the render thread that no more
            // samples are coming, then stop the render endpoint.
            {
                let _ = render_tx_arc.lock().unwrap().take();
            }
            if let Some(backend) = platform::get_backend() {
                let _ = backend.stop_render();
            }

            // In PTT mode, stop the manual recording session to submit the accumulated audio.
            if ptt_mode && recording_active.swap(false, Ordering::SeqCst) {
                // Give the audio loop a moment to drain the last injected chunk
                // before calling submit_recording(), so the full audio is captured.
                thread::sleep(Duration::from_millis(150));
                if let Ok(mut ts) = transcribe_state_arc.lock() {
                    ts.submit_recording();
                    ts.set_manual_recording(false);
                    ts.set_playback_source_stem(None);
                }
                let duration_ms = 0u64; // duration tracking not needed for playback PTT
                let _ = sender.send(EngineEvent::RecordingStopped { duration_ms });
            } else {
                // Non-PTT path: clear the playback source stem when playback ends.
                if let Ok(mut ts) = transcribe_state_arc.lock() {
                    ts.set_playback_source_stem(None);
                }
            }

            let _ = sender.send(EngineEvent::PlaybackComplete);
            info!("[Playback] File playback finished");
        });

        Ok(())
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

    /// Return the system default audio output device (loopback/render endpoint).
    ///
    /// On Windows this resolves the default render endpoint via
    /// `GetDefaultAudioEndpoint(eRender, eConsole)`.  On macOS it resolves
    /// the default output device via CoreAudio.  On Linux and other platforms
    /// this returns the first enumerated system device as a best-effort
    /// fallback.
    ///
    /// Returns `None` if no system devices are available.
    pub fn get_default_system_device(&self) -> Option<AudioDevice> {
        platform::get_backend()
            .and_then(|b| b.get_default_system_device())
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

        // Resolve the effective recording mode: use the live override when set,
        // otherwise fall back to the baked-in EngineConfig value.
        let effective_recording_mode = self
            .recording_mode_override
            .lock()
            .ok()
            .and_then(|g| *g)
            .unwrap_or(self.config.recording_mode);

        // When echo cancellation is requested, source2 must be a system audio
        // device (loopback/render endpoint).  AEC requires a render reference:
        // if source2 is a microphone/input device the render buffer is never
        // filled and AEC will suppress the primary mic signal entirely.
        if effective_recording_mode == RecordingMode::EchoCancel {
            if let Some(ref id) = source2_id {
                let system_ids: std::collections::HashSet<String> = backend
                    .list_system_devices()
                    .into_iter()
                    .map(|d| d.id)
                    .collect();
                if !system_ids.contains(id) {
                    return Err(format!(
                        "Echo cancellation requires source2 to be a system audio (loopback) \
                         device, but '{}' is an input device. Select a system audio device as \
                         the secondary source when echo cancellation is enabled.",
                        id
                    ));
                }
            } else {
                return Err(
                    "Echo cancellation requires a secondary system audio (loopback) source. \
                     Provide a system audio device as source2."
                        .to_string(),
                );
            }
        }

        // Consolidate AEC into recording_mode — EchoCancel implies AEC enabled
        backend.set_aec_enabled(effective_recording_mode == RecordingMode::EchoCancel);
        backend.set_recording_mode(effective_recording_mode);
        backend.start_capture_sources(source1_id.clone(), source2_id.clone())?;

        let sample_rate = backend.sample_rate();
        {
            let mut ts = self.transcribe_state.lock().unwrap();
            ts.init_for_capture(sample_rate, 2);
            ts.is_active = self.transcription_enabled.load(Ordering::SeqCst);
        }

        // Create the audio injection channel for file playback.
        // Bounded to 2 frames (~20ms) so the feeder stays tightly paced with
        // the audio loop and visualization events track the audio element closely.
        let (inject_tx, inject_rx) = std::sync::mpsc::sync_channel::<AudioData>(2);
        *self.playback_tx.lock().unwrap() = Some(inject_tx);

        self.start_audio_loop(sample_rate, inject_rx);

        let _ = self.sender.send(EngineEvent::CaptureStateChanged {
            capturing: true,
            error: None,
        });

        Ok(())
    }

    /// Spawn the audio processing loop thread.
    ///
    /// This is factored out so it can be started both from `start_capture`
    /// (with a hardware backend providing audio) and from `play_file`
    /// (injection-only, no hardware backend required).
    fn start_audio_loop(
        &self,
        sample_rate: u32,
        inject_rx: std::sync::mpsc::Receiver<AudioData>,
    ) {
        let loop_active = self.audio_loop_active.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        let sender = self.sender.clone();
        let transcribe_state = self.transcribe_state.clone();
        let transcription_enabled = self.transcription_enabled.clone();
        let vad_enabled = self.vad_enabled;
        let visualization_enabled = self.visualization_enabled;
        let audio_streaming_enabled = self.audio_streaming_enabled;
        let raw_audio_streaming_enabled = self.raw_audio_streaming_enabled;
        let recording_active = self.recording_active.clone();
        let playback_active_loop = self.playback_active.clone();
        let ptt_mode = self.ptt_mode.clone();
        let word_break_segmentation_enabled = self.config.word_break_segmentation_enabled;
        let mic_gain_db_atomic = self.mic_gain_db.clone();
        let agc_config_shared = self.agc_config.clone();
        let render_tx_shared = self.render_tx.clone();

        loop_active.store(true, Ordering::SeqCst);

        thread::spawn(move || {
            info!("[AudioLoop] Starting audio processing loop");

            let mut speech_detector = processor::SpeechDetector::new(sample_rate);
            let mut viz_processor = processor::VisualizationProcessor::new(sample_rate, 256);
            let mut pending_state_change = processor::SpeechStateChange::None;
            let mut pending_word_break: Option<processor::WordBreakEvent> = None;

            // Initialise AGC processor from the current config snapshot.
            let initial_agc_config = agc_config_shared
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            let mut agc_processor = processor::AgcProcessor::new(initial_agc_config);

            // Cumulative sample counter for audio streaming events.
            let mut audio_stream_sample_offset: u64 = 0;

            loop {
                if !loop_active.load(Ordering::SeqCst) || shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }

                // Prefer live hardware audio; fall back to injected playback audio.
                let audio_data = platform::get_backend()
                    .and_then(|b| b.try_recv())
                    .or_else(|| inject_rx.try_recv().ok());

                if let Some(data) = audio_data {
                    // Mix down to mono.  This copy is kept unmodified as the
                    // "raw" mono snapshot used for the raw WAV accumulation buffer.
                    let raw_mono_samples = audio::convert_to_mono(&data.samples, data.channels as usize);

                    // --- Raw audio streaming (pre-processing) ---
                    if raw_audio_streaming_enabled {
                        let _ = sender.send(EngineEvent::RawAudioData(StreamingAudioData {
                            samples: raw_mono_samples.clone(),
                            sample_rate,
                            sample_offset: audio_stream_sample_offset,
                        }));
                    }

                    // `processed_samples` starts as a copy of `raw_mono_samples` and is
                    // then mutated in-place by every processing stage.  It is kept
                    // entirely separate from `raw_mono_samples` so there is no risk of
                    // accidentally writing the wrong data to the wrong WAV file.
                    let mut processed_samples = raw_mono_samples.clone();

                    // --- Software mic gain ---
                    let gain_db = f32::from_bits(mic_gain_db_atomic.load(Ordering::Relaxed));
                    if gain_db != 0.0 {
                        let linear = 10f32.powf(gain_db / 20.0);
                        for s in processed_samples.iter_mut() {
                            *s = (*s * linear).clamp(-1.0, 1.0);
                        }
                    }

                    // --- Automatic Gain Control ---
                    // try_lock: if the UI thread is mid-update, skip this chunk's hot-swap
                    // (the previous config remains active — safe, transient).
                    if let Ok(new_cfg) = agc_config_shared.try_lock() {
                        agc_processor.update_config(new_cfg.clone());
                    }
                    if let Some(agc_gain_db) = agc_processor.process(&mut processed_samples, sample_rate) {
                        let _ = sender.send(EngineEvent::AgcGainChanged(agc_gain_db));
                    }

                    // --- VAD ---
                    if vad_enabled {
                        speech_detector.process(&processed_samples);
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
                        if let Some(viz) = viz_processor.process(&processed_samples) {
                            let _ = sender.send(EngineEvent::VisualizationData(viz));
                        }
                    }

                    // --- Processed audio streaming (post-processing) ---
                    if audio_streaming_enabled {
                        let _ = sender.send(EngineEvent::AudioData(StreamingAudioData {
                            samples: processed_samples.clone(),
                            sample_rate,
                            sample_offset: audio_stream_sample_offset,
                        }));
                    }

                    // --- Render output (during file playback) ---
                    if playback_active_loop.load(Ordering::Relaxed) {
                        if let Ok(guard) = render_tx_shared.try_lock() {
                            if let Some(ref tx) = *guard {
                                // try_send: drop the chunk if the render thread
                                // is behind rather than blocking the audio loop.
                                let _ = tx.try_send(processed_samples.clone());
                            }
                        }
                    }

                    // --- Audio stream sample offset ---
                    if audio_streaming_enabled || raw_audio_streaming_enabled {
                        audio_stream_sample_offset += raw_mono_samples.len() as u64;
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

                    let is_recording = recording_active.load(Ordering::SeqCst);
                    let is_ptt = ptt_mode.load(Ordering::SeqCst);

                    if let Ok(mut ts) = transcribe_state.try_lock() {
                        if is_recording {
                            // Pre-gain mono mix → raw WAV accumulation.
                            ts.write_manual_buffer(&raw_mono_samples);
                            // Gain/AGC-processed mono → processed WAV + transcription.
                            ts.write_processed_buffer(&processed_samples);
                        } else {
                            // Not recording: let the global transcription_enabled flag
                            // control whether VAD mode is active.
                            ts.is_active = transcription_enabled.load(Ordering::SeqCst);
                        }

                        // VAD-driven segmentation runs when:
                        //   • Not in PTT mode (auto-transcription on), AND
                        //   • Transcription is active, AND
                        //   • VAD is enabled.
                        // This applies both during a recording session and during
                        // idle capture, so live segments are transcribed in real time.
                        if !is_ptt && ts.is_active && vad_enabled {
                            if let processor::SpeechStateChange::Started { lookback_samples } =
                                &pending_state_change
                            {
                                ts.on_speech_started(*lookback_samples);
                                let _ = sender.send(EngineEvent::SpeechStarted);
                            }

                            // Feed the ring buffer with processed mono samples so that
                            // VAD-driven segment extraction and transcription use the
                            // gain/AGC-adjusted signal.
                            ts.process_samples(&processed_samples);

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

    /// Set the microphone input gain in dB.
    ///
    /// Takes effect immediately on the next audio buffer processed in the
    /// capture loop. Also calls [`AudioBackend::set_gain`] so platform
    /// backends may apply hardware gain if they choose to implement it.
    ///
    /// A value of `0.0` means no change (linear multiplier of 1.0).
    /// Recommended range: -20.0 to +20.0 dB.
    pub fn set_mic_gain(&self, db: f32) {
        self.mic_gain_db.store(db.to_bits(), Ordering::Relaxed);
        if let Some(backend) = platform::get_backend() {
            backend.set_gain(db);
        }
    }

    /// Get the current microphone input gain in dB.
    pub fn mic_gain_db(&self) -> f32 {
        f32::from_bits(self.mic_gain_db.load(Ordering::Relaxed))
    }

    /// Replace the active AGC configuration.
    ///
    /// Takes effect on the next audio chunk processed in the capture loop
    /// (at most one chunk duration, typically ~10–40 ms).
    pub fn set_agc_config(&self, config: AgcConfig) {
        if let Ok(mut guard) = self.agc_config.lock() {
            *guard = config;
        }
    }

    /// Get the current AGC configuration.
    pub fn agc_config(&self) -> AgcConfig {
        self.agc_config.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Get the current engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Set the recording mode for the next `start_capture` call.
    ///
    /// This overrides the `recording_mode` baked into `EngineConfig` at build
    /// time, allowing the mode to be updated at runtime without requiring a
    /// mutable reference to the engine.  The override is applied immediately
    /// on the next `start_capture` call.
    ///
    /// This is the preferred path for the FlowSTT app layer to switch between
    /// `Mixed` (mic-only) and `EchoCancel` (AEC active) modes based on whether
    /// a reference device (source2) is configured.
    pub fn set_recording_mode(&self, mode: RecordingMode) {
        if let Ok(mut guard) = self.recording_mode_override.lock() {
            *guard = Some(mode);
        }
    }

    /// Update the engine configuration.
    ///
    /// The new configuration takes effect on the next `start_capture` call,
    /// except for `mic_gain_db` and `agc` which are applied immediately.
    pub fn set_config(&mut self, config: EngineConfig) {
        let gain = config.mic_gain_db;
        let agc = config.agc.clone();
        self.config = config;
        self.set_mic_gain(gain);
        self.set_agc_config(agc);
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

    /// Set PTT (push-to-talk / manual) mode.
    ///
    /// When `enabled` is `true`, VAD-driven automatic segmentation is
    /// suppressed and the user must call [`start_recording`](Self::start_recording) /
    /// [`stop_recording`](Self::stop_recording) to capture audio.
    ///
    /// When `enabled` is `false` (auto-transcription mode), VAD drives
    /// segmentation automatically during live capture or file playback.
    pub fn set_ptt_mode(&self, enabled: bool) {
        info!("[Engine] PTT mode: {}", enabled);
        self.ptt_mode.store(enabled, Ordering::SeqCst);
    }

    /// Whether PTT (manual recording) mode is currently enabled.
    pub fn is_ptt_mode(&self) -> bool {
        self.ptt_mode.load(Ordering::SeqCst)
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

// =============================================================================
// Unit tests
// =============================================================================

#[cfg(test)]
mod tests {
    /// Task 3.3: 0 dB gain produces a linear multiplier of 1.0 (no change).
    #[test]
    fn gain_zero_db_is_unity() {
        let gain_db: f32 = 0.0;
        let linear = 10f32.powf(gain_db / 20.0);
        assert!((linear - 1.0f32).abs() < 1e-6, "0 dB should give linear=1.0, got {}", linear);

        let sample: f32 = 0.5;
        let result = (sample * linear).clamp(-1.0, 1.0);
        assert!((result - 0.5).abs() < 1e-6);
    }

    /// Task 3.4: Large positive gain clamps output samples to [-1.0, 1.0].
    #[test]
    fn gain_clamps_to_valid_range() {
        let gain_db: f32 = 40.0; // 100× linear — will saturate
        let linear = 10f32.powf(gain_db / 20.0);

        let samples: Vec<f32> = vec![0.1, -0.1, 0.9, -0.9, 1.0, -1.0];
        let clamped: Vec<f32> = samples.iter().map(|s| (*s * linear).clamp(-1.0, 1.0)).collect();

        for s in &clamped {
            assert!(*s >= -1.0 && *s <= 1.0, "sample {} out of range", s);
        }
        // A large input should be clamped to exactly ±1.0
        assert_eq!(clamped[2], 1.0);
        assert_eq!(clamped[3], -1.0);
    }

    /// Audio stream sample offset tracks cumulative chunk sizes correctly.
    #[test]
    fn audio_stream_sample_offset_accumulation() {
        // Simulate the offset counter logic from the audio loop
        let mut offset: u64 = 0;
        let chunk_sizes: Vec<usize> = vec![480, 480, 960, 240, 480];

        let mut offsets = Vec::new();
        for chunk_size in &chunk_sizes {
            offsets.push(offset);
            offset += *chunk_size as u64;
        }

        assert_eq!(offsets, vec![0, 480, 960, 1920, 2160]);
        assert_eq!(offset, 2640); // total samples after all chunks
    }

    /// Verify the sample_offset → timestamp conversion formula.
    #[test]
    fn audio_stream_offset_to_timestamp() {
        let sample_rate: u32 = 48000;

        // 0 offset = 0.0 seconds
        assert_eq!(0u64 as f64 / sample_rate as f64, 0.0);

        // 48000 samples = 1.0 second
        assert!((48000u64 as f64 / sample_rate as f64 - 1.0).abs() < 1e-9);

        // 480000 samples = 10.0 seconds
        assert!((480000u64 as f64 / sample_rate as f64 - 10.0).abs() < 1e-9);

        // 144000000 samples = 3000.0 seconds (50 minutes)
        assert!((144_000_000u64 as f64 / sample_rate as f64 - 3000.0).abs() < 1e-9);
    }

    /// Both streams (processed + raw) share the same offset sequence when
    /// derived from the same chunk.
    #[test]
    fn audio_stream_shared_offset_sequence() {
        use crate::StreamingAudioData;

        let mut offset: u64 = 0;
        let chunk = vec![0.1f32, -0.2, 0.3];

        // Simulate emitting both events from the same chunk
        let raw_event = StreamingAudioData {
            samples: chunk.clone(),
            sample_rate: 48000,
            sample_offset: offset,
        };
        let processed_event = StreamingAudioData {
            samples: chunk.iter().map(|s| s * 2.0).collect(), // different data
            sample_rate: 48000,
            sample_offset: offset,
        };

        // Both share the same offset
        assert_eq!(raw_event.sample_offset, processed_event.sample_offset);

        // After the chunk, offset advances by chunk length
        offset += chunk.len() as u64;
        assert_eq!(offset, 3);
    }

    /// The sample_offset counter is only incremented when at least one
    /// streaming flag is enabled (matches the conditional in the audio loop).
    #[test]
    fn audio_stream_offset_gated_by_flags() {
        // Simulate the audio loop's conditional offset increment
        let audio_streaming_enabled = false;
        let raw_audio_streaming_enabled = false;
        let mut offset: u64 = 0;

        // With both disabled, offset should not advance
        if audio_streaming_enabled || raw_audio_streaming_enabled {
            offset += 480;
        }
        assert_eq!(offset, 0);

        // With one enabled, offset should advance
        let audio_streaming_enabled = true;
        if audio_streaming_enabled || raw_audio_streaming_enabled {
            offset += 480;
        }
        assert_eq!(offset, 480);
    }
}
