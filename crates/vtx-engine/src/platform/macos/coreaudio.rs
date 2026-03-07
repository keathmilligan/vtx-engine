//! CoreAudio backend for macOS
//!
//! This module provides full audio capture functionality for macOS:
//! - Input device enumeration (microphones) via CoreAudio
//! - System audio enumeration and capture via ScreenCaptureKit (macOS 12.3+)
//! - Multi-source capture with mixing
//! - Echo cancellation using AEC3

use crate::platform::backend::{AudioBackend, AudioData};
use crate::platform::macos::screencapturekit::{self, SCKAudioCapture};
use crate::{AudioDevice, AudioSourceType, RecordingMode};
use aec3::voip::VoipAec3;
use coreaudio::audio_unit::macos_helpers::{
    get_audio_device_ids, get_audio_device_supports_scope, get_default_device_id, get_device_name,
};
use coreaudio::audio_unit::Scope;
use coreaudio::sys::{
    self, kAudioOutputUnitProperty_SetInputCallback, kAudioUnitProperty_StreamFormat, AudioBuffer,
    AudioBufferList, AudioUnitRenderActionFlags,
};
use std::collections::HashSet;
use std::os::raw::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Target sample rate for output (matches Linux/Windows backends)
const TARGET_SAMPLE_RATE: f64 = 48000.0;

/// AEC3 frame size: 10ms at 48kHz = 480 samples per channel
const AEC_FRAME_SAMPLES: usize = 480;

/// System audio device ID prefix
const SYSTEM_AUDIO_PREFIX: &str = "sck:";

/// Context passed to the input callback
struct InputCallbackContext {
    audio_unit: sys::AudioUnit,
    audio_tx: mpsc::Sender<StreamSamples>,
    resampler: Option<Mutex<Resampler>>,
    num_channels: usize,
    is_non_interleaved: bool,
    stream_index: usize,
}

/// Raw input callback procedure for CoreAudio
extern "C" fn input_callback_proc(
    in_ref_con: *mut c_void,
    io_action_flags: *mut AudioUnitRenderActionFlags,
    in_time_stamp: *const sys::AudioTimeStamp,
    in_bus_number: u32,
    in_number_frames: u32,
    _io_data: *mut AudioBufferList,
) -> sys::OSStatus {
    let context = unsafe { &*(in_ref_con as *const InputCallbackContext) };

    // Allocate buffer list for the audio data
    let num_buffers = if context.is_non_interleaved {
        context.num_channels
    } else {
        1
    };

    let bytes_per_frame = 4; // f32
    let frames_per_buffer = in_number_frames as usize;
    let bytes_per_buffer = if context.is_non_interleaved {
        // Non-interleaved: each buffer has one channel
        frames_per_buffer * bytes_per_frame
    } else {
        // Interleaved: single buffer has all channels
        frames_per_buffer * context.num_channels * bytes_per_frame
    };

    // Create buffer storage
    let mut buffer_data: Vec<Vec<u8>> = (0..num_buffers)
        .map(|_| vec![0u8; bytes_per_buffer])
        .collect();

    // Create AudioBufferList
    let buffer_list_size = std::mem::size_of::<AudioBufferList>()
        + (num_buffers - 1) * std::mem::size_of::<AudioBuffer>();
    let mut buffer_list_storage = vec![0u8; buffer_list_size];
    let buffer_list = buffer_list_storage.as_mut_ptr() as *mut AudioBufferList;

    unsafe {
        (*buffer_list).mNumberBuffers = num_buffers as u32;

        let buffers_ptr = (*buffer_list).mBuffers.as_mut_ptr();
        for (i, data) in buffer_data.iter_mut().enumerate().take(num_buffers) {
            let buffer = &mut *buffers_ptr.add(i);
            buffer.mNumberChannels = if context.is_non_interleaved {
                1
            } else {
                context.num_channels as u32
            };
            buffer.mDataByteSize = bytes_per_buffer as u32;
            buffer.mData = data.as_mut_ptr() as *mut c_void;
        }
    }

    // Call AudioUnitRender to get the audio data
    let status = unsafe {
        sys::AudioUnitRender(
            context.audio_unit,
            io_action_flags,
            in_time_stamp,
            in_bus_number,
            in_number_frames,
            buffer_list,
        )
    };

    if status != 0 {
        tracing::error!(
            "[AudioCallback] AudioUnitRender failed with OSStatus: {}",
            status
        );
        return status;
    }

    // Process the audio data
    let num_frames = in_number_frames as usize;
    let mut samples = Vec::with_capacity(num_frames * 2);

    unsafe {
        let buffer_list_ref = &*buffer_list;

        if context.is_non_interleaved {
            let buffers_ptr = buffer_list_ref.mBuffers.as_ptr();

            let mut channel_ptrs: Vec<*const f32> = Vec::with_capacity(num_buffers);
            for i in 0..num_buffers {
                let buffer = &*buffers_ptr.add(i);
                channel_ptrs.push(buffer.mData as *const f32);
            }

            // Interleave to stereo
            for i in 0..num_frames {
                let left = *channel_ptrs[0].add(i);
                let right = if num_buffers > 1 {
                    *channel_ptrs[1].add(i)
                } else {
                    left
                };
                samples.push(left);
                samples.push(right);
            }
        } else {
            let buffer = &buffer_list_ref.mBuffers[0];
            let data_ptr = buffer.mData as *const f32;

            if context.num_channels == 1 {
                for i in 0..num_frames {
                    let sample = *data_ptr.add(i);
                    samples.push(sample);
                    samples.push(sample);
                }
            } else {
                let total_samples = num_frames * context.num_channels;
                for i in 0..total_samples {
                    samples.push(*data_ptr.add(i));
                }
            }
        }
    }

    // Resample if needed
    let samples = if let Some(ref resampler) = context.resampler {
        resampler.lock().unwrap().process(&samples, 2)
    } else {
        samples
    };

    if !samples.is_empty() {
        let _ = context.audio_tx.send(StreamSamples {
            stream_index: context.stream_index,
            samples,
            is_loopback: false,
        });
    }

    0 // noErr
}

/// Internal audio samples for channel communication
struct CoreAudioSamples {
    samples: Vec<f32>,
    channels: u16,
}

/// Samples from a stream thread to the mixer
struct StreamSamples {
    #[allow(dead_code)]
    stream_index: usize,
    samples: Vec<f32>,
    /// Whether this stream is loopback (system audio) - used for AEC routing
    is_loopback: bool,
}

/// Commands sent to the capture thread
enum CaptureCommand {
    StartSources {
        source1_id: Option<String>,
        source2_id: Option<String>,
        result_tx: mpsc::Sender<Result<(), String>>,
    },
    Stop,
    Shutdown,
}

/// Audio mixer for combining samples from multiple streams
struct AudioMixer {
    /// Buffer for capture samples (microphone/input)
    capture_buffer: Vec<f32>,
    /// Buffer for render samples (system audio/reference) - fed to AEC and kept for mixing
    render_buffer: Vec<f32>,
    /// Buffer for render samples to mix with processed capture (for Mixed mode)
    render_mix_buffer: Vec<f32>,
    /// Number of active streams (1 or 2)
    num_streams: usize,
    /// Channels per stream
    channels: u16,
    /// Output sender
    output_tx: mpsc::Sender<CoreAudioSamples>,
    /// Flag to enable/disable AEC (shared with main thread)
    aec_enabled: Arc<Mutex<bool>>,
    /// Recording mode - Mixed or EchoCancel (shared with main thread)
    recording_mode: Arc<Mutex<RecordingMode>>,
    /// AEC3 pipeline (created when in mixed mode with 2 streams)
    aec: Option<VoipAec3>,
}

impl AudioMixer {
    fn new(
        output_tx: mpsc::Sender<CoreAudioSamples>,
        aec_enabled: Arc<Mutex<bool>>,
        recording_mode: Arc<Mutex<RecordingMode>>,
    ) -> Self {
        Self {
            capture_buffer: Vec::new(),
            render_buffer: Vec::new(),
            render_mix_buffer: Vec::new(),
            num_streams: 0,
            channels: 2,
            output_tx,
            aec_enabled,
            recording_mode,
            aec: None,
        }
    }

    fn set_num_streams(&mut self, num: usize) {
        self.num_streams = num;
        self.capture_buffer.clear();
        self.render_buffer.clear();
        self.render_mix_buffer.clear();

        // Create AEC3 pipeline when we have 2 streams (mic + system audio)
        if num == 2 {
            match VoipAec3::builder(48000, self.channels as usize, self.channels as usize)
                .enable_high_pass(true)
                .initial_delay_ms(0)
                .build()
            {
                Ok(aec) => {
                    tracing::info!(
                        "CoreAudio: AEC3 initialized: 48kHz, {} channels, {}ms frames",
                        self.channels,
                        AEC_FRAME_SAMPLES * 1000 / 48000
                    );
                    self.aec = Some(aec);
                }
                Err(e) => {
                    tracing::error!("CoreAudio: Failed to initialize AEC3: {:?}", e);
                    self.aec = None;
                }
            }
        } else {
            self.aec = None;
        }
    }

    /// Add samples from a stream, routing based on source type
    fn push_samples(&mut self, samples: &[f32], is_loopback: bool) {
        if self.num_streams == 1 {
            // Single stream - send directly (no AEC possible)
            let _ = self.output_tx.send(CoreAudioSamples {
                samples: samples.to_vec(),
                channels: self.channels,
            });
            return;
        }

        // Two streams mode
        let frame_size = AEC_FRAME_SAMPLES * self.channels as usize;

        if is_loopback {
            // System audio (render) - feed to AEC immediately
            self.render_buffer.extend_from_slice(samples);
            self.render_mix_buffer.extend_from_slice(samples);

            // Feed render frames to AEC immediately
            if let Some(ref mut aec) = self.aec {
                while self.render_buffer.len() >= frame_size {
                    let render_frame: Vec<f32> = self.render_buffer.drain(0..frame_size).collect();
                    if let Err(e) = aec.handle_render_frame(&render_frame) {
                        tracing::error!("CoreAudio: AEC3 handle_render_frame error: {:?}", e);
                    }
                }
            }
        } else {
            // Microphone (capture) - buffer and process
            self.capture_buffer.extend_from_slice(samples);
            self.process_capture();
        }
    }

    /// Process buffered capture samples through AEC
    fn process_capture(&mut self) {
        let aec_enabled = *self.aec_enabled.lock().unwrap();
        let recording_mode = *self.recording_mode.lock().unwrap();

        let frame_size = AEC_FRAME_SAMPLES * self.channels as usize;

        // In EchoCancel mode the render frame is not mixed into the output, so
        // we don't need to wait for render_mix_buffer to fill before processing
        // capture frames — doing so would stall mic output whenever system audio
        // is delayed or scarce.
        while self.capture_buffer.len() >= frame_size
            && (recording_mode == RecordingMode::EchoCancel
                || self.render_mix_buffer.len() >= frame_size)
        {
            let capture_frame: Vec<f32> = self.capture_buffer.drain(0..frame_size).collect();
            // Only drain render_mix_buffer when it's needed for mixing.
            let render_frame: Vec<f32> = if recording_mode == RecordingMode::Mixed
                && self.render_mix_buffer.len() >= frame_size
            {
                self.render_mix_buffer.drain(0..frame_size).collect()
            } else {
                vec![0.0f32; frame_size]
            };

            // Apply AEC if enabled
            let processed_capture = if aec_enabled {
                if let Some(ref mut aec) = self.aec {
                    let mut out = vec![0.0f32; capture_frame.len()];

                    match aec.process_capture_frame(&capture_frame, false, &mut out) {
                        Ok(_metrics) => out,
                        Err(e) => {
                            tracing::error!("CoreAudio: AEC3 process_capture_frame error: {:?}", e);
                            capture_frame
                        }
                    }
                } else {
                    capture_frame
                }
            } else {
                capture_frame
            };

            // Generate output based on recording mode
            let output: Vec<f32> = match recording_mode {
                RecordingMode::Mixed => {
                    // Mix processed capture with system audio using soft clipping
                    processed_capture
                        .iter()
                        .zip(render_frame.iter())
                        .map(|(&s1, &s2)| {
                            let sum = s1 + s2;
                            if sum > 1.0 {
                                1.0 - (-2.0 * (sum - 1.0)).exp() * 0.5
                            } else if sum < -1.0 {
                                -1.0 + (-2.0 * (-sum - 1.0)).exp() * 0.5
                            } else {
                                sum
                            }
                        })
                        .collect()
                }
                RecordingMode::EchoCancel => {
                    // Output only the processed capture signal
                    processed_capture
                }
            };

            // Debug logging (periodic)
            static LOG_COUNTER: AtomicU32 = AtomicU32::new(0);
            let count = LOG_COUNTER.fetch_add(1, Ordering::Relaxed);
            if count.is_multiple_of(500) {
                let render_rms: f32 = if !render_frame.is_empty() {
                    (render_frame.iter().map(|s| s * s).sum::<f32>() / render_frame.len() as f32)
                        .sqrt()
                } else {
                    0.0
                };
                let out_rms: f32 = if !output.is_empty() {
                    (output.iter().map(|s| s * s).sum::<f32>() / output.len() as f32).sqrt()
                } else {
                    0.0
                };
                tracing::debug!(
                    "CoreAudio AudioMixer: mode={:?}, aec={}, render_rms={:.4}, out_rms={:.4}",
                    recording_mode,
                    aec_enabled,
                    render_rms,
                    out_rms
                );
            }

            // Send output
            let _ = self.output_tx.send(CoreAudioSamples {
                samples: output,
                channels: self.channels,
            });
        }
    }
}

/// Manager for multiple capture streams
struct MultiCaptureManager {
    /// CoreAudio input stream thread handle and stop flag
    input_stream: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
    /// ScreenCaptureKit system audio capture
    system_capture: Option<SCKAudioCapture>,
    /// Stop flag for system audio polling
    system_stop_flag: Arc<AtomicBool>,
    /// System audio polling thread
    system_thread: Option<JoinHandle<()>>,
}

impl MultiCaptureManager {
    fn new(
        source1_id: Option<String>,
        is_loopback1: bool,
        source2_id: Option<String>,
        is_loopback2: bool,
        stream_tx: mpsc::Sender<StreamSamples>,
    ) -> Result<Self, String> {
        let mut input_stream = None;
        let mut system_capture: Option<SCKAudioCapture> = None;
        let system_thread = None;
        let system_stop_flag = Arc::new(AtomicBool::new(false));

        // Start stream 1
        if let Some(device_id) = source1_id {
            if is_loopback1 {
                // System audio via ScreenCaptureKit
                let capture = SCKAudioCapture::new()?;
                capture.start()?;
                system_capture = Some(capture);
            } else {
                // Input device via CoreAudio
                let stop_flag = Arc::new(AtomicBool::new(false));
                let stop_flag_clone = Arc::clone(&stop_flag);
                let tx = stream_tx.clone();

                let handle = thread::spawn(move || {
                    run_input_capture(device_id, 1, tx, stop_flag_clone);
                });

                input_stream = Some((handle, stop_flag));
            }
        }

        // Start stream 2
        if let Some(device_id) = source2_id {
            if is_loopback2 {
                // System audio via ScreenCaptureKit
                if system_capture.is_none() {
                    let capture = SCKAudioCapture::new()?;
                    capture.start()?;
                    system_capture = Some(capture);
                }
            } else {
                // Input device via CoreAudio
                if input_stream.is_none() {
                    let stop_flag = Arc::new(AtomicBool::new(false));
                    let stop_flag_clone = Arc::clone(&stop_flag);
                    let tx = stream_tx;

                    let handle = thread::spawn(move || {
                        run_input_capture(device_id, 2, tx, stop_flag_clone);
                    });

                    input_stream = Some((handle, stop_flag));
                }
            }
        }

        Ok(Self {
            input_stream,
            system_capture,
            system_stop_flag,
            system_thread,
        })
    }

    /// Poll system audio capture for samples
    fn poll_system_audio(&self) -> Option<Vec<f32>> {
        if let Some(ref capture) = self.system_capture {
            if let Some(samples) = capture.try_recv() {
                return Some(samples.samples);
            }
        }
        None
    }
}

impl Drop for MultiCaptureManager {
    fn drop(&mut self) {
        // Signal streams to stop
        if let Some((_, ref stop_flag)) = self.input_stream {
            stop_flag.store(true, Ordering::SeqCst);
        }
        self.system_stop_flag.store(true, Ordering::SeqCst);

        // Stop system capture
        if let Some(ref capture) = self.system_capture {
            let _ = capture.stop();
        }

        // Wait for threads to finish
        if let Some((handle, _)) = self.input_stream.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.system_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Run capture thread for CoreAudio input
fn run_input_capture(
    device_id: String,
    stream_index: usize,
    stream_tx: mpsc::Sender<StreamSamples>,
    stop_flag: Arc<AtomicBool>,
) {
    tracing::info!(
        "CoreAudio: Input capture thread started (device={}, index={})",
        device_id,
        stream_index
    );

    let device_id: u32 = match device_id.parse() {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("CoreAudio: Invalid device ID: {}", device_id);
            return;
        }
    };

    // Create audio unit
    let audio_unit = match create_input_audio_unit(device_id) {
        Ok(unit) => unit,
        Err(e) => {
            tracing::error!("CoreAudio: Failed to create audio unit: {}", e);
            return;
        }
    };

    // Get stream format
    let (sample_rate, num_channels, is_non_interleaved) = match get_stream_format(audio_unit) {
        Ok(format) => format,
        Err(e) => {
            tracing::error!("CoreAudio: Failed to get stream format: {}", e);
            unsafe {
                sys::AudioComponentInstanceDispose(audio_unit);
            }
            return;
        }
    };

    // Create resampler if needed
    let needs_resampling = (sample_rate - TARGET_SAMPLE_RATE).abs() > 1.0;
    let resampler = if needs_resampling {
        Some(Mutex::new(Resampler::new(
            sample_rate as u32,
            TARGET_SAMPLE_RATE as u32,
        )))
    } else {
        None
    };

    // Create callback context
    let callback_context = Box::new(InputCallbackContext {
        audio_unit,
        audio_tx: stream_tx,
        resampler,
        num_channels,
        is_non_interleaved,
        stream_index,
    });
    let context_ptr = Box::into_raw(callback_context);

    // Set up the render callback
    let render_callback = sys::AURenderCallbackStruct {
        inputProc: Some(input_callback_proc),
        inputProcRefCon: context_ptr as *mut c_void,
    };

    let status = unsafe {
        sys::AudioUnitSetProperty(
            audio_unit,
            kAudioOutputUnitProperty_SetInputCallback,
            sys::kAudioUnitScope_Global,
            0,
            &render_callback as *const _ as *const c_void,
            std::mem::size_of::<sys::AURenderCallbackStruct>() as u32,
        )
    };

    if status != 0 {
        tracing::error!(
            "CoreAudio: Failed to set input callback: OSStatus {}",
            status
        );
        unsafe {
            let _ = Box::from_raw(context_ptr);
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return;
    }

    // Start the audio unit
    let status = unsafe { sys::AudioOutputUnitStart(audio_unit) };
    if status != 0 {
        tracing::error!("CoreAudio: Failed to start audio unit: OSStatus {}", status);
        unsafe {
            let _ = Box::from_raw(context_ptr);
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return;
    }

    tracing::info!("CoreAudio: Input capture started");

    // Wait for stop signal
    while !stop_flag.load(Ordering::SeqCst) {
        thread::sleep(std::time::Duration::from_millis(10));
    }

    // Stop and clean up
    unsafe {
        sys::AudioOutputUnitStop(audio_unit);
        sys::AudioComponentInstanceDispose(audio_unit);
        let _ = Box::from_raw(context_ptr);
    }

    tracing::info!("CoreAudio: Input capture stopped");
}

/// CoreAudio backend for macOS
pub struct CoreAudioBackend {
    /// Channel to send commands to capture thread
    cmd_tx: mpsc::Sender<CaptureCommand>,
    /// Channel to receive audio samples from capture thread
    audio_rx: Mutex<mpsc::Receiver<CoreAudioSamples>>,
    /// Cached input devices
    input_devices: Arc<Mutex<Vec<AudioDevice>>>,
    /// Cached system devices
    system_devices: Arc<Mutex<Vec<AudioDevice>>>,
    /// Sample rate (always 48kHz after resampling)
    sample_rate: u32,
    /// Capture thread handle
    _thread_handle: JoinHandle<()>,
    /// Flag indicating if capture is active
    #[allow(dead_code)]
    is_capturing: Arc<AtomicBool>,
    /// AEC enabled flag
    aec_enabled: Arc<Mutex<bool>>,
    /// Recording mode
    recording_mode: Arc<Mutex<RecordingMode>>,
}

impl CoreAudioBackend {
    /// Create a new CoreAudio backend
    pub fn new(
        aec_enabled: Arc<Mutex<bool>>,
        recording_mode: Arc<Mutex<RecordingMode>>,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (audio_tx, audio_rx) = mpsc::channel();
        let input_devices = Arc::new(Mutex::new(Vec::new()));
        let system_devices = Arc::new(Mutex::new(Vec::new()));
        let is_capturing = Arc::new(AtomicBool::new(false));

        // Enumerate devices
        let input_devs = enumerate_input_devices()?;
        let system_devs = enumerate_system_devices();

        *input_devices.lock().unwrap() = input_devs;
        *system_devices.lock().unwrap() = system_devs;

        let system_devices_clone = Arc::clone(&system_devices);
        let is_capturing_clone = Arc::clone(&is_capturing);
        let aec_enabled_clone = Arc::clone(&aec_enabled);
        let recording_mode_clone = Arc::clone(&recording_mode);

        let thread_handle = thread::spawn(move || {
            run_capture_thread(
                cmd_rx,
                audio_tx,
                system_devices_clone,
                is_capturing_clone,
                aec_enabled_clone,
                recording_mode_clone,
            );
        });

        Ok(Self {
            cmd_tx,
            audio_rx: Mutex::new(audio_rx),
            input_devices,
            system_devices,
            sample_rate: TARGET_SAMPLE_RATE as u32,
            _thread_handle: thread_handle,
            is_capturing,
            aec_enabled,
            recording_mode,
        })
    }
}

impl Drop for CoreAudioBackend {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(CaptureCommand::Shutdown);
    }
}

impl AudioBackend for CoreAudioBackend {
    fn list_input_devices(&self) -> Vec<AudioDevice> {
        self.input_devices.lock().unwrap().clone()
    }

    fn list_system_devices(&self) -> Vec<AudioDevice> {
        self.system_devices.lock().unwrap().clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn start_capture_sources(
        &self,
        source1_id: Option<String>,
        source2_id: Option<String>,
    ) -> Result<(), String> {
        let (result_tx, result_rx) = mpsc::channel();

        self.cmd_tx
            .send(CaptureCommand::StartSources {
                source1_id,
                source2_id,
                result_tx,
            })
            .map_err(|e| format!("Failed to send start command: {}", e))?;

        match result_rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("Timeout waiting for audio capture to start".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Capture thread disconnected".to_string())
            }
        }
    }

    fn stop_capture(&self) -> Result<(), String> {
        self.cmd_tx
            .send(CaptureCommand::Stop)
            .map_err(|e| format!("Failed to send stop command: {}", e))?;
        Ok(())
    }

    fn try_recv(&self) -> Option<AudioData> {
        self.audio_rx
            .lock()
            .unwrap()
            .try_recv()
            .ok()
            .map(|samples| AudioData {
                samples: samples.samples,
                channels: samples.channels,
                sample_rate: self.sample_rate,
            })
    }

    fn set_aec_enabled(&self, enabled: bool) {
        *self.aec_enabled.lock().unwrap() = enabled;
    }

    fn set_recording_mode(&self, mode: RecordingMode) {
        *self.recording_mode.lock().unwrap() = mode;
    }
}

/// Run the capture thread
fn run_capture_thread(
    cmd_rx: mpsc::Receiver<CaptureCommand>,
    audio_tx: mpsc::Sender<CoreAudioSamples>,
    system_devices: Arc<Mutex<Vec<AudioDevice>>>,
    is_capturing: Arc<AtomicBool>,
    aec_enabled: Arc<Mutex<bool>>,
    recording_mode: Arc<Mutex<RecordingMode>>,
) {
    tracing::debug!("CoreAudio: Capture thread started and ready to receive commands");

    // Create mixer (owned by this thread)
    let mut mixer = AudioMixer::new(audio_tx, aec_enabled, recording_mode);

    // Channel for receiving samples from stream threads
    let (stream_tx, stream_rx) = mpsc::channel::<StreamSamples>();

    // Active capture state
    let mut capture_manager: Option<MultiCaptureManager> = None;

    loop {
        // Process any samples from stream threads first
        while let Ok(stream_samples) = stream_rx.try_recv() {
            mixer.push_samples(&stream_samples.samples, stream_samples.is_loopback);
        }

        // Poll system audio if we have an active capture
        if let Some(ref manager) = capture_manager {
            if let Some(samples) = manager.poll_system_audio() {
                mixer.push_samples(&samples, true); // is_loopback = true for system audio
            }
        }

        let timeout = if capture_manager.is_some() {
            std::time::Duration::from_millis(1)
        } else {
            std::time::Duration::from_secs(1)
        };

        match cmd_rx.recv_timeout(timeout) {
            Ok(CaptureCommand::StartSources {
                source1_id,
                source2_id,
                result_tx,
            }) => {
                tracing::info!(
                    "[CaptureThread] Received StartSources command: source1={:?}, source2={:?}",
                    source1_id,
                    source2_id
                );

                // Stop any existing capture
                if let Some(manager) = capture_manager.take() {
                    drop(manager);
                }

                // Determine which sources are loopback (system audio)
                let system_ids: HashSet<String> = system_devices
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|d| d.id.clone())
                    .collect();

                let is_loopback1 = source1_id
                    .as_ref()
                    .map(|id| system_ids.contains(id) || id.starts_with(SYSTEM_AUDIO_PREFIX))
                    .unwrap_or(false);
                let is_loopback2 = source2_id
                    .as_ref()
                    .map(|id| system_ids.contains(id) || id.starts_with(SYSTEM_AUDIO_PREFIX))
                    .unwrap_or(false);

                // Count streams
                let num_streams = source1_id.is_some() as usize + source2_id.is_some() as usize;
                mixer.set_num_streams(num_streams);

                // Start capture
                tracing::debug!(
                    "CoreAudio: Starting MultiCaptureManager (source1={:?}, loopback1={}, source2={:?}, loopback2={})",
                    source1_id, is_loopback1, source2_id, is_loopback2
                );
                match MultiCaptureManager::new(
                    source1_id,
                    is_loopback1,
                    source2_id,
                    is_loopback2,
                    stream_tx.clone(),
                ) {
                    Ok(manager) => {
                        tracing::info!("CoreAudio: Started capture with {} sources", num_streams);
                        is_capturing.store(true, Ordering::SeqCst);
                        capture_manager = Some(manager);
                        let _ = result_tx.send(Ok(()));
                    }
                    Err(e) => {
                        tracing::error!("CoreAudio: Failed to start capture: {}", e);
                        is_capturing.store(false, Ordering::SeqCst);
                        let _ = result_tx.send(Err(e));
                    }
                }
            }
            Ok(CaptureCommand::Stop) => {
                if let Some(manager) = capture_manager.take() {
                    tracing::info!("CoreAudio: Stopping capture");
                    drop(manager);
                }
                mixer.set_num_streams(0);
                is_capturing.store(false, Ordering::SeqCst);
            }
            Ok(CaptureCommand::Shutdown) => {
                if let Some(manager) = capture_manager.take() {
                    drop(manager);
                }
                is_capturing.store(false, Ordering::SeqCst);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Continue processing samples
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    tracing::debug!("CoreAudio: Capture thread exiting");
}

/// Create an input audio unit for the given device ID
fn create_input_audio_unit(device_id: u32) -> Result<sys::AudioUnit, String> {
    let desc = sys::AudioComponentDescription {
        componentType: sys::kAudioUnitType_Output,
        componentSubType: sys::kAudioUnitSubType_HALOutput,
        componentManufacturer: sys::kAudioUnitManufacturer_Apple,
        componentFlags: 0,
        componentFlagsMask: 0,
    };

    let component = unsafe { sys::AudioComponentFindNext(ptr::null_mut(), &desc) };
    if component.is_null() {
        return Err("Failed to find HAL Output audio component".to_string());
    }

    let mut audio_unit: sys::AudioUnit = ptr::null_mut();
    let status = unsafe { sys::AudioComponentInstanceNew(component, &mut audio_unit) };
    if status != 0 {
        return Err(format!(
            "Failed to create audio unit instance: OSStatus {}",
            status
        ));
    }

    // Enable input on element 1
    let enable_input: u32 = 1;
    let status = unsafe {
        sys::AudioUnitSetProperty(
            audio_unit,
            sys::kAudioOutputUnitProperty_EnableIO,
            sys::kAudioUnitScope_Input,
            1,
            &enable_input as *const _ as *const c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if status != 0 {
        unsafe {
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return Err(format!("Failed to enable input: OSStatus {}", status));
    }

    // Disable output on element 0
    let disable_output: u32 = 0;
    let status = unsafe {
        sys::AudioUnitSetProperty(
            audio_unit,
            sys::kAudioOutputUnitProperty_EnableIO,
            sys::kAudioUnitScope_Output,
            0,
            &disable_output as *const _ as *const c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if status != 0 {
        unsafe {
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return Err(format!("Failed to disable output: OSStatus {}", status));
    }

    // Set the input device
    let status = unsafe {
        sys::AudioUnitSetProperty(
            audio_unit,
            sys::kAudioOutputUnitProperty_CurrentDevice,
            sys::kAudioUnitScope_Global,
            0,
            &device_id as *const _ as *const c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if status != 0 {
        unsafe {
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return Err(format!("Failed to set device: OSStatus {}", status));
    }

    // Get the input format from the device
    let mut device_format: sys::AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<sys::AudioStreamBasicDescription>() as u32;
    let status = unsafe {
        sys::AudioUnitGetProperty(
            audio_unit,
            kAudioUnitProperty_StreamFormat,
            sys::kAudioUnitScope_Input,
            1,
            &mut device_format as *mut _ as *mut c_void,
            &mut size,
        )
    };
    if status != 0 {
        unsafe {
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return Err(format!("Failed to get device format: OSStatus {}", status));
    }

    // Set the output format to match the device
    let status = unsafe {
        sys::AudioUnitSetProperty(
            audio_unit,
            kAudioUnitProperty_StreamFormat,
            sys::kAudioUnitScope_Output,
            1,
            &device_format as *const _ as *const c_void,
            std::mem::size_of::<sys::AudioStreamBasicDescription>() as u32,
        )
    };
    if status != 0 {
        unsafe {
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return Err(format!("Failed to set output format: OSStatus {}", status));
    }

    // Initialize the audio unit
    let status = unsafe { sys::AudioUnitInitialize(audio_unit) };
    if status != 0 {
        unsafe {
            sys::AudioComponentInstanceDispose(audio_unit);
        }
        return Err(format!(
            "Failed to initialize audio unit: OSStatus {}",
            status
        ));
    }

    Ok(audio_unit)
}

/// Get the stream format for an audio unit's input
fn get_stream_format(audio_unit: sys::AudioUnit) -> Result<(f64, usize, bool), String> {
    let mut asbd: sys::AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<sys::AudioStreamBasicDescription>() as u32;

    let status = unsafe {
        sys::AudioUnitGetProperty(
            audio_unit,
            kAudioUnitProperty_StreamFormat,
            sys::kAudioUnitScope_Output,
            1,
            &mut asbd as *mut _ as *mut c_void,
            &mut size,
        )
    };

    if status != 0 {
        return Err(format!("Failed to get stream format: OSStatus {}", status));
    }

    let is_non_interleaved = (asbd.mFormatFlags & sys::kAudioFormatFlagIsNonInterleaved) != 0;
    let num_channels = asbd.mChannelsPerFrame as usize;

    Ok((asbd.mSampleRate, num_channels, is_non_interleaved))
}

/// Enumerate available input devices
fn enumerate_input_devices() -> Result<Vec<AudioDevice>, String> {
    let device_ids =
        get_audio_device_ids().map_err(|e| format!("Failed to get audio devices: {:?}", e))?;

    let default_input_id = get_default_device_id(true);

    let mut input_devices = Vec::new();

    for device_id in device_ids {
        let supports_input =
            get_audio_device_supports_scope(device_id, Scope::Input).unwrap_or(false);

        if supports_input {
            let name = get_device_name(device_id)
                .unwrap_or_else(|_| format!("Unknown Device {}", device_id));

            input_devices.push(AudioDevice {
                id: device_id.to_string(),
                name,
                source_type: AudioSourceType::Input,
            });
        }
    }

    // Sort so default device is first
    if let Some(default_id) = default_input_id {
        let default_id_str = default_id.to_string();
        input_devices.sort_by(|a, b| {
            let a_is_default = a.id == default_id_str;
            let b_is_default = b.id == default_id_str;
            b_is_default.cmp(&a_is_default)
        });
    }

    Ok(input_devices)
}

/// Enumerate available system audio devices (via ScreenCaptureKit)
fn enumerate_system_devices() -> Vec<AudioDevice> {
    if !screencapturekit::is_available() {
        return Vec::new();
    }

    match screencapturekit::enumerate_system_devices() {
        Ok(devices) => devices
            .into_iter()
            .map(|d| AudioDevice {
                id: format!("{}{}", SYSTEM_AUDIO_PREFIX, d.id),
                name: d.name,
                source_type: AudioSourceType::System,
            })
            .collect(),
        Err(e) => {
            tracing::error!("CoreAudio: Failed to enumerate system devices: {}", e);
            Vec::new()
        }
    }
}

/// Simple linear resampler
struct Resampler {
    source_rate: u32,
    target_rate: u32,
    buffer: Vec<f32>,
    position: f64,
}

impl Resampler {
    fn new(source_rate: u32, target_rate: u32) -> Self {
        Self {
            source_rate,
            target_rate,
            buffer: Vec::new(),
            position: 0.0,
        }
    }

    fn process(&mut self, samples: &[f32], channels: usize) -> Vec<f32> {
        self.buffer.extend_from_slice(samples);

        let ratio = self.source_rate as f64 / self.target_rate as f64;
        let input_frames = self.buffer.len() / channels;
        let output_frames = ((input_frames as f64 - self.position) / ratio) as usize;

        if output_frames == 0 {
            return Vec::new();
        }

        let mut output = Vec::with_capacity(output_frames * channels);

        for _ in 0..output_frames {
            let src_frame = self.position as usize;
            let frac = self.position - src_frame as f64;

            for ch in 0..channels {
                let idx0 = src_frame * channels + ch;
                let idx1 = (src_frame + 1) * channels + ch;

                let sample = if idx1 < self.buffer.len() {
                    self.buffer[idx0] * (1.0 - frac as f32) + self.buffer[idx1] * frac as f32
                } else if idx0 < self.buffer.len() {
                    self.buffer[idx0]
                } else {
                    0.0
                };
                output.push(sample);
            }

            self.position += ratio;
        }

        let consumed_frames = self.position as usize;
        if consumed_frames > 0 {
            let consumed_samples = consumed_frames * channels;
            if consumed_samples < self.buffer.len() {
                self.buffer.drain(0..consumed_samples);
                self.position -= consumed_frames as f64;
            } else {
                self.buffer.clear();
                self.position = 0.0;
            }
        }

        output
    }
}

/// Create a macOS CoreAudio backend
pub fn create_backend(
    aec_enabled: Arc<Mutex<bool>>,
    recording_mode: Arc<Mutex<RecordingMode>>,
) -> Result<Box<dyn AudioBackend>, String> {
    let backend = CoreAudioBackend::new(aec_enabled, recording_mode)?;
    Ok(Box::new(backend))
}
