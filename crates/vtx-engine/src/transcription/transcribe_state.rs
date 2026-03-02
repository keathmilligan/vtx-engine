//! Automatic transcription mode with continuous recording and speech-based segmentation.
//!
//! This module provides:
//! - `SegmentRingBuffer`: A ring buffer for continuous audio capture
//! - `TranscribeState`: State management for transcribe mode

use std::sync::Arc;

use crate::audio::{generate_recording_filename, save_to_wav};

use super::queue::{QueuedSegment, TranscriptionQueue};

/// Ring buffer capacity: 30 seconds at 48kHz stereo
/// 48000 * 30 * 2 = 2,880,000 samples
const RING_BUFFER_CAPACITY: usize = 48000 * 30 * 2;

/// Overflow threshold: 90% of buffer capacity
const OVERFLOW_THRESHOLD_PERCENT: usize = 90;

/// Maximum segment duration before seeking word break
const MAX_SEGMENT_DURATION_MS: u64 = 4000;

/// Grace period after duration threshold before forcing segment submission (500ms)
const WORD_BREAK_GRACE_MS: u64 = 750;

/// Minimum segment duration to submit for transcription (200ms)
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
    /// Callback for state events
    callback: Option<Arc<dyn TranscribeStateCallback>>,
    /// PTT mode - disables automatic segmentation
    ptt_mode: bool,
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
            callback: None,
            ptt_mode: false,
        }
    }

    /// Enable or disable PTT mode.
    /// In PTT mode, automatic segmentation is disabled - segments are only
    /// submitted when explicitly ended via on_speech_ended().
    pub fn set_ptt_mode(&mut self, enabled: bool) {
        self.ptt_mode = enabled;
        if enabled {
            tracing::debug!("[TranscribeState] PTT mode enabled - automatic segmentation disabled");
        }
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
    }

    /// Deactivate transcribe mode
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.in_speech = false;
        self.seeking_word_break = false;
    }

    /// Process incoming audio samples - writes to ring buffer and checks for overflow/duration
    /// Returns Some(segment) if overflow extraction or grace period extraction occurred
    /// Note: In PTT mode, automatic segmentation is disabled and this always returns None
    pub fn process_samples(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
        if !self.is_active {
            return None;
        }

        // In PTT mode, skip all automatic segmentation - just write samples
        if self.ptt_mode {
            self.ring_buffer.write(samples);
            if self.in_speech {
                self.segment_sample_count += samples.len() as u64;
            }
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

        // If we extracted a segment due to overflow, queue it
        if let Some(segment) = overflow_segment.clone() {
            self.queue_segment(segment);
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

        // Queue the segment for transcription (will validate before actually queueing)
        self.queue_segment(segment.clone());

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

    /// Handle word-break event: if seeking a word break, extract and submit segment
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
    pub fn on_word_break(&mut self, offset_ms: u32, gap_duration_ms: u32) -> Option<Vec<f32>> {
        if !self.is_active || !self.in_speech || !self.seeking_word_break {
            return None;
        }

        // The word break offset_ms is from when speech was confirmed (not including lookback)
        // Our segment includes lookback samples at the start
        // So we need to add lookback to get the correct extraction point

        // Calculate extraction point: start of the word break gap, with safety margin
        // Using gap start (not midpoint) ensures we don't cut into the preceding word
        // The margin backs up slightly to ensure we capture any trailing sounds
        let gap_start_ms = offset_ms as u64;
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
            "[TranscribeState] Timed segment at word break (offset: {}ms, gap: {}ms), extracted {} samples ({} lookback + {} speech) at extraction point {}ms",
            offset_ms,
            gap_duration_ms,
            segment.len(),
            self.lookback_sample_count,
            extraction_point_samples,
            extraction_point_ms
        );

        // Queue the segment for transcription (will validate before actually queueing)
        self.queue_segment(segment.clone());

        // Update state for next segment - the new segment starts at the extraction point
        // No lookback for continuation segments (we already have the audio in the buffer)
        self.segment_start_idx = extraction_end_idx;
        self.lookback_sample_count = 0;
        // Remaining samples in the segment: total minus what we extracted (excluding lookback)
        self.segment_sample_count = self
            .segment_sample_count
            .saturating_sub(extraction_point_samples);
        self.seeking_word_break = false;

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

        // Queue the segment for transcription (will validate before actually queueing)
        self.queue_segment(segment.clone());

        // Update state for next segment - remain in speech
        self.segment_start_idx = self.ring_buffer.write_position();
        self.segment_sample_count = 0;
        self.lookback_sample_count = 0;
        self.seeking_word_break = false;

        Some(segment)
    }

    /// Check if a segment has sufficient audio content for transcription
    /// Returns false if segment is too short or too quiet (likely to produce [BLANK_AUDIO])
    fn is_segment_valid_for_transcription(&self, samples: &[f32]) -> bool {
        if samples.is_empty() {
            return false;
        }

        // Check minimum duration
        // samples.len() is raw sample count (stereo), divide by channels to get frames
        let frames = samples.len() as u64 / self.channels as u64;
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

    /// Queue a segment for transcription (saves WAV and enqueues)
    fn queue_segment(&self, samples: Vec<f32>) {
        if samples.is_empty() {
            return;
        }

        // Validate segment has sufficient content
        if !self.is_segment_valid_for_transcription(&samples) {
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
        let wav_path = match save_to_wav(&samples, self.sample_rate, self.channels, &output_path) {
            Ok(()) => {
                tracing::info!("[TranscribeState] Saved segment to: {:?}", output_path);
                if let Some(ref cb) = self.callback {
                    cb.on_recording_saved(output_path.to_string_lossy().to_string());
                }
                Some(output_path)
            }
            Err(e) => {
                tracing::error!("[TranscribeState] Failed to save WAV: {}", e);
                None
            }
        };

        // Create queued segment
        let queued = QueuedSegment {
            samples,
            sample_rate: self.sample_rate,
            channels: self.channels,
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

    /// Finalize any pending segment (called when transcribe mode is stopped)
    pub fn finalize(&mut self) -> Option<Vec<f32>> {
        if self.in_speech {
            // Extract and queue the in-progress segment
            self.on_speech_ended()
        } else {
            None
        }
    }
}
