# vtx-engine

A reusable voice processing and transcription library built in Rust with TypeScript visualization support for Tauri applications.

Extracted from [FlowSTT](https://github.com/user/flowstt), vtx-engine provides platform-native audio capture, real-time speech detection, audio visualization, and Whisper-based transcription as composable libraries that can be shared across projects.

## Project Structure

```
vtx-engine/
├── crates/
│   ├── vtx-common/          Shared types (AudioDevice, EngineEvent, WhisperModel, etc.)
│   └── vtx-engine/          Core Rust library
│       ├── platform/        Audio capture backends (WASAPI, CoreAudio, PipeWire)
│       ├── processor.rs     Speech detection + visualization processing
│       └── transcription/   Whisper FFI, transcription queue, segmentation
├── packages/
│   └── vtx-viz/             TypeScript visualization renderers (@vtx-engine/viz)
└── apps/
    └── vtx-demo/            Tauri demo application
```

## Crates

### vtx-common

Shared type definitions used by both the Rust engine and TypeScript frontend. All types derive `Serialize`/`Deserialize` for seamless Tauri IPC.

Key types: `AudioDevice`, `VisualizationData`, `SpeechMetrics`, `TranscriptionResult`, `TranscriptionSegment`, `EngineEvent`, `EngineStatus`, `ModelStatus`, `GpuStatus`, `WhisperModel`, `TranscriptionProfile`, `TranscriptionMode`, `RecordingMode`.

### vtx-engine

The core library. Primary entry point is `EngineBuilder`, which produces an `AudioEngine` and a `broadcast::Receiver<EngineEvent>`.

**Capabilities:**

- **Audio capture** -- Platform-native backends: WASAPI (Windows), CoreAudio + ScreenCaptureKit (macOS), PipeWire (Linux)
- **Echo cancellation** -- AEC3-based echo cancellation when mixing microphone and system audio (`RecordingMode::EchoCancel`)
- **Speech detection** -- Dual-mode VAD (voiced + whisper) with multi-feature analysis (amplitude, ZCR, spectral centroid), transient rejection, 200ms lookback, and word-break detection
- **Push-to-talk** -- Application-supplied `PushToTalkController` for PTT segmentation as an alternative to VAD
- **Visualization** -- Real-time waveform downsampling, 512-point FFT spectrogram with log-frequency mapping and custom color LUT, speech activity metrics
- **Transcription (live)** -- Whisper.cpp via dynamic FFI with hallucination mitigation, bounded transcription queue with worker thread, automatic speech segmentation with ring buffer
- **Transcription (stream)** -- `transcribe_audio_stream`: accepts a channel of 16 kHz mono f32 PCM frames, emits `TranscriptionSegment` events in real time, returns complete segment list on completion
- **Transcription (file)** -- `transcribe_audio_file`: loads a WAV file, resamples to 16 kHz mono, returns `Vec<TranscriptionSegment>`
- **Model management** -- `ModelManager`: typed `WhisperModel` enum covering all 9 ggml variants, platform-aware cache directory, async download with progress callback
- **Config persistence** -- `EngineConfig::load()` / `EngineConfig::save()` via TOML in the platform-standard config directory
- **Transcription history** -- `TranscriptionHistory`: bounded NDJSON-backed history store with WAV TTL cleanup

**Quick start:**

```rust
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_common::{EngineEvent, TranscriptionProfile, WhisperModel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the model is available before building the engine.
    let mgr = ModelManager::new("my-app");
    if !mgr.is_available(WhisperModel::BaseEn) {
        mgr.download(WhisperModel::BaseEn, |pct| print!("\r{}%  ", pct)).await?;
    }

    // Build the engine. Returns (engine, event receiver).
    let (engine, mut rx) = EngineBuilder::new()
        .with_profile(TranscriptionProfile::Dictation)
        .build()
        .await?;

    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let EngineEvent::TranscriptionComplete(result) = event {
                println!("{}", result.text);
            }
        }
    });

    let devices = engine.list_input_devices();
    engine.start_capture(devices.first().map(|d| d.id.clone()), None).await?;

    tokio::signal::ctrl_c().await?;
    engine.stop_capture().await?;
    Ok(())
}
```

See [USAGE.md](./USAGE.md) for full integration patterns including stream transcription, model management, push-to-talk, and subsystem configuration.

**`EngineBuilder` API:**

| Method | Description |
|---|---|
| `EngineBuilder::new()` | Create builder with all subsystems enabled and default config |
| `EngineBuilder::from_config(config)` | Pre-populate from an existing `EngineConfig` |
| `.with_profile(TranscriptionProfile)` | Apply a preset profile (`Dictation`, `Transcription`, or `Custom`) |
| `.model(WhisperModel)` | Set the Whisper model variant |
| `.recording_mode(RecordingMode)` | Set recording mode (`Mixed` or `EchoCancel`) |
| `.transcription_mode(TranscriptionMode)` | Set segmentation mode (`Automatic` VAD or `PushToTalk`) |
| `.word_break_segmentation_enabled(bool)` | Enable/disable word-break segment splitting |
| `.segment_max_duration_ms(u64)` | Maximum segment duration in ms |
| `.segment_word_break_grace_ms(u64)` | Grace period after max duration before forced submission |
| `.without_transcription()` | Disable transcription subsystem (no whisper.cpp loaded) |
| `.without_visualization()` | Disable visualization subsystem |
| `.without_vad()` | Disable VAD (PTT still works) |
| `.build().await` | Construct engine; returns `(AudioEngine, broadcast::Receiver<EngineEvent>)` |

**`AudioEngine` API:**

| Method | Description |
|---|---|
| `AudioEngine::new(config)` | Convenience constructor; equivalent to `EngineBuilder::from_config(config).build().await` |
| `subscribe()` | Subscribe an additional receiver to the event broadcast channel |
| `ptt_controller()` | Get a `Clone + Send` `PushToTalkController` |
| `list_input_devices()` | Enumerate microphones |
| `list_system_devices()` | Enumerate system audio / loopback devices |
| `start_capture(source1, source2)` | Start capturing from one or two devices |
| `stop_capture()` | Stop audio capture |
| `is_capturing()` | Whether capture is currently active |
| `set_transcription_enabled(bool)` | Enable/disable transcription without stopping capture |
| `is_transcription_enabled()` | Whether transcription is currently enabled |
| `finalize_segment()` | Force-submit the current in-flight audio segment |
| `transcribe_audio_file(path)` | Load a WAV file and return `Vec<TranscriptionSegment>` |
| `transcribe_audio_stream(rx, session_start)` | Drain a 16 kHz mono f32 channel and return a `JoinHandle<Vec<TranscriptionSegment>>` |
| `check_model_status()` | Check whether the configured Whisper model file is present |
| `download_model()` | Download the configured Whisper model (emits progress events) |
| `check_gpu_status()` | Check CUDA / Metal availability |
| `get_status()` | Get current `EngineStatus` snapshot |
| `start_test_capture(device_id)` | Lightweight capture for audio level metering |
| `stop_test_capture()` | Stop test capture |
| `shutdown()` | Shut down the engine (also called on `Drop`) |

**`ModelManager` API:**

| Method | Description |
|---|---|
| `ModelManager::new(app_name)` | Construct; cache root: `{platform_cache}/{app_name}/whisper/` |
| `path(model)` | Returns `PathBuf` to `ggml-{slug}.bin` (file need not exist) |
| `is_available(model)` | `true` if file exists and has non-zero size |
| `list_cached()` | All available variants in ascending size order |
| `download(model, on_progress)` | Async download from Hugging Face with progress callback (0–100) |

**`EngineEvent` variants:**

| Variant | Description |
|---|---|
| `VisualizationData(VisualizationData)` | Waveform, spectrogram, and speech metric frame |
| `TranscriptionComplete(TranscriptionResult)` | Completed utterance from live VAD/PTT capture |
| `TranscriptionSegment(TranscriptionSegment)` | Timestamped segment from file or stream transcription |
| `SpeechStarted` | VAD onset or PTT press |
| `SpeechEnded { duration_ms }` | VAD offset or PTT release with duration |
| `CaptureStateChanged { capturing, error }` | Capture started or stopped |
| `ModelDownloadProgress { percent }` | Download progress 0–100 |
| `ModelDownloadComplete { success }` | Download finished |
| `AudioLevelUpdate { device_id, level_db }` | RMS level from test capture |

**Features:**

| Feature | Description |
|---|---|
| `default` | CPU-only transcription |
| `cuda` | Enable CUDA GPU acceleration (Linux build-time requirement; Windows uses prebuilt binaries automatically) |

## Packages

### @vtx-engine/viz

TypeScript visualization library for rendering audio data in Canvas2D. Designed for Tauri applications but has no Tauri dependency -- it works with any framework that can provide a `<canvas>` element and feed it data.

**Renderers:**

| Renderer | Description |
|---|---|
| `WaveformRenderer` | Full-size oscilloscope with grid, amplitude/time labels, and glow effect |
| `SpectrogramRenderer` | Scrolling spectrogram with log-frequency Y-axis (20Hz--24kHz), pre-computed RGB from backend |
| `SpeechActivityRenderer` | Multi-metric overlay: amplitude (gold), ZCR (cyan), centroid (magenta) lines; speech state bar with confirmed/lookback/word-break regions; state markers |
| `MiniWaveformRenderer` | Compact stylized waveform with center-attenuated amplitude and Catmull-Rom smoothing |

**Theming:**

All renderers read colors from CSS custom properties prefixed with `--vtx-`. Import the default theme or define your own:

```css
/* Default dark theme is provided */
@import "@vtx-engine/viz/styles";

/* Override any variable */
:root {
  --vtx-waveform-color: #10b981;
  --vtx-waveform-bg: #0a0a0a;
}
```

**Usage:**

```typescript
import {
  WaveformRenderer,
  SpectrogramRenderer,
  SpeechActivityRenderer,
} from "@vtx-engine/viz";
import type { VisualizationPayload } from "@vtx-engine/viz";

const waveform = new WaveformRenderer(canvas);
waveform.start();

// When you receive visualization data from the engine:
function onVisualizationData(data: VisualizationPayload) {
  waveform.pushSamples(data.waveform);

  if (data.spectrogram) {
    spectrogram.pushColumn(data.spectrogram.colors);
  }

  if (data.speech_metrics) {
    speechActivity.pushMetrics(data.speech_metrics);
  }
}
```

**CSS variables reference:**

| Variable | Default | Description |
|---|---|---|
| `--vtx-waveform-bg` | `#0f172a` | Canvas background |
| `--vtx-waveform-color` | `#3b82f6` | Waveform line color |
| `--vtx-waveform-glow` | `rgba(59,130,246,0.5)` | Waveform glow effect |
| `--vtx-waveform-grid` | `rgba(255,255,255,0.06)` | Grid line color |
| `--vtx-waveform-text` | `rgba(255,255,255,0.45)` | Axis label color |
| `--vtx-spectrogram-grid` | `rgba(255,255,255,0.1)` | Spectrogram grid overlay |
| `--vtx-speech-confirmed` | `rgba(34,197,94,0.5)` | Confirmed speech bar |
| `--vtx-speech-lookback` | `rgba(59,130,246,0.7)` | Lookback-detected speech bar |
| `--vtx-speech-word-break` | `rgba(249,115,22,0.85)` | Word break markers |
| `--vtx-metric-amplitude` | `rgba(245,158,11,0.75)` | Amplitude line (gold) |
| `--vtx-metric-zcr` | `rgba(6,182,212,0.75)` | ZCR line (cyan) |
| `--vtx-metric-centroid` | `rgba(217,70,239,0.75)` | Centroid line (magenta) |
| `--vtx-threshold-line` | `rgba(255,255,255,0.15)` | Detection threshold lines |

A built-in `[data-theme="light"]` override is included for light mode.

## Apps

### vtx-demo

A Tauri 2 demo application for testing the library. Features:

- **Device selection** -- Dropdown populated from `list_input_devices()`
- **Live recording** -- Record/Stop buttons that start/stop audio capture
- **WAV file transcription** -- Open a `.wav` file and transcribe it
- **Real-time visualizations** -- Waveform, spectrogram, and speech activity canvases
- **Model management** -- Automatic detection and download of the Whisper model
- **Transcription output** -- Timestamped results displayed as they arrive

## Building

### Prerequisites

- Rust toolchain (stable)
- Node.js and pnpm
- Platform-specific requirements:
  - **Windows**: Visual Studio Build Tools
  - **macOS**: Xcode Command Line Tools
  - **Linux**: PipeWire development headers, CMake (for whisper.cpp build)

### Rust workspace

```bash
# Check compilation
cargo check

# Build all crates
cargo build

# Build with CUDA support (Linux)
cargo build --features vtx-engine/cuda
```

The build script automatically downloads prebuilt whisper.cpp binaries:
- **Windows**: CUDA-enabled DLLs with automatic CPU fallback
- **macOS**: Metal-enabled xcframework
- **Linux**: Built from source via CMake

### TypeScript visualization library

```bash
cd packages/vtx-viz
pnpm install
pnpm build
```

### Demo application

```bash
cd apps/vtx-demo
pnpm install
cargo tauri dev    # Development mode
cargo tauri build  # Production build
```

## Architecture

### Audio Pipeline

1. **Platform backend** captures interleaved f32 audio at native rate (typically 48kHz stereo)
2. **Audio loop** (dedicated thread) converts to mono and feeds two processors in parallel:
   - **SpeechDetector** -- dual-mode VAD (voiced: -42dB/80ms onset, whisper: -52dB/120ms onset) with transient rejection, 200ms lookback ring buffer, and word-break detection
   - **VisualizationProcessor** -- peak-detected waveform downsampling (64 samples/emit), 512-point Hanning-windowed FFT with log-frequency spectrogram
3. **TranscribeState** manages a 30-second stereo ring buffer with speech-triggered segmentation (configurable max duration, word-break grace period)
4. **TranscriptionQueue** processes segments sequentially: mono conversion, 16kHz resampling, whisper.cpp inference with hallucination mitigation (no_context, entropy/logprob thresholds, repetition loop removal)

### Event Flow

```
AudioBackend  -->  AudioLoop  -->  SpeechDetector  -->  TranscribeState  -->  Queue  -->  Transcriber
                       |                |                                                     |
                       v                v                                                     v
              VisualizationProcessor  broadcast channel                           broadcast channel
                       |              (SpeechStarted/Ended)                      (TranscriptionComplete
                       v                                                           TranscriptionSegment)
              broadcast channel
              (VisualizationData)
```

In a Tauri app, subscribe to the `broadcast::Receiver<EngineEvent>` returned by `EngineBuilder::build()` and forward events via `app_handle.emit()` to the frontend, where the vtx-viz renderers consume `VisualizationData` payloads.

## License

MIT
