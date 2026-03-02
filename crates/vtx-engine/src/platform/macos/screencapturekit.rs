//! ScreenCaptureKit system audio capture for macOS
//!
//! This module provides system audio capture functionality using Apple's ScreenCaptureKit API.
//! ScreenCaptureKit requires macOS 12.3+ and Screen Recording permission.
//!
//! Key features:
//! - System audio enumeration (lists available audio outputs)
//! - Audio-only capture (no video overhead minimized)
//! - Excludes app's own audio to prevent feedback
//! - Converts audio to f32 stereo at 48kHz

use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
use screencapturekit_sys::{
    cm_sample_buffer_ref::CMSampleBufferRef,
    content_filter::{UnsafeContentFilter, UnsafeInitParams},
    os_types::base::BOOL,
    os_types::rc::Id,
    shareable_content::UnsafeSCShareableContent,
    stream::UnsafeSCStream,
    stream_configuration::UnsafeStreamConfiguration,
    stream_error_handler::UnsafeSCStreamError,
    stream_output_handler::UnsafeSCStreamOutput,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Target sample rate for output (matches encoder expectations)
const TARGET_SAMPLE_RATE: u32 = 48000;

/// Target channel count
const TARGET_CHANNELS: u32 = 2;

/// Audio output type constant (matches SCStreamOutputType)
const SC_STREAM_OUTPUT_TYPE_AUDIO: u8 = 1;

/// System audio device representation
#[derive(Debug, Clone)]
pub struct SystemAudioDevice {
    pub id: String,
    pub name: String,
}

/// Audio samples from ScreenCaptureKit
pub struct SCKAudioSamples {
    pub samples: Vec<f32>,
    #[allow(dead_code)]
    pub channels: u16,
}

/// Check if system audio capture is available (macOS 12.3+)
pub fn is_available() -> bool {
    use std::process::Command;

    let output = Command::new("sw_vers").arg("-productVersion").output();

    match output {
        Ok(output) => {
            let version_str = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = version_str.trim().split('.').collect();

            if parts.len() >= 2 {
                let major: u32 = parts[0].parse().unwrap_or(0);
                let minor: u32 = parts[1].parse().unwrap_or(0);

                // ScreenCaptureKit requires macOS 12.3+
                if major > 12 {
                    return true;
                }
                if major == 12 && minor >= 3 {
                    return true;
                }
            }
            false
        }
        Err(_) => {
            // If we can't determine the version, assume it's available
            // The actual SCK calls will fail if not supported
            true
        }
    }
}

/// Check if screen recording permission is granted
pub fn check_permission() -> bool {
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Request screen recording permission
pub fn request_permission() {
    extern "C" {
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    unsafe {
        CGRequestScreenCaptureAccess();
    }
}

/// Enumerate available system audio devices
pub fn enumerate_system_devices() -> Result<Vec<SystemAudioDevice>, String> {
    if !is_available() {
        return Ok(Vec::new());
    }

    // Return a single "System Audio" device
    // ScreenCaptureKit captures all system audio, not individual outputs
    Ok(vec![SystemAudioDevice {
        id: "system-audio".to_string(),
        name: "System Audio".to_string(),
    }])
}

/// Commands for the ScreenCaptureKit thread
enum SCKCommand {
    Start {
        result_tx: mpsc::Sender<Result<(), String>>,
    },
    Stop,
    Shutdown,
}

/// System audio capture using ScreenCaptureKit
///
/// This struct manages system audio capture on macOS 12.3+.
/// Requires Screen Recording permission.
pub struct SCKAudioCapture {
    cmd_tx: mpsc::Sender<SCKCommand>,
    audio_rx: Mutex<mpsc::Receiver<SCKAudioSamples>>,
    is_capturing: Arc<AtomicBool>,
    thread_handle: Mutex<Option<JoinHandle<()>>>,
}

impl SCKAudioCapture {
    /// Create a new system audio capture
    pub fn new() -> Result<Self, String> {
        if !is_available() {
            return Err("System audio capture is not available (requires macOS 12.3+)".to_string());
        }

        tracing::info!("System Audio: Using ScreenCaptureKit (macOS 12.3+)");

        if !check_permission() {
            // Request permission - this will show the system dialog
            request_permission();

            // Check again after a brief delay
            std::thread::sleep(std::time::Duration::from_millis(100));

            if !check_permission() {
                return Err(
                    "Screen Recording permission is required for system audio capture. \
                    Please enable it in System Settings > Privacy & Security > Screen Recording."
                        .to_string(),
                );
            }
        }

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (audio_tx, audio_rx) = mpsc::channel();
        let is_capturing = Arc::new(AtomicBool::new(false));
        let is_capturing_clone = Arc::clone(&is_capturing);

        // Start the capture thread
        let thread_handle = thread::spawn(move || {
            run_sck_thread(cmd_rx, audio_tx, is_capturing_clone);
        });

        Ok(Self {
            cmd_tx,
            audio_rx: Mutex::new(audio_rx),
            is_capturing,
            thread_handle: Mutex::new(Some(thread_handle)),
        })
    }

    /// Start capturing system audio
    pub fn start(&self) -> Result<(), String> {
        if self.is_capturing.load(Ordering::SeqCst) {
            return Ok(());
        }

        let (result_tx, result_rx) = mpsc::channel();

        self.cmd_tx
            .send(SCKCommand::Start { result_tx })
            .map_err(|_| "Failed to send start command".to_string())?;

        match result_rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(result) => result,
            Err(_) => Err("Timeout starting system audio capture".to_string()),
        }
    }

    /// Stop capturing system audio
    pub fn stop(&self) -> Result<(), String> {
        if !self.is_capturing.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.cmd_tx
            .send(SCKCommand::Stop)
            .map_err(|_| "Failed to send stop command".to_string())?;

        // Wait briefly for stop to take effect
        std::thread::sleep(std::time::Duration::from_millis(100));

        Ok(())
    }

    /// Try to receive audio samples (non-blocking)
    pub fn try_recv(&self) -> Option<SCKAudioSamples> {
        self.audio_rx.lock().unwrap().try_recv().ok()
    }

    /// Check if capture is active
    #[allow(dead_code)]
    pub fn is_capturing(&self) -> bool {
        self.is_capturing.load(Ordering::SeqCst)
    }
}

impl Drop for SCKAudioCapture {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(SCKCommand::Shutdown);
        if let Some(handle) = self.thread_handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

/// Run the ScreenCaptureKit capture thread
///
/// This thread runs a CFRunLoop which is required for ScreenCaptureKit's
/// async completion handlers to fire. Without an active run loop, the
/// completion handlers for getShareableContent and startCapture will
/// never be called, causing hangs.
fn run_sck_thread(
    cmd_rx: mpsc::Receiver<SCKCommand>,
    audio_tx: mpsc::Sender<SCKAudioSamples>,
    is_capturing: Arc<AtomicBool>,
) {
    tracing::debug!("ScreenCaptureKit: Capture thread started");

    // State for active capture
    let mut capture_state: Option<SCKCaptureState> = None;

    // Get the run loop for this thread
    let run_loop = CFRunLoop::get_current();

    loop {
        // Run the CFRunLoop for a short interval to process any pending callbacks
        // This is essential for ScreenCaptureKit completion handlers to fire
        // The run loop will return after the timeout or if it has no sources
        // (in which case we immediately continue to check for commands)
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            std::time::Duration::from_millis(10),
            true,
        );

        // Check for commands (non-blocking)
        match cmd_rx.try_recv() {
            Ok(SCKCommand::Start { result_tx }) => {
                // Stop any existing capture
                if let Some(state) = capture_state.take() {
                    drop(state);
                }

                // Start new capture
                match start_capture(audio_tx.clone()) {
                    Ok(state) => {
                        capture_state = Some(state);
                        is_capturing.store(true, Ordering::SeqCst);
                        let _ = result_tx.send(Ok(()));
                        tracing::info!("ScreenCaptureKit: Capture started");
                    }
                    Err(e) => {
                        is_capturing.store(false, Ordering::SeqCst);
                        let _ = result_tx.send(Err(e));
                    }
                }
            }
            Ok(SCKCommand::Stop) => {
                if let Some(state) = capture_state.take() {
                    drop(state);
                }
                is_capturing.store(false, Ordering::SeqCst);
                tracing::info!("ScreenCaptureKit: Capture stopped");
            }
            Ok(SCKCommand::Shutdown) => {
                if let Some(state) = capture_state.take() {
                    drop(state);
                }
                is_capturing.store(false, Ordering::SeqCst);
                run_loop.stop();
                break;
            }
            Err(mpsc::TryRecvError::Empty) => {
                // No commands, continue running the loop
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                run_loop.stop();
                break;
            }
        }
    }

    tracing::debug!("ScreenCaptureKit: Capture thread exiting");
}

/// Error handler for SCStream
struct AudioCaptureErrorHandler;

impl UnsafeSCStreamError for AudioCaptureErrorHandler {
    fn handle_error(&self) {
        tracing::error!("ScreenCaptureKit: Stream error occurred");
    }
}

/// Audio output handler that converts CMSampleBuffer to audio samples
struct AudioOutputHandler {
    tx: mpsc::Sender<SCKAudioSamples>,
    stop_flag: Arc<AtomicBool>,
}

impl UnsafeSCStreamOutput for AudioOutputHandler {
    fn did_output_sample_buffer(&self, sample: Id<CMSampleBufferRef>, of_type: u8) {
        // Only handle audio output (type 1)
        if of_type != SC_STREAM_OUTPUT_TYPE_AUDIO {
            return;
        }

        // Check if we should stop
        if self.stop_flag.load(Ordering::Relaxed) {
            return;
        }

        // Check if this sample buffer has valid audio format description
        let format_desc = match sample.get_format_description() {
            Some(desc) => desc,
            None => {
                // No format description means no valid audio data
                return;
            }
        };

        // Get AudioStreamBasicDescription
        let asbd = match format_desc.audio_format_description_get_stream_basic_description() {
            Some(desc) => desc,
            None => {
                // Not an audio format description
                return;
            }
        };

        let sample_rate = asbd.sample_rate as u32;
        if sample_rate == 0 {
            return;
        }

        // Check format flags for non-interleaved audio
        // kAudioFormatFlagIsNonInterleaved = 32
        let is_non_interleaved = (asbd.format_flags & 32) != 0;
        let channel_count = asbd.channels_per_frame as usize;

        // Log first sample's format info for debugging
        static LOGGED_FORMAT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !LOGGED_FORMAT.swap(true, Ordering::Relaxed) {
            tracing::debug!(
                "ScreenCaptureKit: Audio format: rate={}, channels={}, flags={}, non_interleaved={}",
                sample_rate,
                channel_count,
                asbd.format_flags,
                is_non_interleaved
            );
        }

        // Get audio buffer list
        let audio_buffers = sample.get_av_audio_buffer_list();

        if audio_buffers.is_empty() {
            return;
        }

        // Handle non-interleaved vs interleaved audio
        let interleaved_samples: Vec<f32> = if is_non_interleaved && audio_buffers.len() >= 2 {
            // Non-interleaved stereo: interleave the two channel buffers
            let left_bytes = &audio_buffers[0].data;
            let right_bytes = &audio_buffers[1].data;

            let left_samples: &[f32] = unsafe {
                std::slice::from_raw_parts(
                    left_bytes.as_ptr() as *const f32,
                    left_bytes.len() / std::mem::size_of::<f32>(),
                )
            };
            let right_samples: &[f32] = unsafe {
                std::slice::from_raw_parts(
                    right_bytes.as_ptr() as *const f32,
                    right_bytes.len() / std::mem::size_of::<f32>(),
                )
            };

            // Interleave: L0, R0, L1, R1, L2, R2, ...
            let frame_count = left_samples.len().min(right_samples.len());
            let mut interleaved = Vec::with_capacity(frame_count * 2);
            for i in 0..frame_count {
                interleaved.push(left_samples[i]);
                interleaved.push(right_samples[i]);
            }
            interleaved
        } else if is_non_interleaved && audio_buffers.len() == 1 {
            // Non-interleaved mono: duplicate to stereo
            let mono_bytes = &audio_buffers[0].data;
            let mono_samples: &[f32] = unsafe {
                std::slice::from_raw_parts(
                    mono_bytes.as_ptr() as *const f32,
                    mono_bytes.len() / std::mem::size_of::<f32>(),
                )
            };
            mono_samples.iter().flat_map(|&s| [s, s]).collect()
        } else {
            // Interleaved audio: collect all samples directly
            let mut all_samples: Vec<f32> = Vec::new();
            for buffer in &audio_buffers {
                let bytes = &buffer.data;
                let sample_count = bytes.len() / std::mem::size_of::<f32>();
                if sample_count > 0 {
                    let samples: &[f32] = unsafe {
                        std::slice::from_raw_parts(bytes.as_ptr() as *const f32, sample_count)
                    };
                    all_samples.extend_from_slice(samples);
                }
            }

            // Handle mono to stereo if needed
            if channel_count == 1 {
                all_samples.iter().flat_map(|&s| [s, s]).collect()
            } else {
                all_samples
            }
        };

        if interleaved_samples.is_empty() {
            return;
        }

        // Resample to target rate if needed
        let final_samples = if sample_rate != TARGET_SAMPLE_RATE {
            resample_linear(&interleaved_samples, sample_rate, TARGET_SAMPLE_RATE, 2)
        } else {
            interleaved_samples
        };

        // Send to channel
        let _ = self.tx.send(SCKAudioSamples {
            samples: final_samples,
            channels: TARGET_CHANNELS as u16,
        });
    }
}

/// Simple linear resampler for converting audio sample rates
fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32, channels: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let channels = channels as usize;
    let input_frames = samples.len() / channels;
    let output_frames = ((input_frames as f64) / ratio).ceil() as usize;

    let mut output = Vec::with_capacity(output_frames * channels);

    for out_frame in 0..output_frames {
        let in_pos = out_frame as f64 * ratio;
        let in_frame = in_pos.floor() as usize;
        let frac = (in_pos - in_frame as f64) as f32;

        for ch in 0..channels {
            let idx0 = in_frame * channels + ch;
            let idx1 = ((in_frame + 1).min(input_frames - 1)) * channels + ch;

            if idx0 < samples.len() && idx1 < samples.len() {
                // Linear interpolation between adjacent samples
                let s0 = samples[idx0];
                let s1 = samples[idx1];
                output.push(s0 + frac * (s1 - s0));
            } else if idx0 < samples.len() {
                output.push(samples[idx0]);
            }
        }
    }

    output
}

/// State for an active ScreenCaptureKit capture session
struct SCKCaptureState {
    stop_flag: Arc<AtomicBool>,
    // Keep the stream alive - it will be dropped when this struct is dropped
    _stream_keepalive: std::thread::JoinHandle<()>,
}

impl Drop for SCKCaptureState {
    fn drop(&mut self) {
        // Signal stop
        self.stop_flag.store(true, Ordering::SeqCst);
        tracing::debug!("ScreenCaptureKit: Cleaning up capture state");
    }
}

/// Start a ScreenCaptureKit capture session
fn start_capture(audio_tx: mpsc::Sender<SCKAudioSamples>) -> Result<SCKCaptureState, String> {
    tracing::info!("ScreenCaptureKit: Starting capture - getting shareable content...");

    // Get shareable content
    let content = UnsafeSCShareableContent::get()
        .map_err(|e| format!("Failed to get shareable content: {}", e))?;

    tracing::info!("ScreenCaptureKit: Got shareable content, looking for display...");

    // Get the first display (required even for audio-only capture)
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or_else(|| "No display found".to_string())?;

    let display_width = display.get_width();
    let display_height = display.get_height();
    tracing::debug!(
        "ScreenCaptureKit: Found display {}x{}",
        display_width,
        display_height
    );

    // Create content filter for the display
    tracing::debug!("ScreenCaptureKit: Creating content filter...");
    let filter = UnsafeContentFilter::init(UnsafeInitParams::Display(display));

    // Configure stream with audio enabled
    // Use minimal video settings since we only want audio
    tracing::debug!("ScreenCaptureKit: Configuring stream...");
    let config = UnsafeStreamConfiguration {
        width: display_width.min(320), // Small to minimize overhead
        height: display_height.min(240),
        captures_audio: BOOL::from(true),
        sample_rate: TARGET_SAMPLE_RATE,
        channel_count: TARGET_CHANNELS,
        excludes_current_process_audio: BOOL::from(true), // Don't capture our own audio
        shows_cursor: BOOL::from(false),
        ..Default::default()
    };

    // Create stop flag
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();

    // Create stream
    tracing::debug!("ScreenCaptureKit: Creating SCStream...");
    let stream = UnsafeSCStream::init(filter, config.into(), AudioCaptureErrorHandler);

    // Add audio output handler
    tracing::debug!("ScreenCaptureKit: Adding audio output handler...");
    let handler = AudioOutputHandler {
        tx: audio_tx,
        stop_flag: stop_flag.clone(),
    };
    stream.add_stream_output(handler, SC_STREAM_OUTPUT_TYPE_AUDIO);

    // Start capture
    tracing::debug!("ScreenCaptureKit: Starting capture...");
    stream
        .start_capture()
        .map_err(|e| format!("Failed to start capture: {}", e))?;

    tracing::info!("ScreenCaptureKit: Audio capture started successfully");

    // Keep stream alive in a background thread
    let stream_thread = std::thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        tracing::debug!("ScreenCaptureKit: Stopping stream");
        let _ = stream.stop_capture();
    });

    Ok(SCKCaptureState {
        stop_flag: stop_flag_clone,
        _stream_keepalive: stream_thread,
    })
}
