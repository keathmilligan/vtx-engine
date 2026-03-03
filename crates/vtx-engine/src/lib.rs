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
//! use vtx_engine::{AudioEngine, EngineConfig, EventHandler};
//! use vtx_common::EngineEvent;
//!
//! struct MyHandler;
//! impl EventHandler for MyHandler {
//!     fn on_event(&self, event: EngineEvent) {
//!         println!("Event: {:?}", event);
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = EngineConfig::default();
//!     let engine = AudioEngine::new(config, MyHandler).await.unwrap();
//!     let devices = engine.list_input_devices();
//!     if let Some(device) = devices.first() {
//!         engine.start_capture(Some(device.id.clone()), None).await.unwrap();
//!     }
//! }
//! ```

mod audio;
pub mod platform;
pub mod processor;
pub mod transcription;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tracing::{debug, error, info};
use vtx_common::*;

// Re-export the common types
pub use vtx_common;

// =============================================================================
// Public API
// =============================================================================

/// Callback trait for receiving engine events.
///
/// Implement this trait to receive real-time events from the engine,
/// such as visualization data, transcription results, and state changes.
pub trait EventHandler: Send + Sync + 'static {
    /// Called when the engine produces an event.
    fn on_event(&self, event: EngineEvent);
}

/// Configuration for the audio engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Whether to enable echo cancellation (requires a secondary source).
    pub aec_enabled: bool,
    /// Recording mode (mixed or echo-cancel).
    pub recording_mode: RecordingMode,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            aec_enabled: false,
            recording_mode: RecordingMode::Mixed,
        }
    }
}

/// The main audio engine. Manages audio capture, speech detection,
/// visualization, and transcription.
pub struct AudioEngine {
    /// Engine configuration
    config: EngineConfig,
    /// Event handler callback
    event_handler: Arc<dyn EventHandler>,
    /// Transcription queue
    transcription_queue: Arc<transcription::TranscriptionQueue>,
    /// Transcribe state (ring buffer + segment management)
    transcribe_state: Arc<std::sync::Mutex<transcription::TranscribeState>>,
    /// Whether the audio loop is running
    audio_loop_active: Arc<AtomicBool>,
    /// Whether transcription is enabled (independent of capture state)
    transcription_enabled: Arc<AtomicBool>,
    /// Global shutdown flag
    shutdown_flag: Arc<AtomicBool>,
}

impl AudioEngine {
    /// Create a new audio engine with the given configuration and event handler.
    ///
    /// This initializes the platform audio backend and transcription system,
    /// but does not start capturing audio.
    pub async fn new(
        config: EngineConfig,
        handler: impl EventHandler,
    ) -> Result<Self, String> {
        let event_handler: Arc<dyn EventHandler> = Arc::new(handler);
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        // Initialize platform audio backend
        info!("Initializing audio backend...");
        platform::init_audio_backend()?;

        // Initialize transcription system
        let callback = EngineTranscriptionCallback {
            event_handler: event_handler.clone(),
        };
        let transcription_queue = Arc::new(transcription::TranscriptionQueue::new());
        transcription_queue.set_callback(Arc::new(callback));

        // Start the worker thread that pulls from the queue and runs Whisper.
        // This must happen once at engine creation; without it no segments are ever processed.
        let model_path = transcription::Transcriber::new().get_model_path().clone();
        transcription_queue.start_worker(model_path);

        let transcribe_state = Arc::new(std::sync::Mutex::new(
            transcription::TranscribeState::new(transcription_queue.clone()),
        ));

        Ok(Self {
            config,
            event_handler,
            transcription_queue,
            transcribe_state,
            audio_loop_active: Arc::new(AtomicBool::new(false)),
            transcription_enabled: Arc::new(AtomicBool::new(true)),
            shutdown_flag,
        })
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

        backend.set_aec_enabled(self.config.aec_enabled);
        backend.set_recording_mode(self.config.recording_mode);
        backend.start_capture_sources(source1_id.clone(), source2_id.clone())?;

        let sample_rate = backend.sample_rate();
        // WASAPI always outputs stereo (2 channels) after its internal mono->stereo
        // conversion.  Other backends follow the same convention.  We initialise
        // TranscribeState here so duration math is correct from the very first
        // callback without needing to touch the mutex inside the hot audio loop.
        {
            let mut ts = self.transcribe_state.lock().unwrap();
            ts.init_for_capture(sample_rate, 2);
            ts.is_active = self.transcription_enabled.load(Ordering::SeqCst);
        }

        // Start audio processing loop
        let loop_active = self.audio_loop_active.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        let event_handler = self.event_handler.clone();
        let transcribe_state = self.transcribe_state.clone();
        let transcription_enabled = self.transcription_enabled.clone();

        loop_active.store(true, Ordering::SeqCst);

        thread::spawn(move || {
            info!("[AudioLoop] Starting audio processing loop");

            let mut speech_detector = processor::SpeechDetector::new(sample_rate);
            let mut viz_processor = processor::VisualizationProcessor::new(sample_rate, 256);
            // Pending speech events survive a try_lock miss so they are never dropped.
            let mut pending_state_change = processor::SpeechStateChange::None;
            let mut pending_word_break: Option<processor::WordBreakEvent> = None;

            loop {
                if !loop_active.load(Ordering::SeqCst) || shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }

                let audio_data = platform::get_backend().and_then(|b| b.try_recv());

                if let Some(data) = audio_data {
                    let mono_samples = audio::convert_to_mono(&data.samples, data.channels as usize);

                    // Speech detection
                    speech_detector.process(&mono_samples);
                    let speech_metrics = speech_detector.get_metrics();

                    // Visualization
                    viz_processor.set_speech_metrics(speech_metrics.clone());
                    let viz_data = viz_processor.process(&mono_samples);

                    // Emit visualization event
                    if let Some(viz) = viz_data {
                        event_handler.on_event(EngineEvent::VisualizationData(viz));
                    }

                    // Merge any new speech events from this batch with any that
                    // survived a previous try_lock miss (so events are never lost).
                    let new_state = speech_detector.take_state_change();
                    let new_wb = speech_detector.take_word_break_event();

                    // A new terminal event (Started / Ended) always wins over a
                    // previously pending None; a None does not overwrite a pending event.
                    match new_state {
                        processor::SpeechStateChange::None => {}
                        other => pending_state_change = other,
                    }
                    if new_wb.is_some() {
                        pending_word_break = new_wb;
                    }

                    // Try to acquire the transcription state lock without blocking.
                    // The mutex is only held briefly by start_capture/stop_capture;
                    // if we miss a cycle we retry next iteration with events still pending.
                    if let Ok(mut ts) = transcribe_state.try_lock() {
                        ts.is_active = transcription_enabled.load(Ordering::SeqCst);

                        if ts.is_active {
                            // Apply speech-start BEFORE writing so index_from_lookback()
                            // references the write_pos from the previous batch.
                            if let processor::SpeechStateChange::Started { lookback_samples } =
                                &pending_state_change
                            {
                                ts.on_speech_started(*lookback_samples);
                                event_handler.on_event(EngineEvent::SpeechStarted);
                            }

                            ts.process_samples(&data.samples);

                            if let processor::SpeechStateChange::Ended { duration_ms } =
                                pending_state_change
                            {
                                ts.on_speech_ended();
                                event_handler
                                    .on_event(EngineEvent::SpeechEnded { duration_ms });
                            }

                            if let Some(wb) = pending_word_break.take() {
                                ts.on_word_break(wb.offset_ms, wb.gap_duration_ms);
                            }
                        }

                        // Events consumed — reset pending state.
                        pending_state_change = processor::SpeechStateChange::None;
                        pending_word_break = None;
                    }
                    // If try_lock failed, pending_state_change / pending_word_break
                    // are retained and will be applied on the next successful lock.
                } else {
                    thread::sleep(Duration::from_millis(1));
                }
            }

            info!("[AudioLoop] Audio processing loop stopped");
        });

        // Emit capture state change
        self.event_handler.on_event(EngineEvent::CaptureStateChanged {
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

        self.event_handler.on_event(EngineEvent::CaptureStateChanged {
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
        let event_handler = self.event_handler.clone();
        let model_path = transcription::Transcriber::new().get_model_path().clone();

        let eh = event_handler.clone();
        let result = transcription::download_model(&model_path, move |percent| {
            eh.on_event(EngineEvent::ModelDownloadProgress { percent });
        })
        .await;

        self.event_handler.on_event(EngineEvent::ModelDownloadComplete {
            success: result.is_ok(),
        });

        result
    }

    /// Enable or disable echo cancellation. Takes effect on the next `start_capture` call.
    pub fn set_aec_enabled(&self, enabled: bool) {
        info!("[Engine] AEC enabled: {}", enabled);
        // Update the config value — requires interior mutability via a cell
        // We use the backend directly so it takes effect immediately for the next capture.
        if let Some(backend) = platform::get_backend() {
            backend.set_aec_enabled(enabled);
        }
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

    /// Enable or disable PTT (push-to-talk) mode for transcription.
    ///
    /// When PTT mode is enabled, automatic speech-based segmentation is disabled.
    /// Audio is accumulated in the ring buffer and only submitted when
    /// `finalize_segment` is called (typically when recording is stopped).
    ///
    /// When PTT mode is disabled (the default), segments are automatically
    /// submitted based on speech detection events.
    pub fn set_ptt_mode(&self, enabled: bool) {
        info!("[Engine] PTT mode: {}", enabled);
        if let Ok(mut ts) = self.transcribe_state.lock() {
            ts.set_ptt_mode(enabled);
        }
    }

    /// Whether PTT mode is currently enabled.
    pub fn is_ptt_mode(&self) -> bool {
        self.transcribe_state
            .lock()
            .map(|ts| ts.ptt_mode)
            .unwrap_or(false)
    }

    /// Finalize and submit the current recording session for transcription.
    ///
    /// In PTT mode (auto-transcription disabled), this submits all audio accumulated
    /// in the ring buffer since recording started. Should be called before `stop_capture`.
    ///
    /// In automatic mode this is effectively a no-op since segments are submitted
    /// continuously via VAD events.
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
            cuda_available: system_info.contains("CUDA"),
            metal_available: system_info.contains("METAL = 1"),
            system_info,
        })
    }

    /// Get current engine status.
    pub fn get_status(&self) -> EngineStatus {
        EngineStatus {
            capturing: self.audio_loop_active.load(Ordering::SeqCst),
            in_speech: false, // TODO: track from speech detector
            queue_depth: self.transcription_queue.queue_depth(),
            error: None,
            source1_id: None,
            source2_id: None,
        }
    }

    /// Transcribe a WAV file and return the result.
    pub async fn transcribe_file(&self, path: String) -> Result<TranscriptionResult, String> {
        let _event_handler = self.event_handler.clone();

        let result = tokio::task::spawn_blocking(move || {
            // Read WAV file
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

            // Convert to mono if needed
            let mono = if spec.channels > 1 {
                audio::convert_to_mono(&samples, spec.channels as usize)
            } else {
                samples
            };

            // Resample to 16kHz
            let resampled = audio::resample_to_16khz(&mono, spec.sample_rate)?;

            // Transcribe
            let mut transcriber = transcription::Transcriber::new();
            let text = transcriber.transcribe(&resampled)?;

            let duration_ms = Some((mono.len() as u64 * 1000) / spec.sample_rate as u64);

            Ok(TranscriptionResult {
                text,
                duration_ms,
                audio_path: Some(path),
            })
        })
        .await
        .map_err(|e| format!("Transcription task failed: {}", e))?;

        if let Ok(ref result) = result {
            self.event_handler.on_event(EngineEvent::TranscriptionComplete(result.clone()));
        }

        result
    }

    /// Start a lightweight test capture on a device to report audio levels.
    pub fn start_test_capture(&self, device_id: String) -> Result<(), String> {
        let event_handler = self.event_handler.clone();

        let backend = platform::get_backend()
            .ok_or_else(|| "Audio backend not initialized".to_string())?;

        let sample_rate = backend.sample_rate();

        // Start capture on the test device
        backend.start_capture_sources(Some(device_id.clone()), None)?;

        // Spawn a thread that reads audio and reports levels
        let shutdown = self.shutdown_flag.clone();

        thread::spawn(move || {
            let mut sample_buffer: Vec<f32> = Vec::new();
            let samples_per_report = (sample_rate as usize) / 10; // ~100ms

            loop {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }

                if let Some(data) = platform::get_backend().and_then(|b| b.try_recv()) {
                    let mono = audio::convert_to_mono(&data.samples, data.channels as usize);
                    sample_buffer.extend_from_slice(&mono);

                    if sample_buffer.len() >= samples_per_report {
                        // Calculate RMS in dB
                        let sum_sq: f32 = sample_buffer.iter().map(|s| s * s).sum();
                        let rms = (sum_sq / sample_buffer.len() as f32).sqrt();
                        let db = if rms > 0.0 {
                            20.0 * rms.log10()
                        } else {
                            -100.0
                        };

                        event_handler.on_event(EngineEvent::AudioLevelUpdate {
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

/// Internal transcription callback that forwards results to the event handler.
struct EngineTranscriptionCallback {
    event_handler: Arc<dyn EventHandler>,
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

        self.event_handler.on_event(EngineEvent::TranscriptionComplete(
            TranscriptionResult {
                text: trimmed.to_string(),
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
