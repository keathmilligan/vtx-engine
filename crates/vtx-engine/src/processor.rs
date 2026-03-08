//! Audio processing utilities for speech detection and visualization.
//!
//! This module contains the SpeechDetector and VisualizationProcessor which
//! analyze audio streams for speech activity and generate visualization data.

use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
use std::sync::Arc;

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
/// Uses multi-feature analysis for robust speech detection:
/// - RMS amplitude for basic energy detection
/// - Zero-Crossing Rate (ZCR) to distinguish voiced speech from transients
/// - Spectral centroid approximation to identify speech-band frequency content
///
/// Implements dual-mode detection:
/// - **Voiced mode**: For normal speech (lower ZCR, speech-band centroid)
/// - **Whisper mode**: For soft/breathy speech (higher ZCR, broader centroid range)
///
/// Explicit transient rejection filters keyboard clicks and similar impulsive sounds.
///
/// Includes lookback functionality to capture the true start of speech by maintaining
/// a ring buffer of recent audio samples and analyzing them retroactively.
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
    /// - Whisper mode: -52dB threshold, ZCR 0.08-0.45, centroid 300-7000Hz, 120ms onset
    /// - Transient rejection: ZCR > 0.45 AND centroid > 6500Hz
    /// - Hold time: 200ms (reduced from 300ms to detect sentence-end pauses from fast talkers)
    /// - Onset grace period: 30ms (brief dips in features don't reset onset counters)
    /// - Lookback buffer: 200ms (covers max onset time + margin)
    /// - Lookback threshold: -55dB (more sensitive to catch speech starts)
    /// - Word break min gap: 40ms (reduced from 80ms to catch fast-talker inter-word gaps)
    /// - Word break threshold ratio: 0.25 (25% of rolling average, tuned for dense speech)
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
                zcr_range: (0.08, 0.45),
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
    fn is_transient(&self, zcr: f32, centroid: f32) -> bool {
        zcr > self.transient_zcr_threshold && centroid > self.transient_centroid_threshold
    }

    /// Check if features match voiced speech mode
    fn matches_voiced_mode(&self, db: f32, zcr: f32, centroid: f32) -> bool {
        db >= self.voiced_config.threshold_db
            && zcr >= self.voiced_config.zcr_range.0
            && zcr <= self.voiced_config.zcr_range.1
            && centroid >= self.voiced_config.centroid_range.0
            && centroid <= self.voiced_config.centroid_range.1
    }

    /// Check if features match whisper speech mode
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
    fn get_recent_speech_amplitude(&self) -> f32 {
        if self.recent_speech_amplitude_count == 0 {
            return 0.0;
        }
        self.recent_speech_amplitude_sum / self.recent_speech_amplitude_count as f32
    }

    /// Reset word break detection state
    fn reset_word_break_state(&mut self) {
        self.in_word_break = false;
        self.word_break_sample_count = 0;
        self.word_break_start_speech_samples = 0;
        self.recent_speech_amplitude_sum = 0.0;
        self.recent_speech_amplitude_count = 0;
        self.last_is_word_break = false;
        self.last_word_break_event = None;
    }

    /// Process audio samples for speech detection
    pub fn process(&mut self, samples: &[f32]) {
        // Reset state change at start of each process call
        self.last_state_change = SpeechStateChange::None;
        self.last_word_break_event = None;

        // Add samples to lookback buffer
        self.push_to_lookback_buffer(samples);

        // Calculate features
        let rms = Self::calculate_rms(samples);
        let db = Self::amplitude_to_db(rms);
        let zcr = Self::calculate_zcr(samples);
        let centroid = self.estimate_spectral_centroid(samples, db);

        // Store metrics
        self.last_amplitude_db = db;
        self.last_zcr = zcr;
        self.last_centroid_hz = centroid;
        self.last_is_transient = self.is_transient(zcr, centroid);
        self.last_lookback_offset_ms = None;
        self.last_is_word_break = false;

        if !self.initialized {
            self.initialized = true;
            return;
        }

        // Transient rejection
        if self.last_is_transient {
            self.reset_onset_state();
            if !self.is_speaking {
                return;
            }
        }

        // Check feature matching
        let is_voiced = self.matches_voiced_mode(db, zcr, centroid);
        let is_whisper = self.matches_whisper_mode(db, zcr, centroid);
        let is_speech_candidate = is_voiced || is_whisper;

        let samples_len = samples.len() as u32;

        if is_speech_candidate {
            self.silence_sample_count = 0;

            if self.is_speaking {
                self.speech_sample_count += samples.len() as u64;
                self.update_speech_amplitude_average(rms, samples_len);

                // Check if word break ended
                if self.in_word_break {
                    if self.word_break_sample_count >= self.min_word_break_samples
                        && self.word_break_sample_count <= self.max_word_break_samples
                    {
                        let gap_duration_ms =
                            self.samples_to_ms(self.word_break_sample_count as u64) as u32;
                        let offset_ms =
                            self.samples_to_ms(self.word_break_start_speech_samples) as u32;

                        let payload = WordBreakPayload {
                            offset_ms,
                            gap_duration_ms,
                        };

                        if let Some(ref callback) = self.callback {
                            callback.on_word_break(payload);
                        }

                        self.last_word_break_event = Some(WordBreakEvent {
                            offset_ms,
                            gap_duration_ms,
                        });

                        tracing::debug!(
                            "Word break detected (offset: {}ms, gap: {}ms)",
                            offset_ms,
                            gap_duration_ms
                        );
                    }
                    self.in_word_break = false;
                    self.word_break_sample_count = 0;
                }
            } else {
                // Handle onset accumulation
                if is_voiced {
                    self.voiced_grace_count = 0;
                    if !self.is_pending_voiced {
                        self.is_pending_voiced = true;
                        self.voiced_onset_count = samples_len;
                    } else {
                        self.voiced_onset_count += samples_len;
                    }

                    if self.voiced_onset_count >= self.voiced_config.onset_samples {
                        self.is_speaking = true;
                        self.speech_sample_count = self.voiced_onset_count as u64;
                        self.reset_onset_state();

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
                            "Speech started (voiced mode, lookback: {}ms)",
                            lookback_offset_ms
                        );
                        return;
                    }
                }

                if is_whisper {
                    self.whisper_grace_count = 0;
                    if !self.is_pending_whisper {
                        self.is_pending_whisper = true;
                        self.whisper_onset_count = samples_len;
                    } else {
                        self.whisper_onset_count += samples_len;
                    }

                    if !self.is_speaking
                        && self.whisper_onset_count >= self.whisper_config.onset_samples
                    {
                        self.is_speaking = true;
                        self.speech_sample_count = self.whisper_onset_count as u64;
                        self.reset_onset_state();

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
                            "Speech started (whisper mode, lookback: {}ms)",
                            lookback_offset_ms
                        );
                    }
                }
            }
        } else {
            // Grace period handling
            if self.is_pending_voiced {
                self.voiced_grace_count += samples_len;
                if self.voiced_grace_count >= self.onset_grace_samples {
                    self.is_pending_voiced = false;
                    self.voiced_onset_count = 0;
                    self.voiced_grace_count = 0;
                }
            }

            if self.is_pending_whisper {
                self.whisper_grace_count += samples_len;
                if self.whisper_grace_count >= self.onset_grace_samples {
                    self.is_pending_whisper = false;
                    self.whisper_onset_count = 0;
                    self.whisper_grace_count = 0;
                }
            }

            if self.is_speaking {
                self.silence_sample_count += samples_len;

                // Word break detection
                let recent_avg = self.get_recent_speech_amplitude();
                let threshold = recent_avg * self.word_break_threshold_ratio;

                if recent_avg > 0.0 && rms < threshold {
                    if !self.in_word_break {
                        self.in_word_break = true;
                        self.word_break_sample_count = samples_len;
                        self.word_break_start_speech_samples = self.speech_sample_count;
                    } else {
                        self.word_break_sample_count += samples_len;
                    }

                    if self.word_break_sample_count >= self.min_word_break_samples
                        && self.word_break_sample_count <= self.max_word_break_samples
                    {
                        self.last_is_word_break = true;
                    }
                }

                // Check hold time
                if self.silence_sample_count >= self.hold_samples {
                    let duration_ms = self.samples_to_ms(self.speech_sample_count);
                    self.is_speaking = false;
                    self.speech_sample_count = 0;
                    self.reset_word_break_state();

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
