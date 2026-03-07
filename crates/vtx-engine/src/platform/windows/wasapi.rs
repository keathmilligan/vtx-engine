//! WASAPI audio backend for Windows
//!
//! This module provides full audio capture functionality using Windows Audio Session API (WASAPI):
//! - Input device capture (microphones)
//! - System audio capture (loopback from render endpoints)
//! - Multi-source capture with mixing
//! - Echo cancellation using AEC3

use crate::platform::backend::{AudioBackend, AudioData};
use crate::{AudioDevice, AudioSourceType, RecordingMode};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use aec3::voip::VoipAec3;
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eRender, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceCollection,
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

/// WAVE_FORMAT_EXTENSIBLE constant (0xFFFE)
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// WAVE_FORMAT_PCM constant (1)
const WAVE_FORMAT_PCM: u16 = 1;

/// WAVE_FORMAT_IEEE_FLOAT constant (3)
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT GUID
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// Target sample rate for output (matches Linux backend)
const TARGET_SAMPLE_RATE: u32 = 48000;

/// AEC3 frame size: 10ms at 48kHz = 480 samples per channel
const AEC_FRAME_SAMPLES: usize = 480;

/// Internal audio samples for channel communication
struct WasapiAudioSamples {
    samples: Vec<f32>,
    channels: u16,
}

/// Samples from a stream thread to the mixer
struct StreamSamples {
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

/// WASAPI audio backend for Windows
pub struct WasapiBackend {
    /// Channel to send commands to capture thread
    cmd_tx: mpsc::Sender<CaptureCommand>,
    /// Channel to receive audio samples from capture thread (wrapped in Mutex for Sync)
    audio_rx: Mutex<mpsc::Receiver<WasapiAudioSamples>>,
    /// Cached input devices
    input_devices: Arc<Mutex<Vec<AudioDevice>>>,
    /// Cached system devices (loopback sources)
    system_devices: Arc<Mutex<Vec<AudioDevice>>>,
    /// Sample rate (always 48kHz after resampling)
    sample_rate: u32,
    /// Capture thread handle
    _thread_handle: JoinHandle<()>,
    /// AEC enabled flag (shared with mixer)
    aec_enabled: Arc<Mutex<bool>>,
    /// Recording mode (shared with mixer)
    recording_mode: Arc<Mutex<RecordingMode>>,
}

impl WasapiBackend {
    /// Create a new WASAPI backend
    pub fn new(
        aec_enabled: Arc<Mutex<bool>>,
        recording_mode: Arc<Mutex<RecordingMode>>,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (audio_tx, audio_rx) = mpsc::channel();
        let input_devices = Arc::new(Mutex::new(Vec::new()));
        let system_devices = Arc::new(Mutex::new(Vec::new()));
        let is_capturing = Arc::new(AtomicBool::new(false));

        // Initialize COM on this thread if not already initialized
        let com_initialized = unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            hr.is_ok()
        };

        // Enumerate devices
        let input_devs = enumerate_input_devices();
        let system_devs = enumerate_render_devices();

        // Uninitialize COM if we initialized it
        if com_initialized {
            unsafe {
                CoUninitialize();
            }
        }

        // Store devices
        let input_devs = input_devs?;
        let system_devs = system_devs?;
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
            sample_rate: TARGET_SAMPLE_RATE,
            _thread_handle: thread_handle,
            aec_enabled,
            recording_mode,
        })
    }
}

impl Drop for WasapiBackend {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(CaptureCommand::Shutdown);
    }
}

impl AudioBackend for WasapiBackend {
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

        match result_rx.recv_timeout(std::time::Duration::from_secs(5)) {
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

/// Create a Windows audio backend using WASAPI
pub fn create_backend(
    aec_enabled: Arc<Mutex<bool>>,
    recording_mode: Arc<Mutex<RecordingMode>>,
) -> Result<Box<dyn AudioBackend>, String> {
    let backend = WasapiBackend::new(aec_enabled, recording_mode)?;
    Ok(Box::new(backend))
}

/// Enumerate available input devices (microphones)
fn enumerate_input_devices() -> Result<Vec<AudioDevice>, String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

        let collection: IMMDeviceCollection = enumerator
            .EnumAudioEndpoints(eCapture, windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("Failed to enumerate audio endpoints: {}", e))?;

        let count = collection
            .GetCount()
            .map_err(|e| format!("Failed to get device count: {}", e))?;

        let mut devices = Vec::new();

        for i in 0..count {
            if let Ok(device) = collection.Item(i) {
                if let Some(platform_device) =
                    device_to_audio_device(&device, AudioSourceType::Input)
                {
                    devices.push(platform_device);
                }
            }
        }

        Ok(devices)
    }
}

/// Enumerate available render devices (for loopback capture)
fn enumerate_render_devices() -> Result<Vec<AudioDevice>, String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

        let collection: IMMDeviceCollection = enumerator
            .EnumAudioEndpoints(eRender, windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("Failed to enumerate render endpoints: {}", e))?;

        let count = collection
            .GetCount()
            .map_err(|e| format!("Failed to get render device count: {}", e))?;

        let mut devices = Vec::new();

        for i in 0..count {
            if let Ok(device) = collection.Item(i) {
                if let Some(mut platform_device) =
                    device_to_audio_device(&device, AudioSourceType::System)
                {
                    // Add (Loopback) suffix to distinguish from input devices
                    platform_device.name = format!("{} (Loopback)", platform_device.name);
                    devices.push(platform_device);
                }
            }
        }

        Ok(devices)
    }
}

/// Convert an IMMDevice to an AudioDevice
fn device_to_audio_device(device: &IMMDevice, source_type: AudioSourceType) -> Option<AudioDevice> {
    unsafe {
        let id_ptr: PWSTR = device.GetId().ok()?;
        let id = pwstr_to_string(id_ptr);
        windows::Win32::System::Com::CoTaskMemFree(Some(id_ptr.0 as *const _));

        let props: IPropertyStore = device.OpenPropertyStore(STGM_READ).ok()?;
        let prop_variant = props.GetValue(&PKEY_Device_FriendlyName).ok()?;

        let name = {
            let name_str = prop_variant.to_string();
            if name_str.is_empty() {
                "Unknown Device".to_string()
            } else {
                name_str
            }
        };

        Some(AudioDevice {
            id,
            name,
            source_type,
        })
    }
}

/// Convert a PWSTR to a Rust String
fn pwstr_to_string(pwstr: PWSTR) -> String {
    unsafe {
        if pwstr.0.is_null() {
            return String::new();
        }
        let len = (0..).take_while(|&i| *pwstr.0.add(i) != 0).count();
        let slice = std::slice::from_raw_parts(pwstr.0, len);
        String::from_utf16_lossy(slice)
    }
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
    output_tx: mpsc::Sender<WasapiAudioSamples>,
    /// Flag to enable/disable AEC (shared with main thread)
    aec_enabled: Arc<Mutex<bool>>,
    /// Recording mode - Mixed or EchoCancel (shared with main thread)
    recording_mode: Arc<Mutex<RecordingMode>>,
    /// AEC3 pipeline (created when in mixed mode with 2 streams)
    aec: Option<VoipAec3>,
}

impl AudioMixer {
    fn new(
        output_tx: mpsc::Sender<WasapiAudioSamples>,
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
                        "WASAPI: AEC3 initialized: 48kHz, {} channels, {}ms frames",
                        self.channels,
                        AEC_FRAME_SAMPLES * 1000 / 48000
                    );
                    self.aec = Some(aec);
                }
                Err(e) => {
                    tracing::error!("WASAPI: Failed to initialize AEC3: {:?}", e);
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
            let _ = self.output_tx.send(WasapiAudioSamples {
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
                        tracing::error!("WASAPI: AEC3 handle_render_frame error: {:?}", e);
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
                            tracing::error!("WASAPI: AEC3 process_capture_frame error: {:?}", e);
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
                    "WASAPI AudioMixer: mode={:?}, aec={}, render_rms={:.4}, out_rms={:.4}",
                    recording_mode,
                    aec_enabled,
                    render_rms,
                    out_rms
                );
            }

            // Send output
            let _ = self.output_tx.send(WasapiAudioSamples {
                samples: output,
                channels: self.channels,
            });
        }
    }
}

/// Run the capture thread
fn run_capture_thread(
    cmd_rx: mpsc::Receiver<CaptureCommand>,
    audio_tx: mpsc::Sender<WasapiAudioSamples>,
    system_devices: Arc<Mutex<Vec<AudioDevice>>>,
    is_capturing: Arc<AtomicBool>,
    aec_enabled: Arc<Mutex<bool>>,
    recording_mode: Arc<Mutex<RecordingMode>>,
) {
    tracing::info!("WASAPI: Capture thread started");

    unsafe {
        let com_result = CoInitializeEx(None, COINIT_MULTITHREADED);
        if com_result.is_err() {
            tracing::error!(
                "WASAPI: Failed to initialize COM on capture thread: {:?}",
                com_result
            );
            while let Ok(cmd) = cmd_rx.try_recv() {
                if let CaptureCommand::StartSources { result_tx, .. } = cmd {
                    let _ =
                        result_tx.send(Err(format!("COM initialization failed: {:?}", com_result)));
                }
            }
            return;
        }
        tracing::debug!("WASAPI: COM initialized on capture thread");

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
                    // Stop any existing capture
                    if let Some(manager) = capture_manager.take() {
                        drop(manager);
                    }

                    // Determine which sources are loopback (system audio)
                    let system_ids: std::collections::HashSet<String> = system_devices
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|d| d.id.clone())
                        .collect();

                    let is_loopback1 = source1_id
                        .as_ref()
                        .map(|id| system_ids.contains(id))
                        .unwrap_or(false);
                    let is_loopback2 = source2_id
                        .as_ref()
                        .map(|id| system_ids.contains(id))
                        .unwrap_or(false);

                    // Count streams
                    let num_streams = source1_id.is_some() as usize + source2_id.is_some() as usize;
                    mixer.set_num_streams(num_streams);

                    // Start capture
                    match MultiCaptureManager::new(
                        source1_id,
                        is_loopback1,
                        source2_id,
                        is_loopback2,
                        stream_tx.clone(),
                    ) {
                        Ok(manager) => {
                            tracing::info!("WASAPI: Started capture with {} sources", num_streams);
                            is_capturing.store(true, Ordering::SeqCst);
                            capture_manager = Some(manager);
                            let _ = result_tx.send(Ok(()));
                        }
                        Err(e) => {
                            tracing::error!("WASAPI: Failed to start capture: {}", e);
                            is_capturing.store(false, Ordering::SeqCst);
                            let _ = result_tx.send(Err(e));
                        }
                    }
                }
                Ok(CaptureCommand::Stop) => {
                    if let Some(manager) = capture_manager.take() {
                        tracing::info!("WASAPI: Stopping capture");
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

        CoUninitialize();
    }
}

/// Manager for multiple capture streams
struct MultiCaptureManager {
    /// Stream 1 thread handle and stop flag
    stream1: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
    /// Stream 2 thread handle and stop flag
    stream2: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
}

impl MultiCaptureManager {
    fn new(
        source1_id: Option<String>,
        is_loopback1: bool,
        source2_id: Option<String>,
        is_loopback2: bool,
        stream_tx: mpsc::Sender<StreamSamples>,
    ) -> Result<Self, String> {
        let mut stream1 = None;
        let mut stream2 = None;

        // Start stream 1 if specified
        if let Some(device_id) = source1_id {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_flag_clone = Arc::clone(&stop_flag);
            let tx = stream_tx.clone();

            let handle = thread::spawn(move || {
                run_stream_capture(device_id, is_loopback1, 1, tx, stop_flag_clone);
            });

            stream1 = Some((handle, stop_flag));
        }

        // Start stream 2 if specified
        if let Some(device_id) = source2_id {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_flag_clone = Arc::clone(&stop_flag);
            let tx = stream_tx;

            let handle = thread::spawn(move || {
                run_stream_capture(device_id, is_loopback2, 2, tx, stop_flag_clone);
            });

            stream2 = Some((handle, stop_flag));
        }

        Ok(Self { stream1, stream2 })
    }
}

impl Drop for MultiCaptureManager {
    fn drop(&mut self) {
        // Signal streams to stop
        if let Some((_, ref stop_flag)) = self.stream1 {
            stop_flag.store(true, Ordering::SeqCst);
        }
        if let Some((_, ref stop_flag)) = self.stream2 {
            stop_flag.store(true, Ordering::SeqCst);
        }

        // Wait for threads to finish
        if let Some((handle, _)) = self.stream1.take() {
            let _ = handle.join();
        }
        if let Some((handle, _)) = self.stream2.take() {
            let _ = handle.join();
        }
    }
}

/// Run capture for a single stream
fn run_stream_capture(
    device_id: String,
    is_loopback: bool,
    stream_index: usize,
    stream_tx: mpsc::Sender<StreamSamples>,
    stop_flag: Arc<AtomicBool>,
) {
    tracing::info!(
        "WASAPI: Stream {} capture thread started (device={}, loopback={})",
        stream_index,
        device_id,
        is_loopback
    );

    unsafe {
        // Initialize COM for this thread
        let com_result = CoInitializeEx(None, COINIT_MULTITHREADED);
        if com_result.is_err() {
            tracing::error!(
                "WASAPI: Stream {} failed to initialize COM: {:?}",
                stream_index,
                com_result
            );
            return;
        }

        // Start capture
        match start_capture(&device_id, is_loopback) {
            Ok(mut state) => {
                tracing::info!(
                    "WASAPI: Stream {} capture started from device {}",
                    stream_index,
                    device_id
                );

                // Capture loop
                while !stop_flag.load(Ordering::SeqCst) {
                    if let Err(e) = process_capture(&mut state, is_loopback, &stream_tx) {
                        tracing::error!("WASAPI: Stream {} capture error: {}", stream_index, e);
                        break;
                    }
                }

                tracing::info!("WASAPI: Stream {} capture stopped", stream_index);
            }
            Err(e) => {
                tracing::error!(
                    "WASAPI: Stream {} failed to start capture: {}",
                    stream_index,
                    e
                );
            }
        }

        CoUninitialize();
    }
}

/// State for an active capture session
struct CaptureState {
    audio_client: IAudioClient,
    capture_client: IAudioCaptureClient,
    format: CaptureFormat,
    event_handle: windows::Win32::Foundation::HANDLE,
    resampler: Option<Resampler>,
}

impl Drop for CaptureState {
    fn drop(&mut self) {
        unsafe {
            let _ = self.audio_client.Stop();
            if !self.event_handle.is_invalid() {
                let _ = windows::Win32::Foundation::CloseHandle(self.event_handle);
            }
        }
    }
}

/// Format information for captured audio
struct CaptureFormat {
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    is_float: bool,
}

/// Start capturing from a device
unsafe fn start_capture(device_id: &str, is_loopback: bool) -> Result<CaptureState, String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

    let device_id_wide: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
    let device: IMMDevice = enumerator
        .GetDevice(PCWSTR(device_id_wide.as_ptr()))
        .map_err(|e| format!("Failed to get device {}: {}", device_id, e))?;

    let audio_client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|e| format!("Failed to activate audio client: {}", e))?;

    let mix_format_ptr = audio_client
        .GetMixFormat()
        .map_err(|e| format!("Failed to get mix format: {}", e))?;

    let mix_format = &*mix_format_ptr;
    let format = parse_wave_format(mix_format)?;

    tracing::debug!(
        "WASAPI: Device format: {}Hz, {} channels, {} bits, float={}, loopback={}",
        format.sample_rate,
        format.channels,
        format.bits_per_sample,
        format.is_float,
        is_loopback
    );

    let event_handle = CreateEventW(None, false, false, None)
        .map_err(|e| format!("Failed to create event: {}", e))?;

    let buffer_duration: i64 = 1_000_000; // 100ms

    // Use loopback flag for system audio capture
    let stream_flags = if is_loopback {
        AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
    } else {
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK
    };

    audio_client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            stream_flags,
            buffer_duration,
            0,
            mix_format_ptr,
            None,
        )
        .map_err(|e| format!("Failed to initialize audio client: {}", e))?;

    audio_client
        .SetEventHandle(event_handle)
        .map_err(|e| format!("Failed to set event handle: {}", e))?;

    let capture_client: IAudioCaptureClient = audio_client
        .GetService()
        .map_err(|e| format!("Failed to get capture client: {}", e))?;

    let resampler = if format.sample_rate != TARGET_SAMPLE_RATE {
        Some(Resampler::new(format.sample_rate, TARGET_SAMPLE_RATE))
    } else {
        None
    };

    audio_client
        .Start()
        .map_err(|e| format!("Failed to start capture: {}", e))?;

    windows::Win32::System::Com::CoTaskMemFree(Some(mix_format_ptr as *const _ as *const _));

    Ok(CaptureState {
        audio_client,
        capture_client,
        format,
        event_handle,
        resampler,
    })
}

/// Parse WAVEFORMATEX into CaptureFormat
fn parse_wave_format(format: &WAVEFORMATEX) -> Result<CaptureFormat, String> {
    let is_float;
    let bits_per_sample;

    let format_tag = format.wFormatTag;
    let sample_rate = format.nSamplesPerSec;
    let channels = format.nChannels;
    let bits = format.wBitsPerSample;

    if format_tag == WAVE_FORMAT_EXTENSIBLE {
        let ext = unsafe { &*(format as *const WAVEFORMATEX as *const WAVEFORMATEXTENSIBLE) };
        let sub_format = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(ext.SubFormat)) };
        let valid_bits = unsafe {
            std::ptr::read_unaligned(std::ptr::addr_of!(ext.Samples.wValidBitsPerSample))
        };
        is_float = sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
        bits_per_sample = valid_bits;
    } else if format_tag == WAVE_FORMAT_IEEE_FLOAT {
        is_float = true;
        bits_per_sample = bits;
    } else if format_tag == WAVE_FORMAT_PCM {
        is_float = false;
        bits_per_sample = bits;
    } else {
        return Err(format!("Unsupported audio format tag: {}", format_tag));
    }

    Ok(CaptureFormat {
        sample_rate,
        channels,
        bits_per_sample,
        is_float,
    })
}

/// Process captured audio data
unsafe fn process_capture(
    state: &mut CaptureState,
    is_loopback: bool,
    stream_tx: &mpsc::Sender<StreamSamples>,
) -> Result<(), String> {
    let wait_result = WaitForSingleObject(state.event_handle, 10);
    if wait_result.0 != 0 {
        return Ok(());
    }

    loop {
        let mut buffer_ptr: *mut u8 = std::ptr::null_mut();
        let mut num_frames: u32 = 0;
        let mut flags: u32 = 0;

        let result = state.capture_client.GetBuffer(
            &mut buffer_ptr,
            &mut num_frames,
            &mut flags,
            None,
            None,
        );

        if result.is_err() || num_frames == 0 {
            break;
        }

        let samples = convert_to_f32(buffer_ptr, num_frames as usize, &state.format);

        let _ = state.capture_client.ReleaseBuffer(num_frames);

        if samples.is_empty() {
            continue;
        }

        // Resample if needed
        let final_samples = if let Some(ref mut resampler) = state.resampler {
            resampler.process(&samples, state.format.channels as usize)
        } else {
            samples
        };

        // Convert mono to stereo if needed
        let stereo_samples = if state.format.channels == 1 {
            mono_to_stereo(&final_samples)
        } else {
            final_samples
        };

        // Send to mixer thread via channel with loopback flag
        let _ = stream_tx.send(StreamSamples {
            samples: stereo_samples,
            is_loopback,
        });
    }

    Ok(())
}

/// Convert raw audio buffer to f32 samples
unsafe fn convert_to_f32(buffer: *const u8, num_frames: usize, format: &CaptureFormat) -> Vec<f32> {
    let num_samples = num_frames * format.channels as usize;

    if format.is_float && format.bits_per_sample == 32 {
        let f32_ptr = buffer as *const f32;
        std::slice::from_raw_parts(f32_ptr, num_samples).to_vec()
    } else if !format.is_float && format.bits_per_sample == 16 {
        let i16_ptr = buffer as *const i16;
        let i16_slice = std::slice::from_raw_parts(i16_ptr, num_samples);
        i16_slice.iter().map(|&s| s as f32 / 32768.0).collect()
    } else if !format.is_float && format.bits_per_sample == 24 {
        let mut samples = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let offset = i * 3;
            let b0 = *buffer.add(offset) as i32;
            let b1 = *buffer.add(offset + 1) as i32;
            let b2 = *buffer.add(offset + 2) as i32;
            let value = (b0 | (b1 << 8) | (b2 << 16)) << 8 >> 8;
            samples.push(value as f32 / 8388608.0);
        }
        samples
    } else if !format.is_float && format.bits_per_sample == 32 {
        let i32_ptr = buffer as *const i32;
        let i32_slice = std::slice::from_raw_parts(i32_ptr, num_samples);
        i32_slice.iter().map(|&s| s as f32 / 2147483648.0).collect()
    } else {
        tracing::error!(
            "WASAPI: Unsupported format: float={}, bits={}",
            format.is_float,
            format.bits_per_sample
        );
        Vec::new()
    }
}

/// Convert mono audio to stereo by duplicating channels
fn mono_to_stereo(mono: &[f32]) -> Vec<f32> {
    let mut stereo = Vec::with_capacity(mono.len() * 2);
    for &sample in mono {
        stereo.push(sample);
        stereo.push(sample);
    }
    stereo
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
