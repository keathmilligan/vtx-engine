//! Transcription queue for async processing.
//!
//! This module provides a bounded queue for audio segments awaiting transcription,
//! with a worker thread that processes segments sequentially.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::audio::{process_recorded_audio, RawRecordedAudio};

use super::Transcriber;

/// Maximum queue size for transcription segments
const MAX_QUEUE_SIZE: usize = 10;

/// A segment of audio queued for transcription.
pub struct QueuedSegment {
    /// Audio samples (raw, may be multi-channel)
    pub samples: Vec<f32>,
    /// Sample rate of the audio
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
    /// Path to saved WAV file (if saved)
    pub wav_path: Option<PathBuf>,
}

/// Callback trait for transcription events.
///
/// Implement this trait to receive transcription results and status updates.
pub trait TranscriptionCallback: Send + Sync + 'static {
    /// Called when transcription is about to start (GPU may become active).
    fn on_transcription_started(&self);

    /// Called when transcription completes successfully.
    fn on_transcription_complete(&self, text: String, wav_path: Option<String>);

    /// Called when transcription fails.
    fn on_transcription_error(&self, error: String);

    /// Called when transcription finishes (GPU no longer active).
    fn on_transcription_finished(&self);

    /// Called when the queue depth changes.
    fn on_queue_update(&self, depth: usize);
}

/// Queue for managing transcription segments.
pub struct TranscriptionQueue {
    /// The queue of segments
    queue: Arc<Mutex<VecDeque<QueuedSegment>>>,
    /// Flag indicating worker should continue running
    worker_active: Arc<AtomicBool>,
    /// Count of segments currently in queue
    queue_count: Arc<AtomicUsize>,
    /// Callback for transcription events
    callback: Arc<Mutex<Option<Arc<dyn TranscriptionCallback>>>>,
}

impl TranscriptionQueue {
    /// Create a new transcription queue.
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            worker_active: Arc::new(AtomicBool::new(false)),
            queue_count: Arc::new(AtomicUsize::new(0)),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the callback for transcription events.
    pub fn set_callback(&self, callback: Arc<dyn TranscriptionCallback>) {
        *self.callback.lock().unwrap() = Some(callback);
    }

    /// Clear the callback.
    pub fn clear_callback(&self) {
        *self.callback.lock().unwrap() = None;
    }

    /// Get the current queue depth.
    pub fn queue_depth(&self) -> usize {
        self.queue_count.load(Ordering::SeqCst)
    }

    /// Check if the worker is active.
    pub fn is_worker_active(&self) -> bool {
        self.worker_active.load(Ordering::SeqCst)
    }

    /// Enqueue a segment for transcription.
    /// Returns false if queue is full (segment was not added).
    pub fn enqueue(&self, segment: QueuedSegment) -> bool {
        let mut queue = self.queue.lock().unwrap();
        if queue.len() >= MAX_QUEUE_SIZE {
            // Queue is full, don't add
            return false;
        }
        queue.push_back(segment);
        let depth = queue.len();
        self.queue_count.store(depth, Ordering::SeqCst);

        // Notify callback of queue update
        if let Some(ref cb) = *self.callback.lock().unwrap() {
            cb.on_queue_update(depth);
        }

        true
    }

    /// Start the transcription worker thread.
    pub fn start_worker(&self, model_path: PathBuf) {
        if self.worker_active.load(Ordering::SeqCst) {
            return; // Already running
        }

        self.worker_active.store(true, Ordering::SeqCst);

        let queue = Arc::clone(&self.queue);
        let worker_active = Arc::clone(&self.worker_active);
        let queue_count = Arc::clone(&self.queue_count);
        let callback = Arc::clone(&self.callback);

        thread::spawn(move || {
            let mut transcriber = Transcriber::new();

            // Try to load model at start
            if model_path.exists() {
                if let Err(e) = transcriber.load_model() {
                    tracing::error!("[TranscriptionQueue] Failed to load model: {}", e);
                }
            }

            loop {
                // Check if we should stop
                if !worker_active.load(Ordering::SeqCst) {
                    // Drain remaining queue before exiting
                    let remaining = {
                        let q = queue.lock().unwrap();
                        q.len()
                    };
                    if remaining == 0 {
                        break;
                    }
                    // Continue processing remaining items
                }

                // Try to get a segment from queue
                let segment = {
                    let mut q = queue.lock().unwrap();
                    let seg = q.pop_front();
                    let depth = q.len();
                    queue_count.store(depth, Ordering::SeqCst);

                    // Notify callback of queue update
                    if seg.is_some() {
                        if let Some(ref cb) = *callback.lock().unwrap() {
                            cb.on_queue_update(depth);
                        }
                    }

                    seg
                };

                match segment {
                    Some(seg) => {
                        // Process the segment
                        let raw_audio = RawRecordedAudio {
                            samples: seg.samples,
                            sample_rate: seg.sample_rate,
                            channels: seg.channels,
                        };

                        let wav_path_str = seg
                            .wav_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string());

                        // Convert to format suitable for Whisper
                        match process_recorded_audio(raw_audio) {
                            Ok(processed) => {
                                // Notify that transcription is starting
                                if let Some(ref cb) = *callback.lock().unwrap() {
                                    cb.on_transcription_started();
                                }

                                // Transcribe
                                match transcriber.transcribe(&processed) {
                                    Ok(text) => {
                                        if let Some(ref cb) = *callback.lock().unwrap() {
                                            cb.on_transcription_complete(text, wav_path_str);
                                        }
                                    }
                                    Err(e) => {
                                        if let Some(ref cb) = *callback.lock().unwrap() {
                                            cb.on_transcription_error(e);
                                        }
                                    }
                                }

                                // Notify that transcription finished
                                if let Some(ref cb) = *callback.lock().unwrap() {
                                    cb.on_transcription_finished();
                                }
                            }
                            Err(e) => {
                                if let Some(ref cb) = *callback.lock().unwrap() {
                                    cb.on_transcription_error(e);
                                }
                            }
                        }
                    }
                    None => {
                        // No segment available, sleep briefly
                        thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }

            tracing::info!("[TranscriptionQueue] Worker thread exiting");
        });
    }

    /// Stop the transcription worker (will drain remaining queue).
    pub fn stop_worker(&self) {
        self.worker_active.store(false, Ordering::SeqCst);
    }

    /// Clear the queue (discard pending segments).
    pub fn clear(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.clear();
        self.queue_count.store(0, Ordering::SeqCst);

        // Notify callback
        if let Some(ref cb) = *self.callback.lock().unwrap() {
            cb.on_queue_update(0);
        }
    }
}

impl Default for TranscriptionQueue {
    fn default() -> Self {
        Self::new()
    }
}
