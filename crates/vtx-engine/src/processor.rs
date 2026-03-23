//! Audio processing utilities for speech detection and visualization.
//!
//! This module contains the SpeechDetector and VisualizationProcessor which
//! analyze audio streams for speech activity and generate visualization data.

use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
use std::sync::Arc;

use crate::AgcConfig;

/// Speech state change events detected by the speech detector
#[derive(Clone, Debug)]
pub enum SpeechStateChange {
    /// No change in speech state
    None,
    /// Speech started with lookback sample count
    Started { lookback_samples: usize },
    /// Speech ended with duration in milliseconds
    Ended { duration_ms: u64 },
}

/// Word break event detected during speech
#[derive(Clone, Debug)]
pub struct WordBreakEvent {
    /// Offset from speech start in milliseconds
    pub offset_ms: u32,
    /// Duration of the gap in milliseconds
    pub gap_duration_ms: u32,
}

/// Speech detection metrics for visualization
#[derive(Clone, Debug, Serialize)]
pub struct SpeechMetrics {
    /// RMS amplitude in decibels
    pub amplitude_db: f32,
    /// Zero-crossing rate (0.0 to 0.5)
    pub zcr: f32,
    /// Estimated spectral centroid in Hz
    pub centroid_hz: f32,
    /// Whether speech is currently detected
    pub is_speaking: bool,
    /// Whether voiced speech onset is pending
    pub is_voiced_pending: bool,
    /// Whether whisper speech onset is pending
    pub is_whisper_pending: bool,
    /// Whether current frame is classified as transient
    pub is_transient: bool,
    /// Whether this is lookback-determined speech (retroactively identified)
    pub is_lookback_speech: bool,
    /// Lookback offset in milliseconds (when speech was just confirmed)
    pub lookback_offset_ms: Option<u32>,
    /// Whether a word break (inter-word gap) is currently detected
    pub is_word_break: bool,
}

/// Event payload for speech detection events
#[derive(Clone, Debug, Serialize)]
pub struct SpeechEventPayload {
    /// Duration in milliseconds (for speech-ended: how long the speech lasted)
    pub duration_ms: Option<u64>,
    /// Lookback offset in milliseconds (how far back the true start was found)
    pub lookback_offset_ms: Option<u32>,
}

/// Event payload for word break detection events
#[derive(Clone, Debug, Serialize)]
pub struct WordBreakPayload {
    /// Timestamp offset in milliseconds from speech start
    pub offset_ms: u32,
    /// Duration of the detected gap in milliseconds
    pub gap_duration_ms: u32,
}

/// Callback trait for receiving speech events
pub trait SpeechEventCallback: Send {
    /// Called when speech starts
    fn on_speech_started(&self, payload: SpeechEventPayload);
    /// Called when speech ends
    fn on_speech_ended(&self, payload: SpeechEventPayload);
    /// Called when a word break is detected
    fn on_word_break(&self, payload: WordBreakPayload);
}

/// Configuration for a speech detection mode (voiced or whisper)
#[derive(Clone)]
#[allow(dead_code)]
struct SpeechModeConfig {
    /// Minimum amplitude threshold in dB
    threshold_db: f32,
    /// ZCR range (min, max) - normalized as crossings per sample
    zcr_range: (f32, f32),
    /// Spectral centroid range in Hz (min, max)
    centroid_range: (f32, f32),
    /// Onset time in samples before confirming speech
    onset_samples: u32,
}

/// Speech detector that detects when speech starts and ends.
///
/// Currently uses simple amplitude-based silence detection.
/// ZCR, spectral centroid, transient rejection, and word break detection
/// are computed for visualization metrics but do not affect speech state.
///
/// Includes lookback functionality to capture the true start of speech by maintaining
/// a ring buffer of recent audio samples and analyzing them retroactively.
#[allow(dead_code)]
pub struct SpeechDetector {
    /// Sample rate for time/frequency calculations
    sample_rate: u32,
    /// Voiced speech detection configuration
    voiced_config: SpeechModeConfig,
    /// Whisper speech detection configuration  
    whisper_config: SpeechModeConfig,
    /// Transient rejection: ZCR threshold (reject if above)
    transient_zcr_threshold: f32,
    /// Transient rejection: centroid threshold in Hz (reject if above, combined with ZCR)
    transient_centroid_threshold: f32,
    /// Hold time in samples before emitting speech-ended event
    hold_samples: u32,
    /// Current speech state (true = speaking, false = silent)
    is_speaking: bool,
    /// Whether we're in "pending voiced" state
    is_pending_voiced: bool,
    /// Whether we're in "pending whisper" state
    is_pending_whisper: bool,
    /// Counter for voiced onset time
    voiced_onset_count: u32,
    /// Counter for whisper onset time
    whisper_onset_count: u32,
    /// Counter for hold time during silence
    silence_sample_count: u32,
    /// Counter for speech duration (from confirmed start)
    speech_sample_count: u64,
    /// Grace samples allowed during onset (brief dips don't reset counters)
    onset_grace_samples: u32,
    /// Current grace counter for voiced onset
    voiced_grace_count: u32,
    /// Current grace counter for whisper onset
    whisper_grace_count: u32,
    /// Whether we've initialized (first sample processed)
    initialized: bool,
    /// Last computed amplitude in dB (for metrics)
    last_amplitude_db: f32,
    /// Last computed ZCR (for metrics)
    last_zcr: f32,
    /// Last computed spectral centroid in Hz (for metrics)
    last_centroid_hz: f32,
    /// Whether last frame was classified as transient (for metrics)
    last_is_transient: bool,

    // Lookback ring buffer fields
    /// Ring buffer for recent audio samples (for lookback analysis)
    lookback_buffer: Vec<f32>,
    /// Current write position in the ring buffer
    lookback_write_index: usize,
    /// Capacity of the lookback buffer in samples
    lookback_capacity: usize,
    /// Whether the lookback buffer has been filled at least once
    lookback_filled: bool,
    /// Lookback threshold in dB (more sensitive than detection threshold)
    lookback_threshold_db: f32,
    /// Last lookback offset in milliseconds (for metrics, set when speech confirmed)
    last_lookback_offset_ms: Option<u32>,
    /// Last state change detected during process() - for transcribe mode integration
    last_state_change: SpeechStateChange,

    // Word break detection fields
    /// Word break threshold ratio (amplitude must drop below this fraction of recent average)
    word_break_threshold_ratio: f32,
    /// Minimum gap duration in samples for word break (15ms)
    min_word_break_samples: u32,
    /// Maximum gap duration in samples for word break (200ms)
    max_word_break_samples: u32,
    /// Window size in samples for tracking recent speech amplitude (100ms)
    recent_speech_window_samples: u32,
    /// Running sum of recent speech amplitude (linear, not dB)
    recent_speech_amplitude_sum: f32,
    /// Count of samples in recent speech amplitude window
    recent_speech_amplitude_count: u32,
    /// Whether we're currently in a word break gap
    in_word_break: bool,
    /// Sample count of current word break gap
    word_break_sample_count: u32,
    /// Sample count at start of current word break (for offset calculation)
    word_break_start_speech_samples: u64,
    /// Whether last frame was a word break (for metrics)
    last_is_word_break: bool,
    /// Last word break event detected (for transcribe mode integration)
    last_word_break_event: Option<WordBreakEvent>,

    /// Callback for speech events
    callback: Option<Arc<dyn SpeechEventCallback>>,
}

impl SpeechDetector {
    /// Create a new speech detector with specified sample rate.
    /// Uses default dual-mode configuration optimized for speech detection.
    pub fn new(sample_rate: u32) -> Self {
        Self::with_defaults(sample_rate)
    }

    /// Create a speech detector with default dual-mode configuration.
    ///
    /// Default parameters:
    /// - Voiced mode: -42dB threshold, ZCR 0.01-0.30, centroid 200-5500Hz, 80ms onset
    /// - Whisper mode: -52dB threshold, ZCR 0.02-0.45, centroid 300-7000Hz, 120ms onset
    /// - Transient rejection: ZCR > 0.45 AND centroid > 6500Hz
    /// - Hold time: 200ms (reduced from 300ms to detect sentence-end pauses from fast talkers)
    /// - Onset grace period: 30ms (brief dips in features don't reset onset counters)
    /// - Lookback buffer: 200ms (covers max onset time + margin)
    /// - Lookback threshold: -55dB (more sensitive to catch speech starts)
    /// - Word break min gap: 40ms (reduced from 80ms to catch fast-talker inter-word gaps)
    /// - Word break threshold ratio: 0.25 (25% of rolling average, tuned for dense speech)
    ///
    /// Word break detection is energy-based and runs independently of speech candidate
    /// classification.  During active speech, if the RMS amplitude drops below the
    /// word break threshold ratio of the rolling average, a word break is tracked
    /// regardless of whether the frame's spectral features match a speech mode.
    pub fn with_defaults(sample_rate: u32) -> Self {
        let hold_samples = (sample_rate as u64 * 200 / 1000) as u32;
        // 200ms lookback buffer
        let lookback_capacity = (sample_rate as u64 * 200 / 1000) as usize;

        Self {
            sample_rate,
            voiced_config: SpeechModeConfig {
                threshold_db: -42.0,
                zcr_range: (0.01, 0.30),
                centroid_range: (200.0, 5500.0),
                onset_samples: (sample_rate as u64 * 80 / 1000) as u32,
            },
            whisper_config: SpeechModeConfig {
                threshold_db: -52.0,
                zcr_range: (0.02, 0.45),
                centroid_range: (300.0, 7000.0),
                onset_samples: (sample_rate as u64 * 120 / 1000) as u32,
            },
            transient_zcr_threshold: 0.45,
            transient_centroid_threshold: 6500.0,
            hold_samples,
            is_speaking: false,
            is_pending_voiced: false,
            is_pending_whisper: false,
            voiced_onset_count: 0,
            whisper_onset_count: 0,
            silence_sample_count: 0,
            speech_sample_count: 0,
            onset_grace_samples: (sample_rate as u64 * 30 / 1000) as u32,
            voiced_grace_count: 0,
            whisper_grace_count: 0,
            initialized: false,
            last_amplitude_db: -100.0, // Use finite value instead of NEG_INFINITY (JSON serialization issue)
            last_zcr: 0.0,
            last_centroid_hz: 0.0,
            last_is_transient: false,
            // Lookback buffer initialization
            lookback_buffer: vec![0.0; lookback_capacity],
            lookback_write_index: 0,
            lookback_capacity,
            lookback_filled: false,
            lookback_threshold_db: -55.0,
            last_lookback_offset_ms: None,
            last_state_change: SpeechStateChange::None,

            // Word break detection initialization
            // Threshold ratio: amplitude must drop to this fraction of recent average
            // Using 0.25 (25%) to be sensitive enough to catch dips in dense fast speech
            word_break_threshold_ratio: 0.25,
            // Minimum gap duration: 40ms - catches fast-talker inter-word gaps (~40-70ms)
            // (was 80ms which missed all fast-talker gaps; was previously 15ms which was too aggressive)
            min_word_break_samples: (sample_rate as u64 * 40 / 1000) as u32,
            // Maximum gap duration: 250ms - longer gaps will trigger speech-end instead
            max_word_break_samples: (sample_rate as u64 * 250 / 1000) as u32,
            recent_speech_window_samples: (sample_rate as u64 * 100 / 1000) as u32,
            recent_speech_amplitude_sum: 0.0,
            recent_speech_amplitude_count: 0,
            in_word_break: false,
            word_break_sample_count: 0,
            word_break_start_speech_samples: 0,
            last_is_word_break: false,
            last_word_break_event: None,

            callback: None,
        }
    }

    /// Set the callback for speech events
    pub fn set_callback(&mut self, callback: Arc<dyn SpeechEventCallback>) {
        self.callback = Some(callback);
    }

    /// Calculate RMS amplitude of samples
    fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
        (sum_squares / samples.len() as f32).sqrt()
    }

    /// Convert linear amplitude to decibels
    fn amplitude_to_db(amplitude: f32) -> f32 {
        if amplitude <= 0.0 {
            return -100.0; // Use finite value instead of NEG_INFINITY (JSON serialization issue)
        }
        20.0 * amplitude.log10()
    }

    /// Calculate Zero-Crossing Rate (ZCR) of samples.
    fn calculate_zcr(samples: &[f32]) -> f32 {
        if samples.len() < 2 {
            return 0.0;
        }

        let mut crossings = 0u32;
        for i in 1..samples.len() {
            if (samples[i] >= 0.0) != (samples[i - 1] >= 0.0) {
                crossings += 1;
            }
        }

        crossings as f32 / (samples.len() - 1) as f32
    }

    /// Estimate spectral centroid using first-difference approximation.
    fn estimate_spectral_centroid(&self, samples: &[f32], amplitude_db: f32) -> f32 {
        const CENTROID_GATE_DB: f32 = -55.0;
        if samples.len() < 2 || amplitude_db < CENTROID_GATE_DB {
            return 0.0;
        }

        let mut diff_sum = 0.0f32;
        for i in 1..samples.len() {
            diff_sum += (samples[i] - samples[i - 1]).abs();
        }
        let mean_diff = diff_sum / (samples.len() - 1) as f32;

        let mean_abs: f32 = samples.iter().map(|s| s.abs()).sum::<f32>() / samples.len() as f32;

        if mean_abs < 1e-10 {
            return 0.0;
        }

        self.sample_rate as f32 * mean_diff / (2.0 * mean_abs)
    }

    /// Check if features indicate a transient sound
    #[allow(dead_code)]
    fn is_transient(&self, zcr: f32, centroid: f32) -> bool {
        zcr > self.transient_zcr_threshold && centroid > self.transient_centroid_threshold
    }

    /// Check if features match voiced speech mode
    #[allow(dead_code)]
    fn matches_voiced_mode(&self, db: f32, zcr: f32, centroid: f32) -> bool {
        db >= self.voiced_config.threshold_db
            && zcr >= self.voiced_config.zcr_range.0
            && zcr <= self.voiced_config.zcr_range.1
            && centroid >= self.voiced_config.centroid_range.0
            && centroid <= self.voiced_config.centroid_range.1
    }

    /// Check if features match whisper speech mode
    #[allow(dead_code)]
    fn matches_whisper_mode(&self, db: f32, zcr: f32, centroid: f32) -> bool {
        db >= self.whisper_config.threshold_db
            && zcr >= self.whisper_config.zcr_range.0
            && zcr <= self.whisper_config.zcr_range.1
            && centroid >= self.whisper_config.centroid_range.0
            && centroid <= self.whisper_config.centroid_range.1
    }

    /// Convert sample count to milliseconds
    fn samples_to_ms(&self, samples: u64) -> u64 {
        samples * 1000 / self.sample_rate as u64
    }

    /// Reset all onset tracking state
    #[allow(dead_code)]
    fn reset_onset_state(&mut self) {
        self.is_pending_voiced = false;
        self.is_pending_whisper = false;
        self.voiced_onset_count = 0;
        self.whisper_onset_count = 0;
        self.voiced_grace_count = 0;
        self.whisper_grace_count = 0;
    }

    /// Add samples to the lookback ring buffer
    fn push_to_lookback_buffer(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.lookback_buffer[self.lookback_write_index] = sample;
            self.lookback_write_index = (self.lookback_write_index + 1) % self.lookback_capacity;
            if self.lookback_write_index == 0 {
                self.lookback_filled = true;
            }
        }
    }

    /// Get the contents of the lookback buffer in chronological order
    fn get_lookback_buffer_contents(&self) -> Vec<f32> {
        if !self.lookback_filled {
            return self.lookback_buffer[..self.lookback_write_index].to_vec();
        }
        let mut result = Vec::with_capacity(self.lookback_capacity);
        result.extend_from_slice(&self.lookback_buffer[self.lookback_write_index..]);
        result.extend_from_slice(&self.lookback_buffer[..self.lookback_write_index]);
        result
    }

    /// Find the true start of speech by scanning backward through the lookback buffer.
    fn find_lookback_start(&self) -> (Vec<f32>, u32) {
        let buffer = self.get_lookback_buffer_contents();
        if buffer.is_empty() {
            return (Vec::new(), 0);
        }

        const CHUNK_SIZE: usize = 128;
        let margin_samples = (self.sample_rate as usize * 20) / 1000;
        let threshold_linear = 10.0f32.powf(self.lookback_threshold_db / 20.0);

        let mut first_above_threshold_idx = buffer.len();

        let mut pos = buffer.len();
        while pos > 0 {
            let chunk_start = pos.saturating_sub(CHUNK_SIZE);
            let chunk = &buffer[chunk_start..pos];

            let peak = chunk.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

            if peak >= threshold_linear {
                first_above_threshold_idx = chunk_start;
            } else if first_above_threshold_idx < buffer.len() {
                break;
            }

            pos = chunk_start;
        }

        let start_with_margin = first_above_threshold_idx.saturating_sub(margin_samples);
        let lookback_samples = buffer[start_with_margin..].to_vec();
        let samples_before = buffer.len() - start_with_margin;
        let offset_ms = (samples_before as u64 * 1000 / self.sample_rate as u64) as u32;

        (lookback_samples, offset_ms)
    }

    /// Get the current speech detection metrics.
    pub fn get_metrics(&self) -> SpeechMetrics {
        SpeechMetrics {
            amplitude_db: self.last_amplitude_db,
            zcr: self.last_zcr,
            centroid_hz: self.last_centroid_hz,
            is_speaking: self.is_speaking,
            is_voiced_pending: self.is_pending_voiced,
            is_whisper_pending: self.is_pending_whisper,
            is_transient: self.last_is_transient,
            is_lookback_speech: false,
            lookback_offset_ms: self.last_lookback_offset_ms,
            is_word_break: self.last_is_word_break,
        }
    }

    /// Get the last speech state change detected during process().
    pub fn take_state_change(&mut self) -> SpeechStateChange {
        std::mem::replace(&mut self.last_state_change, SpeechStateChange::None)
    }

    /// Take the last word break event, resetting it to None.
    pub fn take_word_break_event(&mut self) -> Option<WordBreakEvent> {
        self.last_word_break_event.take()
    }

    /// Update the running average of speech amplitude
    #[allow(dead_code)]
    fn update_speech_amplitude_average(&mut self, rms: f32, sample_count: u32) {
        self.recent_speech_amplitude_sum += rms * sample_count as f32;
        self.recent_speech_amplitude_count += sample_count;

        if self.recent_speech_amplitude_count > self.recent_speech_window_samples {
            let scale = self.recent_speech_window_samples as f32
                / self.recent_speech_amplitude_count as f32;
            self.recent_speech_amplitude_sum *= scale;
            self.recent_speech_amplitude_count = self.recent_speech_window_samples;
        }
    }

    /// Get the recent average speech amplitude (linear)
    #[allow(dead_code)]
    fn get_recent_speech_amplitude(&self) -> f32 {
        if self.recent_speech_amplitude_count == 0 {
            return 0.0;
        }
        self.recent_speech_amplitude_sum / self.recent_speech_amplitude_count as f32
    }

    /// Reset word break detection state
    #[allow(dead_code)]
    fn reset_word_break_state(&mut self) {
        self.in_word_break = false;
        self.word_break_sample_count = 0;
        self.word_break_start_speech_samples = 0;
        self.recent_speech_amplitude_sum = 0.0;
        self.recent_speech_amplitude_count = 0;
        self.last_is_word_break = false;
        self.last_word_break_event = None;
    }

    /// Process audio samples for speech detection.
    ///
    /// Uses simple amplitude-based silence detection:
    /// - Frame above speech threshold dB → speech candidate
    /// - Onset requires sustained energy above threshold
    /// - Speech ends after hold_time of sustained silence
    ///
    /// ZCR, spectral centroid, transient rejection, and word break detection
    /// are disabled (metrics are still computed for visualization but do not
    /// affect speech state).
    pub fn process(&mut self, samples: &[f32]) {
        // Reset per-frame outputs
        self.last_state_change = SpeechStateChange::None;
        self.last_word_break_event = None;

        // Add samples to lookback buffer
        self.push_to_lookback_buffer(samples);

        // Calculate features (all computed for metrics/visualization)
        let rms = Self::calculate_rms(samples);
        let db = Self::amplitude_to_db(rms);
        let zcr = Self::calculate_zcr(samples);
        let centroid = self.estimate_spectral_centroid(samples, db);

        // Store metrics for visualization
        self.last_amplitude_db = db;
        self.last_zcr = zcr;
        self.last_centroid_hz = centroid;
        self.last_is_transient = false; // disabled
        self.last_lookback_offset_ms = None;
        self.last_is_word_break = false;

        if !self.initialized {
            self.initialized = true;
            return;
        }

        // Simple amplitude-based speech detection.
        // Use the voiced threshold as the speech/silence boundary.
        let is_speech_candidate = db >= self.voiced_config.threshold_db;

        let samples_len = samples.len() as u32;

        if is_speech_candidate {
            self.silence_sample_count = 0;

            if self.is_speaking {
                // Already speaking — just accumulate duration
                self.speech_sample_count += samples.len() as u64;
            } else {
                // Onset accumulation (reuse voiced onset counter)
                self.voiced_onset_count += samples_len;

                if self.voiced_onset_count >= self.voiced_config.onset_samples {
                    // Speech confirmed
                    self.is_speaking = true;
                    self.speech_sample_count = self.voiced_onset_count as u64;
                    self.voiced_onset_count = 0;

                    let (lookback_samples, lookback_offset_ms) = self.find_lookback_start();
                    self.last_lookback_offset_ms = Some(lookback_offset_ms);

                    self.last_state_change = SpeechStateChange::Started {
                        lookback_samples: lookback_samples.len(),
                    };

                    let payload = SpeechEventPayload {
                        duration_ms: None,
                        lookback_offset_ms: Some(lookback_offset_ms),
                    };

                    if let Some(ref callback) = self.callback {
                        callback.on_speech_started(payload);
                    }

                    tracing::debug!(
                        "Speech started (amplitude mode, lookback: {}ms)",
                        lookback_offset_ms
                    );
                }
            }
        } else {
            // Below threshold — reset onset counter, accumulate silence
            self.voiced_onset_count = 0;

            if self.is_speaking {
                self.silence_sample_count += samples_len;
                self.speech_sample_count += samples.len() as u64;

                if self.silence_sample_count >= self.hold_samples {
                    let duration_ms = self.samples_to_ms(self.speech_sample_count);
                    self.is_speaking = false;
                    self.speech_sample_count = 0;

                    self.last_state_change = SpeechStateChange::Ended { duration_ms };

                    let payload = SpeechEventPayload {
                        duration_ms: Some(duration_ms),
                        lookback_offset_ms: None,
                    };

                    if let Some(ref callback) = self.callback {
                        callback.on_speech_ended(payload);
                    }

                    tracing::debug!("Speech ended (duration: {}ms)", duration_ms);
                }
            }
        }
    }
}

// ============================================================================
// Visualization Processor
// ============================================================================

/// A single column of spectrogram data ready for rendering
#[derive(Clone, Debug, Serialize)]
pub struct SpectrogramColumn {
    /// RGB triplets for each pixel row (height * 3 bytes)
    pub colors: Vec<u8>,
}

/// Payload for visualization data events
#[derive(Clone, Debug, Serialize)]
pub struct VisualizationPayload {
    /// Pre-downsampled waveform amplitudes
    pub waveform: Vec<f32>,
    /// Spectrogram columns with RGB colors (one per completed FFT window)
    pub spectrogram: Vec<SpectrogramColumn>,
    /// Speech detection metrics (present when speech processor is active)
    pub speech_metrics: Option<SpeechMetrics>,
}

/// Callback trait for receiving visualization data
pub trait VisualizationCallback: Send {
    /// Called when new visualization data is available
    fn on_visualization_data(&self, payload: VisualizationPayload);
}

/// Color stop for gradient interpolation
struct ColorStop {
    position: f32,
    r: u8,
    g: u8,
    b: u8,
}

/// Visualization processor that computes render-ready waveform and spectrogram data.
pub struct VisualizationProcessor {
    /// Sample rate for frequency calculations
    sample_rate: u32,
    /// Target height for spectrogram output (pixels)
    output_height: usize,
    /// FFT size (must be power of 2)
    fft_size: usize,
    /// FFT planner/executor
    fft: Arc<dyn rustfft::Fft<f32>>,
    /// Pre-computed Hanning window
    hanning_window: Vec<f32>,
    /// Buffer for accumulating samples for FFT
    fft_buffer: Vec<f32>,
    /// Current write position in FFT buffer
    fft_write_index: usize,
    /// Pre-computed color lookup table (256 entries, RGB)
    color_lut: Vec<[u8; 3]>,
    /// Waveform accumulator for downsampling
    waveform_buffer: Vec<f32>,
    /// Target waveform output samples per emit
    waveform_target_samples: usize,
    /// Speech metrics to include in next visualization event
    pending_speech_metrics: Option<SpeechMetrics>,
    /// Callback for visualization events
    callback: Option<Arc<dyn VisualizationCallback>>,
}

impl VisualizationProcessor {
    /// Create a new visualization processor
    pub fn new(sample_rate: u32, output_height: usize) -> Self {
        let fft_size = 512;

        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);

        let hanning_window: Vec<f32> = (0..fft_size)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos())
            })
            .collect();

        let color_lut = Self::build_color_lut();

        Self {
            sample_rate,
            output_height,
            fft_size,
            fft,
            hanning_window,
            fft_buffer: Vec::with_capacity(fft_size),
            fft_write_index: 0,
            color_lut,
            waveform_buffer: Vec::with_capacity(256),
            waveform_target_samples: 64,
            pending_speech_metrics: None,
            callback: None,
        }
    }

    /// Set the callback for visualization events
    pub fn set_callback(&mut self, callback: Arc<dyn VisualizationCallback>) {
        self.callback = Some(callback);
    }

    /// Set speech metrics to include in the next visualization event
    pub fn set_speech_metrics(&mut self, metrics: SpeechMetrics) {
        self.pending_speech_metrics = Some(metrics);
    }

    /// Build the color lookup table
    fn build_color_lut() -> Vec<[u8; 3]> {
        let stops = [
            ColorStop {
                position: 0.00,
                r: 10,
                g: 15,
                b: 26,
            },
            ColorStop {
                position: 0.15,
                r: 0,
                g: 50,
                b: 200,
            },
            ColorStop {
                position: 0.35,
                r: 0,
                g: 255,
                b: 150,
            },
            ColorStop {
                position: 0.60,
                r: 200,
                g: 255,
                b: 0,
            },
            ColorStop {
                position: 0.80,
                r: 255,
                g: 155,
                b: 0,
            },
            ColorStop {
                position: 1.00,
                r: 255,
                g: 0,
                b: 0,
            },
        ];

        let mut lut = Vec::with_capacity(256);

        for i in 0..256 {
            let t_raw = i as f32 / 255.0;
            let t = t_raw.powf(0.7);

            let mut color = [255u8, 0, 0];

            for j in 0..stops.len() - 1 {
                let s1 = &stops[j];
                let s2 = &stops[j + 1];

                if t >= s1.position && t <= s2.position {
                    let s = (t - s1.position) / (s2.position - s1.position);
                    color[0] = (s1.r as f32 + s * (s2.r as f32 - s1.r as f32)).round() as u8;
                    color[1] = (s1.g as f32 + s * (s2.g as f32 - s1.g as f32)).round() as u8;
                    color[2] = (s1.b as f32 + s * (s2.b as f32 - s1.b as f32)).round() as u8;
                    break;
                }
            }

            lut.push(color);
        }

        lut
    }

    /// Convert normalized position to fractional frequency bin
    fn position_to_freq_bin(&self, pos: f32, num_bins: usize) -> f32 {
        const MIN_FREQ: f32 = 20.0;
        const MAX_FREQ: f32 = 24000.0;

        let min_log = MIN_FREQ.log10();
        let max_log = MAX_FREQ.log10();

        let log_freq = min_log + pos * (max_log - min_log);
        let freq = 10.0f32.powf(log_freq);

        let bin_index = freq * self.fft_size as f32 / self.sample_rate as f32;
        bin_index.clamp(0.0, (num_bins - 1) as f32)
    }

    /// Get magnitude for a pixel row
    fn get_magnitude_for_pixel(&self, magnitudes: &[f32], y: usize, height: usize) -> f32 {
        let num_bins = magnitudes.len();

        let pos1 = (height - 1 - y) as f32 / height as f32;
        let pos2 = (height - y) as f32 / height as f32;

        let bin1 = self.position_to_freq_bin(pos1, num_bins);
        let bin2 = self.position_to_freq_bin(pos2, num_bins);

        let bin_low = bin1.min(bin2).max(0.0);
        let bin_high = bin1.max(bin2).min((num_bins - 1) as f32);

        if bin_high - bin_low < 1.0 {
            let bin_floor = bin_low.floor() as usize;
            let bin_ceil = (bin_floor + 1).min(num_bins - 1);
            let frac = bin_low - bin_floor as f32;
            return magnitudes[bin_floor] * (1.0 - frac) + magnitudes[bin_ceil] * frac;
        }

        let mut sum = 0.0f32;
        let mut weight = 0.0f32;

        let start_bin = bin_low.floor() as usize;
        let end_bin = bin_high.ceil() as usize;

        #[allow(clippy::needless_range_loop)]
        for b in start_bin..=end_bin.min(num_bins - 1) {
            let bin_start = b as f32;
            let bin_end = (b + 1) as f32;
            let overlap_start = bin_low.max(bin_start);
            let overlap_end = bin_high.min(bin_end);
            let overlap_weight = (overlap_end - overlap_start).max(0.0);

            if overlap_weight > 0.0 {
                sum += magnitudes[b] * overlap_weight;
                weight += overlap_weight;
            }
        }

        if weight > 0.0 {
            sum / weight
        } else {
            0.0
        }
    }

    /// Process FFT buffer and generate spectrogram column
    fn process_fft(&self) -> SpectrogramColumn {
        let mut complex_buffer: Vec<Complex<f32>> = self
            .fft_buffer
            .iter()
            .zip(self.hanning_window.iter())
            .map(|(&sample, &window)| Complex::new(sample * window, 0.0))
            .collect();

        complex_buffer.resize(self.fft_size, Complex::new(0.0, 0.0));

        self.fft.process(&mut complex_buffer);

        let num_bins = self.fft_size / 2;
        let magnitudes: Vec<f32> = complex_buffer[..num_bins]
            .iter()
            .map(|c| (c.re * c.re + c.im * c.im).sqrt() / self.fft_size as f32)
            .collect();

        let max_mag = magnitudes.iter().cloned().fold(0.001f32, f32::max);
        let ref_level = max_mag.max(0.05);

        let mut colors = Vec::with_capacity(self.output_height * 3);

        for y in 0..self.output_height {
            let magnitude = self.get_magnitude_for_pixel(&magnitudes, y, self.output_height);

            let normalized_db = (1.0 + magnitude / ref_level * 9.0).log10();
            let normalized = normalized_db.clamp(0.0, 1.0);

            let color_idx = (normalized * 255.0).floor() as usize;
            let color = &self.color_lut[color_idx.min(255)];

            colors.push(color[0]);
            colors.push(color[1]);
            colors.push(color[2]);
        }

        SpectrogramColumn { colors }
    }

    /// Downsample waveform buffer using peak detection
    fn downsample_waveform(&self, samples: &[f32]) -> Vec<f32> {
        if samples.is_empty() {
            return Vec::new();
        }

        let window_size = (samples.len() / self.waveform_target_samples).max(1);
        let output_count = samples.len().div_ceil(window_size);

        let mut output = Vec::with_capacity(output_count);

        for chunk in samples.chunks(window_size) {
            let peak = chunk
                .iter()
                .max_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap())
                .copied()
                .unwrap_or(0.0);
            output.push(peak);
        }

        output
    }

    /// Process audio samples for visualization.
    ///
    /// Returns the visualization payload. Also calls the callback if set.
    /// A single call may produce zero, one, or multiple spectrogram columns
    /// depending on the chunk size relative to the FFT size (512 samples).
    pub fn process(&mut self, samples: &[f32]) -> Option<crate::VisualizationData> {
        let mut spectrogram_columns: Vec<SpectrogramColumn> = Vec::new();

        // Accumulate samples for FFT, producing a column each time the buffer
        // fills.  This avoids dropping samples when the incoming chunk is larger
        // than the FFT size.
        for &sample in samples {
            if self.fft_buffer.len() <= self.fft_write_index {
                self.fft_buffer.push(sample);
            } else {
                self.fft_buffer[self.fft_write_index] = sample;
            }
            self.fft_write_index += 1;

            if self.fft_write_index >= self.fft_size {
                spectrogram_columns.push(self.process_fft());
                self.fft_write_index = 0;
            }
        }

        // Accumulate samples for waveform
        self.waveform_buffer.extend_from_slice(samples);

        // Downsample waveform
        let waveform = self.downsample_waveform(&self.waveform_buffer);
        self.waveform_buffer.clear();

        // Take speech metrics
        let speech_metrics = self.pending_speech_metrics.take();

        // Build internal payload
        let payload = VisualizationPayload {
            waveform: waveform.clone(),
            spectrogram: spectrogram_columns.clone(),
            speech_metrics: speech_metrics.clone(),
        };

        if let Some(ref callback) = self.callback {
            callback.on_visualization_data(payload);
        }

        // Duration of this chunk in milliseconds — used by the frontend to
        // correctly place time labels on the speech-activity graph.
        let frame_interval_ms = samples.len() as f32 / self.sample_rate as f32 * 1000.0;

        // Convert to public types and return
        let viz = crate::VisualizationData {
            waveform,
            spectrogram: spectrogram_columns
                .into_iter()
                .map(|s| crate::SpectrogramColumn { colors: s.colors })
                .collect(),
            speech_metrics: speech_metrics.map(|m| crate::SpeechMetrics {
                amplitude_db: m.amplitude_db,
                zcr: m.zcr,
                centroid_hz: m.centroid_hz,
                is_speaking: m.is_speaking,
                voiced_onset_pending: m.is_voiced_pending,
                whisper_onset_pending: m.is_whisper_pending,
                is_transient: m.is_transient,
                is_lookback_speech: m.is_lookback_speech,
                is_word_break: m.is_word_break,
            }),
            sample_rate: self.sample_rate,
            frame_interval_ms,
        };

        Some(viz)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 48000;
    /// 10ms chunk at 48kHz = 480 samples (mono)
    const CHUNK_SIZE: usize = 480;

    /// Generate a sine wave chunk at the given frequency and amplitude.
    /// Returns mono f32 samples.
    fn sine_chunk(freq_hz: f32, amplitude: f32, sample_rate: u32, num_samples: usize) -> Vec<f32> {
        (0..num_samples)
            .map(|i| {
                amplitude
                    * (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    /// Generate a silence chunk.
    fn silence_chunk(num_samples: usize) -> Vec<f32> {
        vec![0.0; num_samples]
    }

    /// Feed N milliseconds of a given chunk pattern into the detector.
    fn feed_ms(detector: &mut SpeechDetector, chunk: &[f32], duration_ms: u32) {
        let chunks_needed =
            (SAMPLE_RATE as u64 * duration_ms as u64 / 1000 / CHUNK_SIZE as u64) as u32;
        for _ in 0..chunks_needed {
            detector.process(chunk);
        }
    }

    // --- Speech detection tests ---

    #[test]
    fn voiced_speech_detected_after_onset() {
        let mut det = SpeechDetector::new(SAMPLE_RATE);
        // 300Hz sine at -30dB (amplitude ~0.032) - clear voiced speech
        let chunk = sine_chunk(300.0, 0.032, SAMPLE_RATE, CHUNK_SIZE);

        // Feed 100ms (more than 80ms onset)
        for _ in 0..10 {
            det.process(&chunk);
        }

        assert!(
            det.is_speaking,
            "Speech should be detected after 100ms of voiced audio"
        );
    }

    // --- Feature computation tests ---

    #[test]
    fn rms_of_silence_is_zero() {
        let silence = silence_chunk(CHUNK_SIZE);
        let rms = SpeechDetector::calculate_rms(&silence);
        assert_eq!(rms, 0.0);
    }

    #[test]
    fn zcr_of_silence_is_zero() {
        let silence = silence_chunk(CHUNK_SIZE);
        let zcr = SpeechDetector::calculate_zcr(&silence);
        assert_eq!(zcr, 0.0);
    }

    #[test]
    fn rms_of_sine_matches_theory() {
        // RMS of a sine wave with amplitude A is A/sqrt(2)
        let amplitude = 0.5;
        let chunk = sine_chunk(440.0, amplitude, SAMPLE_RATE, CHUNK_SIZE * 10); // longer for accuracy
        let rms = SpeechDetector::calculate_rms(&chunk);
        let expected = amplitude / 2.0f32.sqrt();
        assert!(
            (rms - expected).abs() < 0.01,
            "RMS of sine should be ~{}, got {}",
            expected,
            rms
        );
    }

    #[test]
    fn silence_ends_speech_after_hold_time() {
        let mut det = SpeechDetector::new(SAMPLE_RATE);
        // -30dB amplitude (above -42 threshold)
        let speech = sine_chunk(300.0, 0.032, SAMPLE_RATE, CHUNK_SIZE);
        let silence = silence_chunk(CHUNK_SIZE);

        // Establish speech
        feed_ms(&mut det, &speech, 200);
        assert!(det.is_speaking, "Speech should be active after onset");

        // Feed silence past hold time (200ms)
        feed_ms(&mut det, &silence, 210);
        assert!(!det.is_speaking, "Speech should end after hold time");
    }

    #[test]
    fn brief_silence_does_not_end_speech() {
        let mut det = SpeechDetector::new(SAMPLE_RATE);
        let speech = sine_chunk(300.0, 0.032, SAMPLE_RATE, CHUNK_SIZE);
        let silence = silence_chunk(CHUNK_SIZE);

        // Establish speech
        feed_ms(&mut det, &speech, 200);
        assert!(det.is_speaking);

        // Brief silence (100ms, below 200ms hold time)
        feed_ms(&mut det, &silence, 100);
        assert!(det.is_speaking, "Brief silence should not end speech");

        // Resume speech
        feed_ms(&mut det, &speech, 50);
        assert!(
            det.is_speaking,
            "Speech should still be active after brief gap"
        );
    }

    #[test]
    fn below_threshold_audio_does_not_trigger_speech() {
        let mut det = SpeechDetector::new(SAMPLE_RATE);
        // -50dB amplitude (below -42 threshold)
        let quiet = sine_chunk(300.0, 0.003, SAMPLE_RATE, CHUNK_SIZE);

        feed_ms(&mut det, &quiet, 500);
        assert!(
            !det.is_speaking,
            "Audio below threshold should not trigger speech"
        );
    }

    /// Diagnostic test: process the reference WAV file through the speech detector
    /// and write per-frame metrics to CSV for offline analysis.
    ///
    /// Run with: cargo test -- --nocapture dump_wav_metrics
    #[test]
    fn dump_wav_metrics() {
        use std::io::Write;

        // Try project root first, then the recordings dir
        let wav_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("exampe.wav");
        if !wav_path.exists() {
            eprintln!("Skipping dump_wav_metrics: {:?} not found", wav_path);
            return;
        }

        let reader = hound::WavReader::open(&wav_path)
            .unwrap_or_else(|e| panic!("Failed to open {:?}: {}", wav_path, e));
        let spec = reader.spec();
        let wav_sr = spec.sample_rate;
        let wav_ch = spec.channels as usize;

        eprintln!(
            "WAV: {}Hz, {} ch, {} bit, {:?}",
            wav_sr, wav_ch, spec.bits_per_sample, spec.sample_format
        );

        // Decode to f32
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

        // Convert to mono (same as audio loop)
        let mono: Vec<f32> = if wav_ch > 1 {
            crate::audio::convert_to_mono(&raw_samples, wav_ch)
        } else {
            raw_samples
        };

        eprintln!(
            "Mono samples: {} ({:.2}s)",
            mono.len(),
            mono.len() as f64 / wav_sr as f64
        );

        // Process in 10ms chunks (same as play_file / audio loop)
        let chunk_size = (wav_sr as usize) / 100; // 480 for 48kHz
        let mut det = SpeechDetector::new(wav_sr);

        let csv_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("speech_metrics.csv");
        let mut csv = std::fs::File::create(&csv_path)
            .unwrap_or_else(|e| panic!("Cannot create {:?}: {}", csv_path, e));

        writeln!(
            csv,
            "frame,time_ms,amplitude_db,zcr,centroid_hz,is_speaking,is_voiced_pending,is_whisper_pending,is_transient,is_word_break,rms_linear"
        )
        .unwrap();

        let mut frame_idx = 0u32;
        for chunk in mono.chunks(chunk_size) {
            let time_ms = frame_idx as f64 * 10.0;

            // Compute raw features for CSV (same math as process())
            let rms = SpeechDetector::calculate_rms(chunk);
            let db = SpeechDetector::amplitude_to_db(rms);
            let _zcr = SpeechDetector::calculate_zcr(chunk);
            let _centroid = det.estimate_spectral_centroid(chunk, db);

            // Run the detector
            det.process(chunk);
            let m = det.get_metrics();

            writeln!(
                csv,
                "{},{:.1},{:.2},{:.4},{:.1},{},{},{},{},{},{:.6}",
                frame_idx,
                time_ms,
                m.amplitude_db,
                m.zcr,
                m.centroid_hz,
                m.is_speaking as u8,
                m.is_voiced_pending as u8,
                m.is_whisper_pending as u8,
                m.is_transient as u8,
                m.is_word_break as u8,
                rms,
            )
            .unwrap();

            frame_idx += 1;
        }

        eprintln!("Wrote {} frames to {:?}", frame_idx, csv_path);

        // Basic sanity: we should have processed the whole file
        let expected_frames = (mono.len() + chunk_size - 1) / chunk_size;
        assert_eq!(frame_idx as usize, expected_frames);
    }
}

// =============================================================================
// AgcProcessor
// =============================================================================

/// Minimum power floor to prevent gain explosion on silence.
const AGC_NOISE_FLOOR_POWER: f32 = 1e-10;

/// Number of audio chunks between `AgcGainChanged` event emissions (~100 ms at
/// 10 ms chunks; we count chunks and emit when the elapsed estimate exceeds the
/// threshold, so the actual rate is approximate).
const AGC_EVENT_INTERVAL_CHUNKS: u32 = 10;

/// Time constant in milliseconds for decaying gain toward unity when the
/// signal power is in the gate region (between the noise floor and the gate
/// threshold). 500 ms is slow enough to avoid audible gain changes during
/// brief inter-word pauses but fast enough to settle within 1-2 seconds.
const AGC_GATE_DECAY_TIME_MS: f32 = 500.0;

/// Maximum upward gain change allowed per chunk once AGC boosting is active.
/// This smooths the transition out of the gate/borderline-noise region so the
/// processed signal does not jump abruptly into amplified noise or speech.
const AGC_MAX_GAIN_RISE_DB_PER_CHUNK: f32 = 3.0;

/// Automatic Gain Control processor.
///
/// Implements a feed-back RMS envelope follower with asymmetric attack/release
/// exponential smoothing. All state is maintained between `process` calls; the
/// processor is designed to run on a single dedicated thread with no allocations
/// per chunk.
///
/// # Algorithm
///
/// Per chunk:
/// 1. Compute `chunk_power = mean(s² for s in samples)`.
/// 2. Select smoothing coefficient: attack (fast) when signal is growing,
///    release (slow) when signal is falling.
/// 3. Update envelope: `power = α * power + (1-α) * chunk_power`.
/// 4. Determine gain based on power region:
///    - Above gate threshold: `gain = target_rms / sqrt(power)`, clamped to
///      `[min, max]` (normal AGC).
///    - Between noise floor and gate threshold: decay gain toward unity (1.0).
///    - At or below noise floor: hold current gain.
/// 5. Apply gain in-place; clamp each sample to `[-1.0, 1.0]`.
pub struct AgcProcessor {
    /// Current smoothed power estimate (linear, mean-squared).
    power_estimate: f32,
    /// Current linear gain applied to samples.
    current_gain_linear: f32,
    /// Active configuration (may be hot-swapped between chunks).
    config: AgcConfig,
    /// Pre-computed gate threshold in the power domain, derived from
    /// `config.gate_threshold_db` as `10^(gate_threshold_db / 10)`.
    gate_threshold_power: f32,
    /// Pre-computed boost activation threshold in the power domain.
    boost_threshold_power: f32,
    /// Chunk counter for throttling `AgcGainChanged` event emissions.
    chunks_since_event: u32,
    /// Elapsed hold time in milliseconds when transitioning from gate region.
    /// Used to delay gain increase and prevent noise burst amplification.
    hold_timer_ms: f32,
    /// Tracks which power region we were in during the last chunk.
    /// Used to detect transitions between noise floor / gate region / above threshold.
    last_power_region: PowerRegion,
    /// Cached gate hold time from config for hot-swap safety.
    gate_hold_time_ms: f32,
}

/// Power regions for AGC gate logic.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PowerRegion {
    /// Below noise floor - gain is held constant.
    BelowNoiseFloor,
    /// Between noise floor and gate threshold - gain decays toward unity.
    GateRegion,
    /// Above gate threshold - normal AGC gain computation.
    AboveThreshold,
}

impl AgcProcessor {
    /// Create a new `AgcProcessor` initialised to a neutral state.
    ///
    /// Initial power is set to a small non-zero value so the first chunk does
    /// not produce an extreme gain jump.
    pub fn new(config: AgcConfig) -> Self {
        let gate_threshold_power = db_to_power(config.gate_threshold_db);
        let boost_threshold_power = db_to_power(config.boost_threshold_db);
        let gate_hold_time_ms = config.gate_hold_time_ms;
        Self {
            power_estimate: 1e-6,
            current_gain_linear: 1.0,
            config,
            gate_threshold_power,
            boost_threshold_power,
            chunks_since_event: 0,
            hold_timer_ms: gate_hold_time_ms,
            last_power_region: PowerRegion::BelowNoiseFloor,
            gate_hold_time_ms,
        }
    }

    /// Replace the active configuration.
    ///
    /// Takes effect on the next call to `process`. Safe to call from a different
    /// thread when the processor is not mid-chunk (caller is responsible for
    /// ensuring this, typically via `try_lock` on the capture thread).
    pub fn update_config(&mut self, config: AgcConfig) {
        self.gate_threshold_power = db_to_power(config.gate_threshold_db);
        self.boost_threshold_power = db_to_power(config.boost_threshold_db);
        self.gate_hold_time_ms = config.gate_hold_time_ms;
        self.config = config;
    }

    /// Process a chunk of mono samples in-place.
    ///
    /// When AGC is disabled (`config.enabled == false`) this is a no-op.
    ///
    /// Returns `Some(gain_db)` approximately every 100 ms (every
    /// `AGC_EVENT_INTERVAL_CHUNKS` chunks) to signal that an
    /// `AgcGainChanged` event should be broadcast. Returns `None` otherwise.
    pub fn process(&mut self, samples: &mut [f32], _sample_rate: u32) -> Option<f32> {
        if !self.config.enabled || samples.is_empty() {
            return None;
        }

        // 1. Compute chunk power (mean squared).
        let chunk_power: f32 = {
            let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
            sum_sq / samples.len() as f32
        };

        // 2. Select smoothing coefficient based on direction.
        //    Chunk duration in seconds is approximated from the sample rate;
        //    however since `_sample_rate` is informational, we rely on the
        //    caller-supplied rate. For the envelope smoother, the absolute
        //    time constant matters more than the per-chunk duration precision.
        //    We use a nominal 10 ms chunk as the base time step.
        let chunk_duration_s = samples.len() as f32 / _sample_rate as f32;
        let alpha = if chunk_power > self.power_estimate {
            // Signal getting louder → use attack (fast).
            let tau = self.config.attack_time_ms / 1000.0;
            (-chunk_duration_s / tau).exp()
        } else {
            // Signal getting quieter → use release (slow).
            let tau = self.config.release_time_ms / 1000.0;
            (-chunk_duration_s / tau).exp()
        };

        // 3. Update envelope estimate.
        self.power_estimate = alpha * self.power_estimate + (1.0 - alpha) * chunk_power;

        // 4. Determine current power region.
        let current_region = if self.power_estimate > self.gate_threshold_power {
            PowerRegion::AboveThreshold
        } else if self.power_estimate > AGC_NOISE_FLOOR_POWER {
            PowerRegion::GateRegion
        } else {
            PowerRegion::BelowNoiseFloor
        };
        let boost_ready = self.power_estimate >= self.boost_threshold_power;

        // 5. Handle hold time logic for gain increases.
        // The countdown only runs once the signal is strong enough to justify
        // gain increase above unity.
        let chunk_duration_ms = chunk_duration_s * 1000.0;

        if boost_ready {
            if self.hold_timer_ms > 0.0 {
                self.hold_timer_ms -= chunk_duration_ms;
                if self.hold_timer_ms < 0.0 {
                    self.hold_timer_ms = 0.0;
                }
            }
        } else {
            self.hold_timer_ms = self.gate_hold_time_ms;
        }

        // 6. Determine gain based on power region, boost threshold, and hold time.
        let hold_active = boost_ready && self.hold_timer_ms > 0.0;

        if hold_active {
            // During hold time, prevent gain from rising into the segment onset.
            let target_rms = db_to_linear(self.config.target_level_db);
            let current_rms = self.power_estimate.sqrt();
            let raw_gain = target_rms / current_rms;

            let min_gain = db_to_linear(self.config.min_gain_db);
            let max_gain = db_to_linear(self.config.max_gain_db);
            let computed_gain = raw_gain.clamp(min_gain, max_gain);
            self.current_gain_linear = self.current_gain_linear.min(computed_gain);
        } else if boost_ready {
            // Strong signal and hold expired -> normal AGC gain computation.
            let target_rms = db_to_linear(self.config.target_level_db);
            let current_rms = self.power_estimate.sqrt();
            let raw_gain = target_rms / current_rms;

            let min_gain = db_to_linear(self.config.min_gain_db);
            let max_gain = db_to_linear(self.config.max_gain_db);
            let target_gain = raw_gain.clamp(min_gain, max_gain);
            let max_rise_linear = db_to_linear(AGC_MAX_GAIN_RISE_DB_PER_CHUNK);
            let allowed_gain = self.current_gain_linear * max_rise_linear;
            self.current_gain_linear = if target_gain > self.current_gain_linear {
                target_gain.min(allowed_gain)
            } else {
                target_gain
            };
        } else if current_region != PowerRegion::BelowNoiseFloor {
            // Borderline noise between the gate threshold and boost threshold
            // decays toward unity instead of being amplified.
            let tau = AGC_GATE_DECAY_TIME_MS / 1000.0;
            let decay_alpha = (-chunk_duration_s / tau).exp();
            self.current_gain_linear =
                decay_alpha * self.current_gain_linear + (1.0 - decay_alpha) * 1.0;
        }
        // Below noise floor → hold the current gain (don't amplify silence).

        // Update last power region for next chunk
        self.last_power_region = current_region;

        // 7. Apply gain in-place.
        let g = self.current_gain_linear;
        for s in samples.iter_mut() {
            *s = (*s * g).clamp(-1.0, 1.0);
        }

        // 8. Throttle event emission.
        self.chunks_since_event += 1;
        if self.chunks_since_event >= AGC_EVENT_INTERVAL_CHUNKS {
            self.chunks_since_event = 0;
            Some(self.current_gain_db())
        } else {
            None
        }
    }

    /// Return the current AGC gain in dB.
    pub fn current_gain_db(&self) -> f32 {
        linear_to_db(self.current_gain_linear)
    }
}

/// Convert dBFS to a linear amplitude multiplier.
#[inline]
fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Convert dBFS to a power (mean-squared) value: `10^(db / 10)`.
#[inline]
fn db_to_power(db: f32) -> f32 {
    10f32.powf(db / 10.0)
}

/// Convert a linear amplitude multiplier to dBFS.
#[inline]
fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

#[cfg(test)]
mod agc_tests {
    use super::*;

    /// Helper: generate a sine wave with a given RMS amplitude.
    fn sine_wave(rms: f32, num_samples: usize, sample_rate: u32) -> Vec<f32> {
        // RMS of a sine wave A*sin(t) is A/sqrt(2), so amplitude = rms * sqrt(2).
        let amplitude = rms * 2f32.sqrt();
        let freq = 440.0_f32;
        (0..num_samples)
            .map(|i| {
                amplitude
                    * (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    fn default_config() -> AgcConfig {
        AgcConfig {
            enabled: true,
            target_level_db: -18.0,
            attack_time_ms: 10.0,
            release_time_ms: 200.0,
            min_gain_db: -6.0,
            max_gain_db: 30.0,
            gate_threshold_db: -50.0,
            boost_threshold_db: -40.0,
            gate_hold_time_ms: 50.0,
        }
    }

    /// 2.6: Unity gain convergence — signal already at target level.
    #[test]
    fn agc_unity_gain_convergence() {
        let sample_rate = 16000_u32;
        let target_rms = db_to_linear(-18.0);
        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        // Feed 500 ms of audio at target level.
        let chunk_size = 160; // 10 ms
        let num_chunks = 50; // 500 ms

        for _ in 0..num_chunks {
            let mut chunk = sine_wave(target_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // After convergence the gain should be within ±1 dB of 0 dB (linear ~1.0).
        let gain_db = proc.current_gain_db();
        assert!(
            gain_db.abs() < 1.5,
            "Expected gain near 0 dB after convergence, got {:.2} dB",
            gain_db
        );
    }

    /// 2.7: Gain increases for quiet input.
    #[test]
    fn agc_increases_gain_for_quiet_input() {
        let sample_rate = 16000_u32;
        let quiet_rms = db_to_linear(-40.0);
        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        // Feed 1000 ms of quiet audio (release time = 200 ms, so 5× release).
        let chunk_size = 160;
        for _ in 0..100 {
            let mut chunk = sine_wave(quiet_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // Gain should be significantly positive (above 10 dB).
        let gain_db = proc.current_gain_db();
        assert!(
            gain_db > 10.0,
            "Expected AGC to boost quiet input (>10 dB), got {:.2} dB",
            gain_db
        );
    }

    /// 2.8: Gain decreases for loud input.
    #[test]
    fn agc_decreases_gain_for_loud_input() {
        let sample_rate = 16000_u32;
        // 0 dBFS sine: amplitude = sqrt(2), but we clamp so use 0.99 RMS.
        let loud_rms = db_to_linear(-1.0);
        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        let chunk_size = 160;
        // Feed 500 ms of loud audio (attack = 10 ms, so 50× attack).
        for _ in 0..50 {
            let mut chunk = sine_wave(loud_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // Gain should be negative dB (attenuating the signal).
        let gain_db = proc.current_gain_db();
        assert!(
            gain_db < -1.0,
            "Expected AGC to attenuate loud input (<-1 dB), got {:.2} dB",
            gain_db
        );
    }

    /// 2.9: Gain is clamped to max_gain_db.
    #[test]
    fn agc_clamps_to_max_gain() {
        let sample_rate = 16000_u32;
        let cfg = AgcConfig {
            max_gain_db: 10.0,
            ..default_config()
        };
        let mut proc = AgcProcessor::new(cfg);

        // Feed very quiet audio for a long time.
        let chunk_size = 160;
        let near_silence_rms = db_to_linear(-60.0);
        for _ in 0..200 {
            let mut chunk = sine_wave(near_silence_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        let gain_db = proc.current_gain_db();
        assert!(
            gain_db <= 10.0 + 1e-3,
            "Gain {:.2} dB exceeds max_gain_db=10.0",
            gain_db
        );
    }

    /// 2.10: All-zero input does not produce NaN, infinity, or excessive gain.
    #[test]
    fn agc_silence_does_not_explode() {
        let sample_rate = 16000_u32;
        let cfg = default_config();
        let max_gain_db = cfg.max_gain_db;
        let mut proc = AgcProcessor::new(cfg);

        let chunk_size = 160;
        for _ in 0..500 {
            let mut chunk = vec![0.0f32; chunk_size];
            proc.process(&mut chunk, sample_rate);
        }

        let gain_db = proc.current_gain_db();
        assert!(
            gain_db.is_finite(),
            "gain_db should be finite on silence, got {}",
            gain_db
        );
        assert!(
            gain_db <= max_gain_db + 1e-3,
            "gain {:.2} dB exceeds max_gain_db={} on silence",
            gain_db,
            max_gain_db
        );
    }

    /// Verify output RMS is within 3 dB of target after convergence.
    #[test]
    fn agc_output_level_near_target() {
        let sample_rate = 16000_u32;
        let target_db = -18.0_f32;
        let input_rms = db_to_linear(-35.0); // Quiet input, 17 dB below target.
        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        let chunk_size = 160;
        // Warm up for 2 s to allow release envelope to converge.
        for _ in 0..200 {
            let mut chunk = sine_wave(input_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // Measure output RMS over the final 100 ms.
        let mut output: Vec<f32> = sine_wave(input_rms, chunk_size * 10, sample_rate);
        proc.process(&mut output, sample_rate);
        let out_rms_db = 20.0 * rms(&output).log10();

        assert!(
            (out_rms_db - target_db).abs() < 3.0,
            "Output RMS {:.1} dBFS not within 3 dB of target {:.1} dBFS",
            out_rms_db,
            target_db
        );
    }

    /// Noise below the gate threshold causes gain to decay toward unity.
    #[test]
    fn agc_gate_decays_gain_on_noise() {
        let sample_rate = 16000_u32;
        let chunk_size = 160; // 10 ms

        // gate_threshold_db = -50 → power = 1e-5
        // Use speech at -30 dBFS first to ramp gain, then switch to noise at -55 dBFS
        // (below the gate threshold).
        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        // Phase 1: Feed speech-level signal to establish a high gain.
        let speech_rms = db_to_linear(-30.0);
        for _ in 0..100 {
            let mut chunk = sine_wave(speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        let gain_after_speech = proc.current_gain_db();

        // Phase 2: Feed noise below gate threshold for 3 seconds.
        let noise_rms = db_to_linear(-55.0);
        for _ in 0..300 {
            let mut chunk = sine_wave(noise_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        let gain_after_noise = proc.current_gain_db();

        // Gain should have decayed substantially toward 0 dB (unity).
        // The envelope takes time to fall from speech-level power through the
        // gate threshold, so we allow some margin. The key assertion is that
        // gain has dropped significantly from its speech-time value.
        assert!(
            gain_after_noise < gain_after_speech - 5.0,
            "Expected gain to decay significantly, got {:.2} dB (was {:.2} dB after speech)",
            gain_after_noise,
            gain_after_speech
        );
        assert!(
            gain_after_noise.abs() < 5.0,
            "Expected gain near 0 dB after gate decay, got {:.2} dB",
            gain_after_noise
        );
    }

    /// Speech above the gate threshold still receives normal AGC processing.
    #[test]
    fn agc_gate_normal_processing_above_threshold() {
        let sample_rate = 16000_u32;
        let chunk_size = 160;

        // Quiet speech above the boost threshold still receives normal AGC processing.
        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        let quiet_speech_rms = db_to_linear(-35.0);
        for _ in 0..100 {
            let mut chunk = sine_wave(quiet_speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // AGC should boost this signal (gain > 10 dB).
        let gain_db = proc.current_gain_db();
        assert!(
            gain_db > 10.0,
            "Expected AGC to boost quiet speech above boost threshold (>10 dB), got {:.2} dB",
            gain_db
        );
    }

    /// Borderline noise above the gate threshold but below the boost threshold
    /// should not be driven to maximum gain.
    #[test]
    fn agc_does_not_boost_borderline_noise() {
        let sample_rate = 16000_u32;
        let chunk_size = 160;

        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        let borderline_noise_rms = db_to_linear(-45.0);
        for _ in 0..200 {
            let mut chunk = sine_wave(borderline_noise_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        let gain_db = proc.current_gain_db();
        assert!(
            gain_db < 6.0,
            "Expected borderline noise to stay near unity, got {:.2} dB",
            gain_db
        );
    }

    /// Smooth transition: gate decay back to active AGC on speech resumption.
    #[test]
    fn agc_gate_smooth_resumption() {
        let sample_rate = 16000_u32;
        let chunk_size = 160;

        let cfg = default_config();
        let mut proc = AgcProcessor::new(cfg);

        // Phase 1: Speech to establish AGC state.
        let speech_rms = db_to_linear(-30.0);
        for _ in 0..100 {
            let mut chunk = sine_wave(speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // Phase 2: Noise below gate for 2 seconds → gain decays toward unity.
        let noise_rms = db_to_linear(-55.0);
        for _ in 0..200 {
            let mut chunk = sine_wave(noise_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        let gain_before_resumption = proc.current_gain_db();

        // Phase 3: Speech resumes. Track gain over successive chunks to ensure
        // it moves smoothly (no jumps > 6 dB between consecutive chunks).
        let mut prev_gain = gain_before_resumption;
        for _ in 0..50 {
            let mut chunk = sine_wave(speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
            let current = proc.current_gain_db();
            let delta = (current - prev_gain).abs();
            assert!(
                delta < 6.0,
                "Gain jumped {:.2} dB between chunks (from {:.2} to {:.2}), expected smooth transition",
                delta,
                prev_gain,
                current
            );
            prev_gain = current;
        }
    }

    /// When gate_threshold_db is set very low, existing AGC behavior is unchanged.
    #[test]
    fn agc_gate_very_low_threshold_preserves_behavior() {
        let sample_rate = 16000_u32;
        let chunk_size = 160;

        // Set gate threshold extremely low so it never triggers.
        let cfg = AgcConfig {
            gate_threshold_db: -100.0,
            boost_threshold_db: -100.0,
            ..default_config()
        };
        let mut proc = AgcProcessor::new(cfg);

        // Feed noise at -55 dBFS — without gate, AGC should boost it.
        let noise_rms = db_to_linear(-55.0);
        for _ in 0..200 {
            let mut chunk = sine_wave(noise_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }

        // With gate_threshold_db = -100, all real signals are above the gate.
        // AGC should boost this signal significantly (gain > 20 dB).
        let gain_db = proc.current_gain_db();
        assert!(
            gain_db > 20.0,
            "Expected AGC to boost noise when gate is disabled (>20 dB), got {:.2} dB",
            gain_db
        );
    }

    /// Gate hold time prevents noise burst at segment start.
    #[test]
    fn agc_gate_hold_time_prevents_noise_burst() {
        let sample_rate = 16000_u32;
        let chunk_size = 160; // 10 ms chunks

        // Configure with 100ms hold time
        let cfg = AgcConfig {
            gate_hold_time_ms: 100.0,
            ..default_config()
        };
        let mut proc = AgcProcessor::new(cfg);

        // Phase 1: Noise in gate region (-55 dB) for 500ms → gain decays to unity
        let noise_rms = db_to_linear(-55.0);
        for _ in 0..50 {
            let mut chunk = sine_wave(noise_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        // Gain should be near unity after decay
        let gain_after_decay = proc.current_gain_db();
        assert!(
            gain_after_decay.abs() < 2.0,
            "Expected gain near unity after noise decay, got {:.2} dB",
            gain_after_decay
        );

        // Phase 2: Transition to speech level (-30 dB, above threshold but with hold time)
        // During hold time, gain should stay at unity (preventing noise burst)
        let speech_rms = db_to_linear(-30.0);
        for _ in 0..5 {
            // 50ms = 5 chunks of 10ms each, but hold time is 100ms
            let mut chunk = sine_wave(speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        // After 50ms (half of hold time), gain should still be near unity
        let gain_during_hold = proc.current_gain_db();
        assert!(
            gain_during_hold.abs() < 3.0,
            "Expected gain near unity during hold time, got {:.2} dB",
            gain_during_hold
        );

        // Phase 3: Continue speech through hold time (another 100ms)
        for _ in 0..10 {
            let mut chunk = sine_wave(speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        // After hold time expires, gain should rise to appropriate level
        let gain_after_hold = proc.current_gain_db();
        assert!(
            gain_after_hold > 5.0,
            "Expected gain to increase after hold time expires, got {:.2} dB",
            gain_after_hold
        );
    }

    /// Zero hold time restores legacy behavior (immediate gain application).
    #[test]
    fn agc_zero_hold_time_legacy_behavior() {
        let sample_rate = 16000_u32;
        let chunk_size = 160;

        // Configure with 0 hold time (legacy behavior)
        let cfg = AgcConfig {
            gate_hold_time_ms: 0.0,
            ..default_config()
        };
        let mut proc = AgcProcessor::new(cfg);

        // Phase 1: Noise in gate region for decay
        let noise_rms = db_to_linear(-55.0);
        for _ in 0..50 {
            let mut chunk = sine_wave(noise_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        let gain_after_decay = proc.current_gain_db();
        assert!(
            gain_after_decay.abs() < 2.0,
            "Expected gain near unity after noise decay, got {:.2} dB",
            gain_after_decay
        );

        // Phase 2: Immediate transition to speech - gain should apply right away
        let speech_rms = db_to_linear(-30.0);
        for _ in 0..5 {
            let mut chunk = sine_wave(speech_rms, chunk_size, sample_rate);
            proc.process(&mut chunk, sample_rate);
        }
        // With 0 hold time, gain should increase immediately (not stay at unity)
        let gain_immediate = proc.current_gain_db();
        assert!(
            gain_immediate > 5.0,
            "Expected immediate gain increase with 0 hold time, got {:.2} dB",
            gain_immediate
        );
    }
}
