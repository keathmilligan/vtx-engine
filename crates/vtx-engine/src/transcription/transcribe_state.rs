//! Transcription state management with continuous recording and speech-based segmentation.
//!
//! This module provides:
//! - `SegmentRingBuffer`: A ring buffer for continuous audio capture (VAD mode)
//! - `TranscribeState`: State management for both VAD-driven and manual recording modes

use std::sync::Arc;

use crate::audio::{generate_recording_filename, save_to_wav};

use super::queue::{QueuedSegment, TranscriptionQueue};

/// Ring buffer capacity: 30 seconds at 48kHz stereo
/// 48000 * 30 * 2 = 2,880,000 samples
const RING_BUFFER_CAPACITY: usize = 48000 * 30 * 2;

/// Maximum manual recording buffer duration: 30 minutes at 48kHz mono.
/// 48000 * 60 * 30 = 86,400,000 samples (~330 MB of f32).
/// Audio beyond this limit is silently dropped and a warning is emitted once.
const MANUAL_MAX_BUFFER_SAMPLES: usize = 48000 * 60 * 30;

/// Maximum processed (mono) recording buffer duration: 30 minutes at 16kHz mono.
/// 16000 * 60 * 30 = 28,800,000 samples (~110 MB of f32).
/// Sized for the 16kHz mono signal that reaches the transcription engine.
const PROCESSED_MAX_BUFFER_SAMPLES: usize = 48000 * 60 * 30;

/// Overflow threshold: 90% of buffer capacity
const OVERFLOW_THRESHOLD_PERCENT: usize = 90;

/// Maximum segment duration before seeking word break (force-split safety net)
const MAX_SEGMENT_DURATION_MS: u64 = 4000;

/// Grace period after duration threshold before forcing segment submission
const WORD_BREAK_GRACE_MS: u64 = 750;

/// Minimum segment duration before word-break events are allowed to split the segment.
/// Below this threshold, word breaks are ignored so that short utterances and individual
/// words are not fragmented. 2000ms represents a comfortable minimum for a short sentence.
const WORD_BREAK_ACTIVATION_MS: u64 = 2000;

/// Minimum segment duration to submit for transcription
/// Segments shorter than this are likely to produce [BLANK_AUDIO] from Whisper
const MIN_SEGMENT_DURATION_MS: u64 = 500;

/// Minimum RMS amplitude threshold for non-silent audio (linear scale)
/// Approximately -40dB
const MIN_AUDIO_RMS_THRESHOLD: f32 = 0.01;

/// Safety margin before word break point (ms) - ensures we don't cut into the end of speech
/// The extraction point will be (gap_start - margin) rather than gap_midpoint
const WORD_BREAK_PRE_MARGIN_MS: u64 = 30;

// ============================================================================
// Segment Ring Buffer
// ============================================================================

/// A ring buffer for continuous audio capture during transcribe mode.
///
/// Provides continuous write without blocking, and segment extraction by copying
/// samples between indices. Handles wraparound correctly.
pub struct SegmentRingBuffer {
    /// The underlying buffer
    buffer: Vec<f32>,
    /// Current write position
    write_pos: usize,
    /// Capacity of the buffer
    capacity: usize,
    /// Total samples written (for tracking)
    total_written: u64,
}

impl SegmentRingBuffer {
    /// Create a new ring buffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity],
            write_pos: 0,
            capacity,
            total_written: 0,
        }
    }

    /// Create a ring buffer with default capacity (30 seconds at 48kHz stereo)
    pub fn with_default_capacity() -> Self {
        Self::new(RING_BUFFER_CAPACITY)
    }

    /// Write samples to the buffer, advancing write position and wrapping
    pub fn write(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.buffer[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.capacity;
            self.total_written += 1;
        }
    }

    /// Get current write position
    pub fn write_position(&self) -> usize {
        self.write_pos
    }

    /// Get buffer capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Calculate segment length from start_idx to current write_pos, handling wraparound
    pub fn segment_length(&self, start_idx: usize) -> usize {
        if self.write_pos >= start_idx {
            self.write_pos - start_idx
        } else {
            // Wraparound case: distance from start to end + distance from 0 to write_pos
            (self.capacity - start_idx) + self.write_pos
        }
    }

    /// Calculate a sample index from lookback offset (samples back from write_pos)
    pub fn index_from_lookback(&self, lookback_samples: usize) -> usize {
        if lookback_samples >= self.capacity {
            // Clamp to buffer size
            self.write_pos
        } else if lookback_samples <= self.write_pos {
            self.write_pos - lookback_samples
        } else {
            // Wraparound case
            self.capacity - (lookback_samples - self.write_pos)
        }
    }

    /// Check if segment length exceeds overflow threshold
    pub fn is_approaching_overflow(&self, start_idx: usize) -> bool {
        let segment_len = self.segment_length(start_idx);
        let threshold = (self.capacity * OVERFLOW_THRESHOLD_PERCENT) / 100;
        segment_len >= threshold
    }

    /// Extract segment from start_idx to current write_pos, handling wraparound
    /// Returns a new Vec with the copied samples
    pub fn extract_segment(&self, start_idx: usize) -> Vec<f32> {
        self.extract_segment_to(start_idx, self.write_pos)
    }

    /// Extract segment from start_idx to a specific end_idx, handling wraparound
    /// Returns a new Vec with the copied samples
    pub fn extract_segment_to(&self, start_idx: usize, end_idx: usize) -> Vec<f32> {
        // Calculate segment length handling wraparound
        let segment_len = if end_idx >= start_idx {
            end_idx - start_idx
        } else {
            (self.capacity - start_idx) + end_idx
        };

        if segment_len == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(segment_len);

        if end_idx >= start_idx {
            // No wraparound: simple slice copy
            result.extend_from_slice(&self.buffer[start_idx..end_idx]);
        } else {
            // Wraparound: copy from start_idx to end, then from 0 to end_idx
            result.extend_from_slice(&self.buffer[start_idx..]);
            result.extend_from_slice(&self.buffer[..end_idx]);
        }

        result
    }

    /// Clear the buffer (reset write position but don't zero memory)
    pub fn clear(&mut self) {
        self.write_pos = 0;
        self.total_written = 0;
    }
}

// ============================================================================
// Transcribe State Callback
// ============================================================================

/// Callback trait for transcribe state events.
///
/// Implement this trait to receive notifications about segment processing.
pub trait TranscribeStateCallback: Send + Sync + 'static {
    /// Called when a recording segment is saved to disk.
    fn on_recording_saved(&self, path: String);

    /// Called when a queue update occurs.
    fn on_queue_update(&self, depth: usize);
}

// ============================================================================
// Transcribe State
// ============================================================================

/// State for automatic transcription mode.
///
/// Manages the ring buffer, tracks speech segments, and coordinates
/// with the transcription queue. Supports timed segment submission
/// that breaks at word boundaries after a maximum duration.
pub struct TranscribeState {
    /// Ring buffer for continuous audio capture
    pub ring_buffer: SegmentRingBuffer,
    /// Whether transcribe mode is active
    pub is_active: bool,
    /// Whether we're currently inside a speech segment
    pub in_speech: bool,
    /// Ring buffer index where current speech segment started
    pub segment_start_idx: usize,
    /// Sample rate for the capture
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
    /// Reference to the transcription queue
    pub transcription_queue: Arc<TranscriptionQueue>,
    /// Number of samples in the current segment (for duration tracking)
    /// This counts samples since speech was confirmed, NOT including lookback
    segment_sample_count: u64,
    /// Whether we're seeking a word break (duration threshold exceeded)
    seeking_word_break: bool,
    /// Sample count when we started seeking word break (for grace period)
    word_break_seek_start_samples: u64,
    /// Number of lookback samples at the start of the current segment
    lookback_sample_count: usize,
    /// Cumulative VAD time (ms) consumed by word-break splits within this speech utterance.
    /// The VAD's speech_sample_count is never reset between splits, so offset_ms values are
    /// always relative to the original speech-confirmed start. This field tracks how much of
    /// that cumulative time has already been extracted, so each word-break offset can be
    /// made relative to the current segment start.
    vad_offset_base_ms: u64,
    /// Callback for state events
    callback: Option<Arc<dyn TranscribeStateCallback>>,
    /// Manual recording mode - disables automatic VAD segmentation.
    /// When true, audio is buffered until `submit_recording()` is called.
    pub manual_recording: bool,
    /// Growable audio accumulator for manual recording sessions.
    ///
    /// In manual recording mode audio is appended here instead of relying on
    /// the ring buffer, so the full session is preserved without wraparound
    /// loss. Capped at `MANUAL_MAX_BUFFER_SAMPLES` (30 minutes at 48kHz mono).
    /// Contains **raw** (pre-gain, pre-AGC) mono PCM: the multi-channel
    /// hardware input mixed down to mono but with no software processing
    /// applied.  Used only for the raw WAV save.
    manual_audio_buffer: Vec<f32>,
    /// Set to true after the first time the manual buffer hits the 30-minute cap,
    /// so the overflow warning is emitted only once per session.
    manual_buffer_full_warned: bool,
    /// Growable accumulator for gain/AGC-processed **mono** audio during a manual
    /// recording session. Mirrors `manual_audio_buffer` but holds the signal after
    /// all software processing stages (mic gain + AGC, channel mix-down to mono).
    ///
    /// This buffer is used for:
    ///  1. The "processed" WAV file saved alongside each recording.
    ///  2. The audio submitted to the transcription engine when the recording ends.
    ///
    /// Capped at `PROCESSED_MAX_BUFFER_SAMPLES` (30 minutes at 48kHz mono).
    processed_audio_buffer: Vec<f32>,
    /// Set to true after the first time the processed buffer hits the cap,
    /// so the overflow warning is emitted only once per session.
    processed_buffer_full_warned: bool,
    /// When set, indicates this recording session is a playback reprocessing
    /// of an existing recording. Holds the original file's stem (e.g.,
    /// `"vtx-20260308-143022"`) so that the processed WAV overwrites the
    /// original `-processed.wav` rather than creating a new timestamped file.
    /// The raw WAV is left untouched during playback reprocessing.
    playback_source_stem: Option<String>,
}

impl TranscribeState {
    /// Create a new transcribe state
    pub fn new(transcription_queue: Arc<TranscriptionQueue>) -> Self {
        Self {
            ring_buffer: SegmentRingBuffer::with_default_capacity(),
            is_active: false,
            in_speech: false,
            segment_start_idx: 0,
            sample_rate: 48000,
            channels: 2,
            transcription_queue,
            segment_sample_count: 0,
            seeking_word_break: false,
            word_break_seek_start_samples: 0,
            lookback_sample_count: 0,
            vad_offset_base_ms: 0,
            callback: None,
            manual_recording: false,
            manual_audio_buffer: Vec::new(),
            manual_buffer_full_warned: false,
            processed_audio_buffer: Vec::new(),
            processed_buffer_full_warned: false,
            playback_source_stem: None,
        }
    }

    /// Set the playback source stem for reprocessing an existing recording.
    ///
    /// When set, `submit_recording` and `save_recording_wav` will overwrite
    /// the existing `-processed.wav` for this stem instead of generating a
    /// new timestamped file, and will skip writing a new raw WAV (the
    /// original raw file is left untouched).
    pub fn set_playback_source_stem(&mut self, stem: Option<String>) {
        if let Some(ref s) = stem {
            tracing::info!(
                "[TranscribeState] Playback reprocessing mode: stem = {:?}",
                s
            );
        }
        self.playback_source_stem = stem;
    }

    /// Enable or disable manual recording mode.
    ///
    /// In manual recording mode all VAD-driven segmentation is suppressed —
    /// `on_speech_started`, `on_speech_ended`, and `on_word_break` are all
    /// ignored.  Audio is written continuously to a growable buffer and
    /// submitted only when `submit_recording()` is called explicitly.
    pub fn set_manual_recording(&mut self, enabled: bool) {
        self.manual_recording = enabled;
        if enabled {
            tracing::debug!(
                "[TranscribeState] Manual recording enabled - VAD segmentation disabled"
            );
        } else {
            tracing::debug!(
                "[TranscribeState] Manual recording disabled - automatic VAD segmentation active"
            );
        }
    }

    /// Save the accumulated manual recording buffers to WAV files and fire the
    /// `on_recording_saved` callback, but do **not** enqueue for transcription.
    ///
    /// Saves two files sharing the same timestamp stem:
    /// - `vtx-<timestamp>.wav` — raw (pre-gain, pre-AGC) multi-channel audio as
    ///   received from the hardware backend.
    /// - `vtx-<timestamp>-processed.wav` — gain/AGC-processed mono audio that
    ///   matches what the transcription engine would receive.
    ///
    /// Used in auto-transcription mode where the VAD has already handled
    /// real-time segmentation.  Both WAVs are saved so the session appears
    /// as the active document and can be reprocessed.
    ///
    /// When `playback_source_stem` is set (playback reprocessing), the original
    /// raw WAV is left untouched and only the `-processed.wav` is overwritten
    /// using the original stem.
    pub fn save_recording_wav(&mut self) {
        let raw_samples: Vec<f32> = std::mem::take(&mut self.manual_audio_buffer);
        self.manual_buffer_full_warned = false;
        let processed_samples: Vec<f32> = std::mem::take(&mut self.processed_audio_buffer);
        self.processed_buffer_full_warned = false;

        if raw_samples.is_empty() {
            tracing::debug!("[TranscribeState] save_recording_wav: no audio accumulated");
            return;
        }

        tracing::info!(
            "[TranscribeState] save_recording_wav: saving {} raw samples ({:.1}s)",
            raw_samples.len(),
            raw_samples.len() as f64 / (self.sample_rate as f64 * self.channels as f64),
        );

        let is_playback_reprocess = self.playback_source_stem.is_some();
        let recordings_dir = crate::audio::recordings_dir();
        if let Err(e) = std::fs::create_dir_all(&recordings_dir) {
            tracing::error!(
                "[TranscribeState] Failed to create recordings directory: {}",
                e
            );
            return;
        }

        // Reuse the original stem during playback reprocessing; otherwise
        // generate a fresh timestamp.
        let stem = self
            .playback_source_stem
            .clone()
            .unwrap_or_else(crate::audio::generate_recording_stem);

        // --- Raw WAV (mono, pre-gain) ---
        // Skip during playback reprocessing to preserve the original recording.
        let raw_path = recordings_dir.join(format!("{}.wav", stem));
        if !is_playback_reprocess {
            match save_to_wav(&raw_samples, self.sample_rate, 1, &raw_path) {
                Ok(()) => {
                    tracing::info!(
                        "[TranscribeState] Saved raw recording WAV to: {:?}",
                        raw_path
                    );
                    // Raw WAV is saved silently — do not update last_recording_path yet.
                    // If no processed WAV follows, fire the callback with the raw path as fallback.
                }
                Err(e) => {
                    tracing::error!("[TranscribeState] Failed to save raw recording WAV: {}", e);
                }
            }
        } else {
            tracing::info!(
                "[TranscribeState] Playback reprocessing — skipping raw WAV write for {:?}",
                raw_path
            );
        }

        // --- Processed WAV ---
        // Fire on_recording_saved with the processed path so the demo opens the
        // gain/AGC-adjusted mono file (what the transcription engine sees).
        // If processed samples are absent (e.g. AGC/gain never enabled) fall back
        // to the raw path so the callback always fires exactly once per session.
        if !processed_samples.is_empty() {
            let proc_filename = format!("{}-processed.wav", stem);
            let proc_path = recordings_dir.join(&proc_filename);
            match save_to_wav(&processed_samples, self.sample_rate, 1, &proc_path) {
                Ok(()) => {
                    tracing::info!(
                        "[TranscribeState] Saved processed recording WAV to: {:?}",
                        proc_path
                    );
                    if let Some(ref cb) = self.callback {
                        cb.on_recording_saved(proc_path.to_string_lossy().to_string());
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "[TranscribeState] Failed to save processed recording WAV: {}",
                        e
                    );
                    // Fall back to raw path so the session document is still set.
                    if let Some(ref cb) = self.callback {
                        cb.on_recording_saved(raw_path.to_string_lossy().to_string());
                    }
                }
            }
        } else {
            // No processed audio accumulated — fire callback with raw path.
            if let Some(ref cb) = self.callback {
                cb.on_recording_saved(raw_path.to_string_lossy().to_string());
            }
        }
    }

    /// Submit the entire accumulated manual recording audio for transcription.
    ///
    /// Saves the raw audio to a WAV file, saves the processed (gain/AGC-applied
    /// mono) audio to a second WAV file, and enqueues the **processed** audio for
    /// transcription. The ring buffer is not consulted here — all manual recording
    /// audio lives in the manual buffers.
    ///
    /// Intended for use at the end of a manual recording session (e.g., when
    /// the app calls `stop_recording()`).
    pub fn submit_recording(&mut self) {
        if !self.is_active {
            return;
        }

        // Drain both buffers atomically so the struct is immediately ready for
        // the next session.
        let raw_segment: Vec<f32> = std::mem::take(&mut self.manual_audio_buffer);
        self.manual_buffer_full_warned = false;
        let processed_segment: Vec<f32> = std::mem::take(&mut self.processed_audio_buffer);
        self.processed_buffer_full_warned = false;

        // Determine which buffer to submit for transcription.
        // Prefer processed (gain/AGC-applied mono); fall back to raw if processed
        // is empty (e.g., AGC was never enabled and no gain was applied but the
        // buffer wasn't populated — defensive).
        let (transcription_segment, transcription_channels) = if !processed_segment.is_empty() {
            (processed_segment.clone(), 1u16)
        } else if !raw_segment.is_empty() {
            tracing::warn!(
                "[TranscribeState] submit_recording: processed buffer is empty, \
                 falling back to raw audio for transcription"
            );
            (raw_segment.clone(), self.channels)
        } else {
            tracing::debug!("[TranscribeState] submit_recording: no audio accumulated");
            return;
        };

        tracing::info!(
            "[TranscribeState] submit_recording: submitting {} processed samples ({:.1}s)",
            transcription_segment.len(),
            transcription_segment.len() as f64 / self.sample_rate as f64,
        );

        // Save WAV files. When reprocessing a playback (playback_source_stem is
        // set), reuse the original stem and only overwrite the processed WAV —
        // the raw recording is left untouched. For fresh recordings, save both
        // raw and processed WAVs with a new timestamp.
        let mut proc_wav_path: Option<std::path::PathBuf> = None;
        let is_playback_reprocess = self.playback_source_stem.is_some();
        let recordings_dir = crate::audio::recordings_dir();
        if let Ok(()) = std::fs::create_dir_all(&recordings_dir) {
            let stem = self
                .playback_source_stem
                .clone()
                .unwrap_or_else(crate::audio::generate_recording_stem);

            // Raw WAV (mono, pre-gain) — skip during playback reprocessing
            // to preserve the original untouched raw recording.
            if !is_playback_reprocess && !raw_segment.is_empty() {
                let raw_path = recordings_dir.join(format!("{}.wav", stem));
                match save_to_wav(&raw_segment, self.sample_rate, 1, &raw_path) {
                    Ok(()) => {
                        tracing::info!("[TranscribeState] Saved raw WAV: {:?}", raw_path);
                    }
                    Err(e) => {
                        tracing::error!("[TranscribeState] Failed to save raw WAV: {}", e);
                    }
                }
            }

            // Processed WAV (overwrites existing -processed.wav during playback)
            if !processed_segment.is_empty() {
                let proc_path = recordings_dir.join(format!("{}-processed.wav", stem));
                match save_to_wav(&processed_segment, self.sample_rate, 1, &proc_path) {
                    Ok(()) => {
                        tracing::info!("[TranscribeState] Saved processed WAV: {:?}", proc_path);
                        proc_wav_path = Some(proc_path.clone());
                        if let Some(ref cb) = self.callback {
                            cb.on_recording_saved(proc_path.to_string_lossy().to_string());
                        }
                    }
                    Err(e) => {
                        tracing::error!("[TranscribeState] Failed to save processed WAV: {}", e);
                    }
                }
            }
        } else {
            tracing::error!("[TranscribeState] Failed to create recordings directory");
        }

        // Reset session state
        self.in_speech = false;
        self.segment_sample_count = 0;
        self.seeking_word_break = false;
        self.lookback_sample_count = 0;
        self.segment_start_idx = self.ring_buffer.write_position();

        // Enqueue the processed segment for transcription, reusing the
        // already-saved processed WAV path.  Using enqueue_for_transcription
        // (not queue_segment_with_channels) avoids writing a third WAV file
        // whose `vtx-{timestamp}.wav` name would collide with and overwrite
        // the raw recording.
        self.enqueue_for_transcription(
            transcription_segment,
            transcription_channels,
            proc_wav_path,
        );
    }

    /// Set the callback for state events.
    pub fn set_callback(&mut self, callback: Arc<dyn TranscribeStateCallback>) {
        self.callback = Some(callback);
    }

    /// Clear the callback.
    pub fn clear_callback(&mut self) {
        self.callback = None;
    }

    /// Initialize for capture with specified parameters
    pub fn init_for_capture(&mut self, sample_rate: u32, channels: u16) {
        self.sample_rate = sample_rate;
        self.channels = channels;
        self.ring_buffer.clear();
        self.in_speech = false;
        self.segment_start_idx = 0;
        self.segment_sample_count = 0;
        self.seeking_word_break = false;
        self.word_break_seek_start_samples = 0;
        self.lookback_sample_count = 0;
        self.vad_offset_base_ms = 0;
        self.manual_audio_buffer.clear();
        self.manual_buffer_full_warned = false;
        self.processed_audio_buffer.clear();
        self.processed_buffer_full_warned = false;
    }

    /// Activate transcribe mode
    pub fn activate(&mut self) {
        self.is_active = true;
        self.in_speech = false;
        self.segment_start_idx = 0;
        self.segment_sample_count = 0;
        self.seeking_word_break = false;
        self.word_break_seek_start_samples = 0;
        self.lookback_sample_count = 0;
        self.vad_offset_base_ms = 0;
    }

    /// Deactivate transcribe mode
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.in_speech = false;
        self.seeking_word_break = false;
    }

    /// Append audio to the manual recording buffer (WAV accumulation) without
    /// affecting the ring buffer or VAD segmentation state.
    ///
    /// This is called during recording sessions regardless of transcription mode
    /// so that the captured audio is always saved to a WAV file.  Audio beyond
    /// the 30-minute cap is silently dropped (one warning emitted).
    pub fn write_manual_buffer(&mut self, samples: &[f32]) {
        let remaining_capacity =
            MANUAL_MAX_BUFFER_SAMPLES.saturating_sub(self.manual_audio_buffer.len());

        if remaining_capacity == 0 {
            if !self.manual_buffer_full_warned {
                tracing::warn!(
                    "[TranscribeState] Manual recording has reached the 30-minute maximum \
                     buffer limit. Further audio will be discarded."
                );
                self.manual_buffer_full_warned = true;
            }
            return;
        }

        let samples_to_write = samples.len().min(remaining_capacity);
        self.manual_audio_buffer
            .extend_from_slice(&samples[..samples_to_write]);

        if samples_to_write < samples.len() && !self.manual_buffer_full_warned {
            tracing::warn!(
                "[TranscribeState] Manual recording has reached the 30-minute maximum \
                 buffer limit. Further audio will be discarded."
            );
            self.manual_buffer_full_warned = true;
        }
    }

    /// Append gain/AGC-processed **mono** audio to the processed recording buffer.
    ///
    /// This mirrors `write_manual_buffer` but operates on the post-processing
    /// signal (after `mic_gain_db` and AGC have been applied, and channels have
    /// been mixed down to mono). The processed buffer is used for:
    ///  1. The `*-processed.wav` file written at the end of a recording session.
    ///  2. The audio submitted to the Whisper transcription engine in PTT mode.
    ///
    /// Audio beyond the `PROCESSED_MAX_BUFFER_SAMPLES` cap is silently dropped
    /// (one warning emitted per session).
    pub fn write_processed_buffer(&mut self, mono_samples: &[f32]) {
        let remaining_capacity =
            PROCESSED_MAX_BUFFER_SAMPLES.saturating_sub(self.processed_audio_buffer.len());

        if remaining_capacity == 0 {
            if !self.processed_buffer_full_warned {
                tracing::warn!(
                    "[TranscribeState] Processed recording buffer has reached the 30-minute \
                     maximum. Further processed audio will be discarded."
                );
                self.processed_buffer_full_warned = true;
            }
            return;
        }

        let samples_to_write = mono_samples.len().min(remaining_capacity);
        self.processed_audio_buffer
            .extend_from_slice(&mono_samples[..samples_to_write]);

        if samples_to_write < mono_samples.len() && !self.processed_buffer_full_warned {
            tracing::warn!(
                "[TranscribeState] Processed recording buffer has reached the 30-minute \
                 maximum. Further processed audio will be discarded."
            );
            self.processed_buffer_full_warned = true;
        }
    }

    /// Process incoming audio samples - writes to ring buffer and checks for overflow/duration
    /// Returns Some(segment) if overflow extraction or grace period extraction occurred
    pub fn process_samples(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
        if !self.is_active {
            return None;
        }

        // Automatic mode: Check for overflow before writing (if in speech)
        let overflow_segment = if self.in_speech
            && self
                .ring_buffer
                .is_approaching_overflow(self.segment_start_idx)
        {
            // Extract current segment before it gets overwritten
            let segment = self.ring_buffer.extract_segment(self.segment_start_idx);

            // Update segment start to current write position
            self.segment_start_idx = self.ring_buffer.write_position();
            self.segment_sample_count = 0;
            self.seeking_word_break = false;
            self.lookback_sample_count = 0; // No lookback for continuation segments

            // Remain in speech state
            tracing::debug!(
                "[TranscribeState] Buffer overflow - extracted partial segment ({} samples)",
                segment.len()
            );

            Some(segment)
        } else {
            None
        };

        // Write samples to ring buffer (always happens)
        self.ring_buffer.write(samples);

        // Track segment duration if in speech
        if self.in_speech {
            self.segment_sample_count += samples.len() as u64;

            // Check if we've exceeded max duration and should start seeking word break
            let duration_ms = self.samples_to_ms(self.segment_sample_count);
            if !self.seeking_word_break && duration_ms >= MAX_SEGMENT_DURATION_MS {
                self.seeking_word_break = true;
                self.word_break_seek_start_samples = self.segment_sample_count;
                tracing::debug!(
                    "[TranscribeState] Duration threshold reached ({}ms), seeking word break",
                    duration_ms
                );
            }

            // Check for grace period expiration if seeking word break
            if self.seeking_word_break {
                let samples_since_seek =
                    self.segment_sample_count - self.word_break_seek_start_samples;
                let grace_ms = self.samples_to_ms(samples_since_seek);

                if grace_ms >= WORD_BREAK_GRACE_MS {
                    // Grace period expired, force extraction
                    let forced = self.force_segment_extraction();
                    if forced.is_some() {
                        return forced;
                    }
                }
            }
        }

        // If we extracted a segment due to overflow, queue it.
        // Ring buffer now contains mono processed audio (channels=1).
        if let Some(segment) = overflow_segment.clone() {
            self.queue_segment_with_channels(segment, 1);
        }

        overflow_segment
    }

    /// Handle speech-started event: mark segment start including lookback
    ///
    /// Note: `lookback_samples` from the speech detector is in MONO samples (frames).
    /// We need to convert to stereo samples for the ring buffer which stores interleaved stereo.
    pub fn on_speech_started(&mut self, lookback_samples: usize) {
        if !self.is_active {
            return;
        }

        // Convert mono lookback samples to stereo samples for ring buffer
        let lookback_stereo_samples = lookback_samples * self.channels as usize;

        self.in_speech = true;
        self.segment_start_idx = self
            .ring_buffer
            .index_from_lookback(lookback_stereo_samples);
        // Start duration tracking from zero (lookback samples are pre-speech)
        self.segment_sample_count = 0;
        self.seeking_word_break = false;
        // Remember lookback count (in stereo samples) for proper word break extraction
        self.lookback_sample_count = lookback_stereo_samples;
        // Reset the VAD offset base: this is a fresh utterance so cumulative VAD time starts at 0
        self.vad_offset_base_ms = 0;
        tracing::debug!(
            "[TranscribeState] Speech started, segment_start_idx={}, lookback={} mono -> {} stereo",
            self.segment_start_idx,
            lookback_samples,
            lookback_stereo_samples
        );
    }

    /// Handle speech-ended event: extract segment and queue for transcription
    pub fn on_speech_ended(&mut self) -> Option<Vec<f32>> {
        if !self.is_active || !self.in_speech {
            return None;
        }

        // Extract the segment
        let segment = self.ring_buffer.extract_segment(self.segment_start_idx);

        self.in_speech = false;
        self.segment_sample_count = 0;
        self.seeking_word_break = false;
        self.lookback_sample_count = 0;

        if segment.is_empty() {
            tracing::debug!("[TranscribeState] Speech ended but segment is empty");
            return None;
        }

        tracing::debug!(
            "[TranscribeState] Speech ended, extracted {} samples",
            segment.len()
        );

        // Queue the segment for transcription (will validate before actually queueing).
        // Ring buffer contains mono processed audio (channels=1).
        self.queue_segment_with_channels(segment.clone(), 1);

        Some(segment)
    }

    /// Convert sample count to milliseconds.
    /// Note: sample count here is raw samples (includes all channels),
    /// so we divide by channels to get frames, then convert to ms.
    fn samples_to_ms(&self, samples: u64) -> u64 {
        let frames = samples / self.channels as u64;
        frames * 1000 / self.sample_rate as u64
    }

    /// Convert milliseconds to sample count.
    /// Note: Returns raw sample count (includes all channels),
    /// so we multiply frames by channels.
    fn ms_to_samples(&self, ms: u64) -> u64 {
        let frames = ms * self.sample_rate as u64 / 1000;
        frames * self.channels as u64
    }

    /// Handle word-break event: extract and submit the current segment at the word break point,
    /// provided the segment is long enough to be valid.
    ///
    /// Parameters:
    /// - `offset_ms`: Offset from speech start where the word break gap started (from speech detector)
    /// - `gap_duration_ms`: Duration of the gap in milliseconds
    ///
    /// Returns Some(segment) if extraction occurred
    ///
    /// Note: We extract at the START of the gap (minus a small margin) rather than the midpoint.
    /// This ensures we capture all speech before the pause and don't accidentally cut into
    /// the end of a word. The next segment will naturally start from this point.
    ///
    /// Word breaks are acted on at any point during active speech (not only after the
    /// MAX_SEGMENT_DURATION_MS threshold) so that fast talkers whose inter-sentence pauses
    /// are shorter than hold_samples still get clean segment boundaries.
    pub fn on_word_break(&mut self, offset_ms: u32, gap_duration_ms: u32) -> Option<Vec<f32>> {
        if !self.is_active || !self.in_speech {
            return None;
        }

        // The VAD's offset_ms is cumulative from the original speech-confirmed start and is
        // never reset between word-break splits. Subtract vad_offset_base_ms to get the
        // offset relative to the start of the current (post-split) segment.
        let relative_offset_ms = (offset_ms as u64).saturating_sub(self.vad_offset_base_ms);

        // Only split at word breaks once enough speech has accumulated in the current segment
        // to represent a complete phrase or sentence.
        let current_duration_ms = self.samples_to_ms(self.segment_sample_count);
        if current_duration_ms < WORD_BREAK_ACTIVATION_MS {
            tracing::debug!(
                "[TranscribeState] Word break at vad_offset {}ms (relative {}ms) ignored (segment only {}ms < {}ms activation threshold)",
                offset_ms,
                relative_offset_ms,
                current_duration_ms,
                WORD_BREAK_ACTIVATION_MS
            );
            return None;
        }

        // The word break offset_ms is from when speech was confirmed (not including lookback)
        // Our segment includes lookback samples at the start
        // So we need to add lookback to get the correct extraction point

        // Calculate extraction point: start of the word break gap, with safety margin
        // Using gap start (not midpoint) ensures we don't cut into the preceding word
        // The margin backs up slightly to ensure we capture any trailing sounds
        let gap_start_ms = relative_offset_ms;
        let extraction_point_ms = gap_start_ms.saturating_sub(WORD_BREAK_PRE_MARGIN_MS);
        let extraction_point_samples = self.ms_to_samples(extraction_point_ms);

        // Total extraction length includes lookback samples
        let extraction_length = self.lookback_sample_count as u64 + extraction_point_samples;

        // Ensure we don't extract more than we have in the segment
        let total_segment_samples = self.lookback_sample_count as u64 + self.segment_sample_count;
        let extraction_length = extraction_length.min(total_segment_samples);

        // Don't extract if the segment would be too short
        let extraction_duration_ms = self.samples_to_ms(extraction_length);
        if extraction_duration_ms < MIN_SEGMENT_DURATION_MS {
            tracing::debug!(
                "[TranscribeState] Word break would create segment too short ({}ms < {}ms), skipping",
                extraction_duration_ms,
                MIN_SEGMENT_DURATION_MS
            );
            return None;
        }

        if extraction_length == 0 {
            tracing::debug!(
                "[TranscribeState] Word break at offset {}ms but no samples to extract",
                offset_ms
            );
            return None;
        }

        // Calculate extraction end index in ring buffer
        let extraction_end_idx =
            (self.segment_start_idx + extraction_length as usize) % self.ring_buffer.capacity();

        // Extract segment up to the word break point
        let segment = self.extract_segment_to(extraction_end_idx);

        if segment.is_empty() {
            tracing::debug!("[TranscribeState] Word break extraction produced empty segment");
            return None;
        }

        tracing::debug!(
            "[TranscribeState] Word break split (vad_offset: {}ms, relative: {}ms, gap: {}ms), extracted {} samples ({} lookback + {} speech) at extraction point {}ms",
            offset_ms,
            relative_offset_ms,
            gap_duration_ms,
            segment.len(),
            self.lookback_sample_count,
            extraction_point_samples,
            extraction_point_ms
        );

        // Queue the segment for transcription (will validate before actually queueing).
        // Ring buffer contains mono processed audio (channels=1).
        self.queue_segment_with_channels(segment.clone(), 1);

        // Update state for next segment - the new segment starts at the extraction point
        // No lookback for continuation segments (we already have the audio in the buffer)
        self.segment_start_idx = extraction_end_idx;
        self.lookback_sample_count = 0;
        // Remaining samples in the segment: total minus what we extracted (excluding lookback)
        self.segment_sample_count = self
            .segment_sample_count
            .saturating_sub(extraction_point_samples);
        // Advance the VAD offset base by the extraction point so the next word-break offset
        // is correctly relativised to the start of the new segment.
        self.vad_offset_base_ms += extraction_point_ms;
        // Reset force-split seek state - we just cleanly split, so the grace timer restarts
        self.seeking_word_break = false;
        self.word_break_seek_start_samples = 0;

        Some(segment)
    }

    /// Extract segment from segment_start_idx to a specific end index
    fn extract_segment_to(&self, end_idx: usize) -> Vec<f32> {
        self.ring_buffer
            .extract_segment_to(self.segment_start_idx, end_idx)
    }

    /// Force segment extraction when grace period expires (no word break found)
    fn force_segment_extraction(&mut self) -> Option<Vec<f32>> {
        if !self.is_active || !self.in_speech {
            return None;
        }

        // Extract the current segment at the current position
        let segment = self.ring_buffer.extract_segment(self.segment_start_idx);

        if segment.is_empty() {
            tracing::debug!("[TranscribeState] Grace period expired but segment is empty");
            self.seeking_word_break = false;
            return None;
        }

        tracing::debug!(
            "[TranscribeState] Grace period expired, force extracted {} samples ({} lookback + {} speech)",
            segment.len(),
            self.lookback_sample_count,
            self.segment_sample_count
        );

        // Queue the segment for transcription (will validate before actually queueing).
        // Ring buffer contains mono processed audio (channels=1).
        self.queue_segment_with_channels(segment.clone(), 1);

        // Update state for next segment - remain in speech
        self.segment_start_idx = self.ring_buffer.write_position();
        self.segment_sample_count = 0;
        self.lookback_sample_count = 0;
        self.seeking_word_break = false;

        Some(segment)
    }

    /// Check if a segment has sufficient audio content for transcription
    /// Returns false if segment is too short or too quiet (likely to produce [BLANK_AUDIO])
    #[allow(dead_code)]
    fn is_segment_valid_for_transcription(&self, samples: &[f32]) -> bool {
        self.is_segment_valid_for_transcription_ch(samples, self.channels)
    }

    /// Check if a segment has sufficient audio content for transcription with an explicit channel count.
    fn is_segment_valid_for_transcription_ch(&self, samples: &[f32], channels: u16) -> bool {
        if samples.is_empty() {
            return false;
        }

        // Check minimum duration.
        // samples.len() is the total sample count; divide by channels to get frames.
        let frames = samples.len() as u64 / channels as u64;
        let duration_ms = frames * 1000 / self.sample_rate as u64;
        if duration_ms < MIN_SEGMENT_DURATION_MS {
            tracing::debug!(
                "[TranscribeState] Segment too short ({}ms < {}ms), skipping",
                duration_ms,
                MIN_SEGMENT_DURATION_MS
            );
            return false;
        }

        // Check RMS amplitude to ensure segment has audio content
        let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
        let rms = (sum_squares / samples.len() as f32).sqrt();

        if rms < MIN_AUDIO_RMS_THRESHOLD {
            tracing::debug!(
                "[TranscribeState] Segment too quiet (RMS {:.6} < {:.6}), skipping",
                rms,
                MIN_AUDIO_RMS_THRESHOLD
            );
            return false;
        }

        true
    }

    /// Queue a segment for transcription using `self.channels` (raw/hardware channel count).
    #[allow(dead_code)]
    fn queue_segment(&self, samples: Vec<f32>) {
        self.queue_segment_with_channels(samples, self.channels);
    }

    /// Queue a segment for transcription with an explicit channel count.
    ///
    /// Saves the segment to a new WAV file (using a timestamped filename) and
    /// enqueues it for transcription.
    ///
    /// Use this when the samples have already been mixed down to a different
    /// channel count than `self.channels` (e.g., mono processed audio with
    /// `channels = 1` when the hardware backend uses stereo).
    fn queue_segment_with_channels(&self, samples: Vec<f32>, channels: u16) {
        if samples.is_empty() {
            return;
        }

        // Validate segment has sufficient content using the correct channel count.
        if !self.is_segment_valid_for_transcription_ch(&samples, channels) {
            return;
        }

        // Save to WAV file in app data directory
        let filename = generate_recording_filename();
        let recordings_dir = crate::audio::recordings_dir();

        // Create directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&recordings_dir) {
            tracing::error!(
                "[TranscribeState] Failed to create recordings directory: {}",
                e
            );
        }

        let output_path = recordings_dir.join(&filename);
        let wav_path = match save_to_wav(&samples, self.sample_rate, channels, &output_path) {
            Ok(()) => {
                tracing::info!("[TranscribeState] Saved segment to: {:?}", output_path);
                // Intentionally do NOT fire on_recording_saved here. Segment WAVs are
                // transcription-pipeline implementation details; firing the callback would
                // overwrite last_recording_path and cause the demo to open a segment file
                // instead of the session-level WAV (raw or processed) saved by
                // save_recording_wav / submit_recording.
                Some(output_path)
            }
            Err(e) => {
                tracing::error!("[TranscribeState] Failed to save WAV: {}", e);
                None
            }
        };

        self.enqueue_for_transcription(samples, channels, wav_path);
    }

    /// Enqueue a segment for transcription with an already-saved WAV path.
    ///
    /// Unlike [`queue_segment_with_channels`], this does **not** save a WAV
    /// file.  Use this when the caller has already persisted the audio (e.g.,
    /// [`submit_recording`] saves its own raw + processed pair and should not
    /// have a third WAV overwrite the raw file).
    fn enqueue_for_transcription(
        &self,
        samples: Vec<f32>,
        channels: u16,
        wav_path: Option<std::path::PathBuf>,
    ) {
        if samples.is_empty() {
            return;
        }

        if !self.is_segment_valid_for_transcription_ch(&samples, channels) {
            return;
        }

        // Create queued segment
        let queued = QueuedSegment {
            samples,
            sample_rate: self.sample_rate,
            channels,
            wav_path,
        };

        // Enqueue for transcription
        if !self.transcription_queue.enqueue(queued) {
            tracing::warn!("[TranscribeState] Transcription queue is full, segment dropped");
        }

        // Emit queue update via callback
        let depth = self.transcription_queue.queue_depth();
        if let Some(ref cb) = self.callback {
            cb.on_queue_update(depth);
        }
    }

    /// Finalize any pending segment (called when transcribe mode is stopped).
    ///
    /// In manual recording mode, submits the accumulated recording buffer.
    /// In VAD mode, extracts and queues the in-progress speech segment.
    pub fn finalize(&mut self) -> Option<Vec<f32>> {
        if self.manual_recording {
            self.submit_recording();
            None
        } else if self.in_speech {
            // Extract and queue the in-progress segment
            self.on_speech_ended()
        } else {
            None
        }
    }
}
