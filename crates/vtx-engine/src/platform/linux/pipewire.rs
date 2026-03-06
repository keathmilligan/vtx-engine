//! PipeWire-based audio capture backend for Linux
//!
//! This module provides audio capture from input devices and system audio (sink monitors)
//! using PipeWire directly. It integrates with the existing audio processing pipeline.
//! When capturing from multiple sources, they are mixed together before being sent
//! to the processing pipeline.

use pipewire::{
    context::Context,
    main_loop::MainLoop,
    properties::properties,
    spa::{
        param::audio::{AudioFormat, AudioInfoRaw},
        pod::Pod,
        utils::Direction,
    },
    stream::{Stream, StreamFlags},
    types::ObjectType,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::platform::backend::{AudioBackend, AudioData};
use crate::{AudioDevice, AudioSourceType, RecordingMode};
use aec3::voip::VoipAec3;

/// Commands sent to the PipeWire thread
#[derive(Debug)]
enum PwCommand {
    /// Start capturing from up to two sources (mixed together)
    StartCaptureSources {
        source1_id: Option<u32>,
        source2_id: Option<u32>,
    },
    /// Stop all capture
    StopCapture,
}

/// Internal audio samples type for PipeWire thread communication
struct PwAudioSamples {
    samples: Vec<f32>,
    channels: u16,
}

/// Handle to the PipeWire audio backend
pub struct PipeWireBackend {
    /// Channel to send commands to PipeWire thread
    cmd_tx: mpsc::Sender<PwCommand>,
    /// Channel to receive audio samples (wrapped in Mutex for Sync)
    audio_rx: Mutex<mpsc::Receiver<PwAudioSamples>>,
    /// Cached input devices
    input_devices: Arc<Mutex<Vec<AudioDevice>>>,
    /// Cached system devices
    system_devices: Arc<Mutex<Vec<AudioDevice>>>,
    /// Thread handle
    _thread_handle: JoinHandle<()>,
    /// Sample rate from PipeWire
    sample_rate: Arc<Mutex<u32>>,
    /// Echo cancellation enabled flag (shared with mixer)
    aec_enabled: Arc<Mutex<bool>>,
    /// Recording mode (shared with mixer)
    recording_mode: Arc<Mutex<RecordingMode>>,
}

impl PipeWireBackend {
    /// Create and start the PipeWire backend with shared AEC enabled flag and recording mode
    pub fn new(
        aec_enabled: Arc<Mutex<bool>>,
        recording_mode: Arc<Mutex<RecordingMode>>,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (audio_tx, audio_rx) = mpsc::channel();
        let input_devices = Arc::new(Mutex::new(Vec::new()));
        let system_devices = Arc::new(Mutex::new(Vec::new()));
        let sample_rate = Arc::new(Mutex::new(48000u32));

        let input_devices_clone = Arc::clone(&input_devices);
        let system_devices_clone = Arc::clone(&system_devices);
        let sample_rate_clone = Arc::clone(&sample_rate);
        let aec_enabled_clone = Arc::clone(&aec_enabled);
        let recording_mode_clone = Arc::clone(&recording_mode);

        let thread_handle = thread::spawn(move || {
            if let Err(e) = run_pipewire_thread(
                cmd_rx,
                audio_tx,
                input_devices_clone,
                system_devices_clone,
                sample_rate_clone,
                aec_enabled_clone,
                recording_mode_clone,
            ) {
                tracing::error!("PipeWire thread error: {}", e);
            }
        });

        // Give PipeWire a moment to enumerate devices
        thread::sleep(std::time::Duration::from_millis(200));

        Ok(Self {
            cmd_tx,
            audio_rx: Mutex::new(audio_rx),
            input_devices,
            system_devices,
            _thread_handle: thread_handle,
            sample_rate,
            aec_enabled,
            recording_mode,
        })
    }
}

impl AudioBackend for PipeWireBackend {
    fn list_input_devices(&self) -> Vec<AudioDevice> {
        self.input_devices.lock().unwrap().clone()
    }

    fn list_system_devices(&self) -> Vec<AudioDevice> {
        self.system_devices.lock().unwrap().clone()
    }

    fn sample_rate(&self) -> u32 {
        *self.sample_rate.lock().unwrap()
    }

    fn start_capture_sources(
        &self,
        source1_id: Option<String>,
        source2_id: Option<String>,
    ) -> Result<(), String> {
        // Convert string IDs to u32 for PipeWire
        let source1: Option<u32> = source1_id.as_ref().and_then(|s| s.parse().ok());
        let source2: Option<u32> = source2_id.as_ref().and_then(|s| s.parse().ok());

        self.cmd_tx
            .send(PwCommand::StartCaptureSources {
                source1_id: source1,
                source2_id: source2,
            })
            .map_err(|e| format!("Failed to send start command: {}", e))
    }

    fn stop_capture(&self) -> Result<(), String> {
        self.cmd_tx
            .send(PwCommand::StopCapture)
            .map_err(|e| format!("Failed to send stop command: {}", e))
    }

    fn try_recv(&self) -> Option<AudioData> {
        let sample_rate = *self.sample_rate.lock().unwrap();
        self.audio_rx
            .lock()
            .unwrap()
            .try_recv()
            .ok()
            .map(|pw_samples| AudioData {
                samples: pw_samples.samples,
                channels: pw_samples.channels,
                sample_rate,
            })
    }

    fn set_aec_enabled(&self, enabled: bool) {
        *self.aec_enabled.lock().unwrap() = enabled;
    }

    fn set_recording_mode(&self, mode: RecordingMode) {
        *self.recording_mode.lock().unwrap() = mode;
    }
}

/// Create a Linux audio backend using PipeWire
pub fn create_backend(
    aec_enabled: Arc<Mutex<bool>>,
    recording_mode: Arc<Mutex<RecordingMode>>,
) -> Result<Box<dyn AudioBackend>, String> {
    let backend = PipeWireBackend::new(aec_enabled, recording_mode)?;
    Ok(Box::new(backend))
}

/// AEC3 frame size: 10ms at 48kHz = 480 samples per channel
const AEC_FRAME_SAMPLES: usize = 480;

/// Mixer state for combining audio from multiple streams
/// Uses separate render-first AEC processing pattern for proper echo cancellation.
struct AudioMixer {
    /// Buffer for capture samples (microphone/input)
    capture_buffer: Vec<f32>,
    /// Buffer for render samples (system audio/reference) - fed to AEC
    render_buffer: Vec<f32>,
    /// Buffer for render samples to mix with processed capture (for Mixed mode)
    render_mix_buffer: Vec<f32>,
    /// Number of active streams (1 or 2)
    num_streams: usize,
    /// Channels per stream
    channels: u16,
    /// Output sender
    output_tx: mpsc::Sender<PwAudioSamples>,
    /// Flag to enable/disable AEC (shared with main thread)
    aec_enabled: Arc<Mutex<bool>>,
    /// Recording mode - Mixed or EchoCancel (shared with main thread)
    recording_mode: Arc<Mutex<RecordingMode>>,
    /// AEC3 pipeline (created when in mixed mode with 2 streams)
    aec: Option<VoipAec3>,
}

impl AudioMixer {
    fn new(
        output_tx: mpsc::Sender<PwAudioSamples>,
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
            // Initial delay hint: start with 0ms and let AEC adapt
            match VoipAec3::builder(48000, self.channels as usize, self.channels as usize)
                .enable_high_pass(true)
                .initial_delay_ms(0)
                .build()
            {
                Ok(aec) => {
                    tracing::info!(
                        "PipeWire: AEC3 initialized: 48kHz, {} channels, {}ms frames",
                        self.channels,
                        AEC_FRAME_SAMPLES * 1000 / 48000
                    );
                    self.aec = Some(aec);
                }
                Err(e) => {
                    tracing::error!("PipeWire: Failed to initialize AEC3: {:?}", e);
                    self.aec = None;
                }
            }
        } else {
            self.aec = None;
        }
    }

    fn set_channels(&mut self, channels: u16) {
        self.channels = channels;
    }

    /// Add samples from a stream, routing based on source type
    /// - Sink capture (system audio) is fed IMMEDIATELY to AEC render path
    /// - Input capture (mic) is buffered and processed when enough data available
    fn push_samples(&mut self, samples: &[f32], is_sink_capture: bool) {
        if self.num_streams == 1 {
            // Only one stream - send directly (no AEC possible)
            let _ = self.output_tx.send(PwAudioSamples {
                samples: samples.to_vec(),
                channels: self.channels,
            });
            return;
        }

        // Two streams mode
        let frame_size = AEC_FRAME_SAMPLES * self.channels as usize;

        if is_sink_capture {
            // System audio (render) - feed to AEC immediately in frame-sized chunks
            // This is critical: AEC needs to see render BEFORE corresponding capture
            self.render_buffer.extend_from_slice(samples);
            // Also keep a copy for mixing in Mixed mode
            self.render_mix_buffer.extend_from_slice(samples);

            // Feed render frames to AEC immediately
            if let Some(ref mut aec) = self.aec {
                while self.render_buffer.len() >= frame_size {
                    let render_frame: Vec<f32> = self.render_buffer.drain(0..frame_size).collect();
                    if let Err(e) = aec.handle_render_frame(&render_frame) {
                        tracing::error!("PipeWire: AEC3 handle_render_frame error: {:?}", e);
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

        // Process capture frames when we have enough data from both sources
        while self.capture_buffer.len() >= frame_size && self.render_mix_buffer.len() >= frame_size
        {
            let capture_frame: Vec<f32> = self.capture_buffer.drain(0..frame_size).collect();
            let render_frame: Vec<f32> = self.render_mix_buffer.drain(0..frame_size).collect();

            // Apply AEC if enabled and we have an AEC instance
            let processed_capture = if aec_enabled {
                if let Some(ref mut aec) = self.aec {
                    let mut out = vec![0.0f32; capture_frame.len()];

                    match aec.process_capture_frame(&capture_frame, false, &mut out) {
                        Ok(_metrics) => out,
                        Err(e) => {
                            tracing::error!("PipeWire: AEC3 process_capture_frame error: {:?}", e);
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
                    // Mix processed capture with system audio (0.5 gain each to prevent clipping)
                    processed_capture
                        .iter()
                        .zip(render_frame.iter())
                        .map(|(&s1, &s2)| (s1 + s2) * 0.5)
                        .collect()
                }
                RecordingMode::EchoCancel => {
                    // Output only the processed capture signal - no mixing
                    processed_capture
                }
            };

            // Debug logging (periodic)
            static LOG_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let count = LOG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                    "PipeWire AudioMixer: mode={:?}, aec={}, render_rms={:.4}, out_rms={:.4}",
                    recording_mode,
                    aec_enabled,
                    render_rms,
                    out_rms
                );
            }

            // Send output
            let _ = self.output_tx.send(PwAudioSamples {
                samples: output,
                channels: self.channels,
            });
        }
    }
}

/// Held stream state - keeps stream and listener alive
struct ActiveStream {
    _stream: Stream,
    // The listener is leaked (forgotten) to keep it alive
}

/// Internal state for the PipeWire thread
struct PwThreadState {
    /// Active streams (kept alive)
    streams: Vec<ActiveStream>,
    /// Sample rate (updated from param_changed)
    sample_rate: Arc<Mutex<u32>>,
    /// Set of sink (system audio) device IDs
    sink_ids: Rc<RefCell<std::collections::HashSet<u32>>>,
}

/// Run the PipeWire main loop thread
fn run_pipewire_thread(
    cmd_rx: mpsc::Receiver<PwCommand>,
    audio_tx: mpsc::Sender<PwAudioSamples>,
    input_devices: Arc<Mutex<Vec<AudioDevice>>>,
    system_devices: Arc<Mutex<Vec<AudioDevice>>>,
    sample_rate: Arc<Mutex<u32>>,
    aec_enabled: Arc<Mutex<bool>>,
    recording_mode: Arc<Mutex<RecordingMode>>,
) -> Result<(), String> {
    // Initialize PipeWire
    pipewire::init();

    let mainloop = MainLoop::new(None).map_err(|e| format!("Failed to create main loop: {}", e))?;
    let context =
        Context::new(&mainloop).map_err(|e| format!("Failed to create context: {}", e))?;
    let core = context
        .connect(None)
        .map_err(|e| format!("Failed to connect to PipeWire: {}", e))?;
    let registry = core
        .get_registry()
        .map_err(|e| format!("Failed to get registry: {}", e))?;

    // Device maps for enumeration
    let input_map: Rc<RefCell<HashMap<u32, AudioDevice>>> = Rc::new(RefCell::new(HashMap::new()));
    let system_map: Rc<RefCell<HashMap<u32, AudioDevice>>> = Rc::new(RefCell::new(HashMap::new()));

    // Setup registry listener for device discovery
    let input_map_clone = Rc::clone(&input_map);
    let system_map_clone = Rc::clone(&system_map);
    let input_devices_clone = Arc::clone(&input_devices);
    let system_devices_clone = Arc::clone(&system_devices);

    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ == ObjectType::Node {
                if let Some(props) = &global.props {
                    let media_class = props.get("media.class").unwrap_or("");
                    let node_name = props.get("node.name").unwrap_or("Unknown");
                    let node_desc = props.get("node.description").unwrap_or(node_name);

                    if media_class == "Audio/Source" {
                        // Input device (microphone)
                        let device = AudioDevice {
                            id: global.id.to_string(),
                            name: node_desc.to_string(),
                            source_type: AudioSourceType::Input,
                        };
                        input_map_clone.borrow_mut().insert(global.id, device);
                        // Update shared list
                        let devices: Vec<_> = input_map_clone.borrow().values().cloned().collect();
                        *input_devices_clone.lock().unwrap() = devices;
                    } else if media_class == "Audio/Sink" {
                        // Output device - we can capture its monitor
                        let device = AudioDevice {
                            id: global.id.to_string(),
                            name: format!("{} (Monitor)", node_desc),
                            source_type: AudioSourceType::System,
                        };
                        system_map_clone.borrow_mut().insert(global.id, device);
                        // Update shared list
                        let devices: Vec<_> = system_map_clone.borrow().values().cloned().collect();
                        *system_devices_clone.lock().unwrap() = devices;
                    }
                }
            }
        })
        .global_remove({
            let input_map = Rc::clone(&input_map);
            let system_map = Rc::clone(&system_map);
            let input_devices = Arc::clone(&input_devices);
            let system_devices = Arc::clone(&system_devices);
            move |id| {
                if input_map.borrow_mut().remove(&id).is_some() {
                    let devices: Vec<_> = input_map.borrow().values().cloned().collect();
                    *input_devices.lock().unwrap() = devices;
                }
                if system_map.borrow_mut().remove(&id).is_some() {
                    let devices: Vec<_> = system_map.borrow().values().cloned().collect();
                    *system_devices.lock().unwrap() = devices;
                }
            }
        })
        .register();

    // Create mixer with AEC enabled flag and recording mode
    let mixer = Rc::new(RefCell::new(AudioMixer::new(
        audio_tx,
        aec_enabled,
        recording_mode,
    )));

    // Thread state - share system_map to know which IDs are sinks
    let state = Rc::new(RefCell::new(PwThreadState {
        streams: Vec::new(),
        sample_rate: Arc::clone(&sample_rate),
        sink_ids: Rc::new(RefCell::new(std::collections::HashSet::new())),
    }));

    // Keep sink_ids in sync with system_map
    let sink_ids_for_state = Rc::clone(&state.borrow().sink_ids);
    let system_map_for_sync = Rc::clone(&system_map);

    // Setup command receiver using a timer that polls the channel
    let core_ref = Rc::new(core);
    let core_for_timer = Rc::clone(&core_ref);
    let state_for_timer = Rc::clone(&state);
    let mixer_for_timer = Rc::clone(&mixer);

    // Create a timer source to poll for commands
    let timer_source = mainloop.loop_().add_timer({
        move |_elapsed| {
            // Update sink_ids from system_map
            {
                let mut sink_ids = sink_ids_for_state.borrow_mut();
                sink_ids.clear();
                for id in system_map_for_sync.borrow().keys() {
                    sink_ids.insert(*id);
                }
            }

            // Poll for commands
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    PwCommand::StartCaptureSources {
                        source1_id,
                        source2_id,
                    } => {
                        // First, check which sources are sinks (before borrowing state mutably)
                        let is_sink1 = source1_id
                            .map(|id| state_for_timer.borrow().sink_ids.borrow().contains(&id))
                            .unwrap_or(false);
                        let is_sink2 = source2_id
                            .map(|id| state_for_timer.borrow().sink_ids.borrow().contains(&id))
                            .unwrap_or(false);

                        let mut state = state_for_timer.borrow_mut();
                        // Clear existing streams
                        state.streams.clear();

                        // Count how many streams we'll have
                        let num_streams =
                            source1_id.is_some() as usize + source2_id.is_some() as usize;
                        mixer_for_timer.borrow_mut().set_num_streams(num_streams);

                        // Create stream for source1 if specified
                        if let Some(id) = source1_id {
                            let mixer_clone = Rc::clone(&mixer_for_timer);
                            match create_capture_stream(
                                &core_for_timer,
                                Some(id),
                                is_sink1,
                                1, // stream index
                                mixer_clone,
                                Arc::clone(&state.sample_rate),
                            ) {
                                Ok(stream) => state.streams.push(stream),
                                Err(e) => {
                                    tracing::error!("Failed to create stream for source1: {}", e)
                                }
                            }
                        }

                        // Create stream for source2 if specified
                        if let Some(id) = source2_id {
                            let mixer_clone = Rc::clone(&mixer_for_timer);
                            match create_capture_stream(
                                &core_for_timer,
                                Some(id),
                                is_sink2,
                                2, // stream index
                                mixer_clone,
                                Arc::clone(&state.sample_rate),
                            ) {
                                Ok(stream) => state.streams.push(stream),
                                Err(e) => {
                                    tracing::error!("Failed to create stream for source2: {}", e)
                                }
                            }
                        }
                    }
                    PwCommand::StopCapture => {
                        state_for_timer.borrow_mut().streams.clear();
                        mixer_for_timer.borrow_mut().set_num_streams(0);
                    }
                }
            }
        }
    });

    // Set timer to fire every 10ms
    timer_source.update_timer(
        Some(std::time::Duration::from_millis(10)),
        Some(std::time::Duration::from_millis(10)),
    );

    // Run the main loop (blocks until quit)
    mainloop.run();

    Ok(())
}

/// Create an audio format pod for stream connection
fn create_audio_format_pod() -> Vec<u8> {
    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    // Leave rate and channels unset to accept native graph format

    let obj = pipewire::spa::pod::Object {
        type_: pipewire::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pipewire::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };

    pipewire::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pipewire::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner()
}

/// Create a capture stream that sends samples to the mixer
fn create_capture_stream(
    core: &pipewire::core::Core,
    device_id: Option<u32>,
    capture_sink: bool,
    stream_index: usize, // 1 or 2
    mixer: Rc<RefCell<AudioMixer>>,
    sample_rate: Arc<Mutex<u32>>,
) -> Result<ActiveStream, String> {
    let stream_name = if capture_sink {
        format!("flowstt-system-capture-{}", stream_index)
    } else {
        format!("flowstt-input-capture-{}", stream_index)
    };

    let props = if capture_sink {
        properties! {
            *pipewire::keys::MEDIA_TYPE => "Audio",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Music",
            *pipewire::keys::STREAM_CAPTURE_SINK => "true",
        }
    } else {
        properties! {
            *pipewire::keys::MEDIA_TYPE => "Audio",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Music",
        }
    };

    let stream = Stream::new(core, &stream_name, props)
        .map_err(|e| format!("Failed to create stream: {}", e))?;

    // Track format info from param_changed
    let format_info: Rc<RefCell<AudioInfoRaw>> = Rc::new(RefCell::new(AudioInfoRaw::default()));
    let format_info_for_param = Rc::clone(&format_info);
    let sample_rate_for_param = Arc::clone(&sample_rate);
    let mixer_for_param = Rc::clone(&mixer);
    let mixer_for_process = mixer;

    let listener = stream
        .add_local_listener_with_user_data(())
        .param_changed(move |_stream, _user_data, id, param| {
            let Some(param) = param else { return };

            if id != pipewire::spa::param::ParamType::Format.as_raw() {
                return;
            }

            // Parse the format
            if let Ok((media_type, media_subtype)) =
                pipewire::spa::param::format_utils::parse_format(param)
            {
                use pipewire::spa::param::format::{MediaSubtype, MediaType};
                if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                    return;
                }

                if format_info_for_param.borrow_mut().parse(param).is_ok() {
                    let rate = format_info_for_param.borrow().rate();
                    let channels = format_info_for_param.borrow().channels();
                    tracing::info!(
                        "Stream {} format: rate={}, channels={}",
                        stream_index,
                        rate,
                        channels
                    );
                    *sample_rate_for_param.lock().unwrap() = rate;
                    mixer_for_param.borrow_mut().set_channels(channels as u16);
                }
            }
        })
        .state_changed(move |_stream, _user_data, old, new| {
            tracing::debug!("Stream {} state: {:?} -> {:?}", stream_index, old, new);
        })
        .process(move |stream, _user_data| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }

                let data = &mut datas[0];
                // Get chunk info first
                let chunk_size = data.chunk().size() as usize;
                let n_samples = chunk_size / mem::size_of::<f32>();

                if n_samples == 0 {
                    return;
                }

                if let Some(samples_data) = data.data() {
                    // Convert bytes to f32 samples
                    let samples: Vec<f32> = samples_data[..chunk_size]
                        .chunks_exact(4)
                        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                        .collect();

                    if !samples.is_empty() {
                        // Route to appropriate mixer buffer based on source type:
                        // - Sink capture (system audio) goes to reference buffer for AEC
                        // - Input capture (mic) goes to capture buffer for AEC
                        let mut mixer = mixer_for_process.borrow_mut();
                        mixer.push_samples(&samples, capture_sink);
                    }
                }
            }
        })
        .register()
        .map_err(|e| format!("Failed to register stream listener: {}", e))?;

    // Create audio format parameters
    let format_pod = create_audio_format_pod();
    let mut params = [Pod::from_bytes(&format_pod).unwrap()];

    // Connect to device (or default if None)
    let flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;

    stream
        .connect(Direction::Input, device_id, flags, &mut params)
        .map_err(|e| format!("Failed to connect stream: {}", e))?;

    // Leak the listener to keep it alive - it will be cleaned up when stream is dropped
    std::mem::forget(listener);

    Ok(ActiveStream { _stream: stream })
}
