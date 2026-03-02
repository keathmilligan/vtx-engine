# vtx-engine

A reusable voice processing and transcription library built in Rust with TypeScript visualization support for Tauri applications.

Extracted from [FlowSTT](https://github.com/user/flowstt), vtx-engine provides platform-native audio capture, real-time speech detection, audio visualization, and Whisper-based transcription as composable libraries that can be shared across projects.

## Project Structure

```
vtx-engine/
├── crates/
│   ├── vtx-common/          Shared types (AudioDevice, EngineEvent, etc.)
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

Key types: `AudioDevice`, `VisualizationData`, `SpeechMetrics`, `TranscriptionResult`, `EngineEvent`, `EngineStatus`, `ModelStatus`, `GpuStatus`.

### vtx-engine

The core library. Provides a single `AudioEngine` struct as the entry point.

**Capabilities:**

- **Audio capture** -- Platform-native backends: WASAPI (Windows), CoreAudio + ScreenCaptureKit (macOS), PipeWire (Linux)
- **Echo cancellation** -- AEC3-based echo cancellation when mixing microphone and system audio
- **Speech detection** -- Dual-mode VAD (voiced + whisper) with multi-feature analysis (amplitude, ZCR, spectral centroid), transient rejection, 200ms lookback, and word-break detection
- **Visualization** -- Real-time waveform downsampling, 512-point FFT spectrogram with log-frequency mapping and custom color LUT, speech activity metrics
- **Transcription** -- whisper.cpp via dynamic FFI with hallucination mitigation, bounded transcription queue with worker thread, automatic speech segmentation with ring buffer
- **Model management** -- Automatic download of ggml-base.en.bin from HuggingFace, GPU status detection (CUDA/Metal)

**Usage:**

```rust
use vtx_engine::{AudioEngine, EngineConfig, EventHandler};
use vtx_common::EngineEvent;

struct MyHandler;

impl EventHandler for MyHandler {
    fn on_event(&self, event: EngineEvent) {
        match event {
            EngineEvent::VisualizationData(data) => {
                // Forward to frontend for rendering
            }
            EngineEvent::TranscriptionComplete(result) => {
                println!("Transcribed: {}", result.text);
            }
            EngineEvent::SpeechStarted => { /* ... */ }
            EngineEvent::SpeechEnded { duration_ms } => { /* ... */ }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let engine = AudioEngine::new(EngineConfig::default(), MyHandler).await?;

    // List and select an input device
    let devices = engine.list_input_devices();
    if let Some(device) = devices.first() {
        engine.start_capture(Some(device.id.clone()), None).await?;
    }

    // Or transcribe a file directly
    let result = engine.transcribe_file("recording.wav".into()).await?;
    println!("{}", result.text);

    Ok(())
}
```

**API overview:**

| Method | Description |
|---|---|
| `AudioEngine::new(config, handler)` | Create engine, initialize audio backend and transcription |
| `list_input_devices()` | Enumerate microphones |
| `list_system_devices()` | Enumerate system audio / loopback devices |
| `start_capture(source1, source2)` | Start capturing from specified devices |
| `stop_capture()` | Stop audio capture |
| `transcribe_file(path)` | Transcribe a WAV file |
| `download_model()` | Download the Whisper model (emits progress events) |
| `check_model_status()` | Check if the Whisper model is available |
| `check_gpu_status()` | Check CUDA / Metal availability |
| `start_test_capture(device_id)` | Lightweight capture for audio level metering |

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
3. **TranscribeState** manages a 30-second stereo ring buffer with speech-triggered segmentation (4s max, 750ms word-break grace)
4. **TranscriptionQueue** processes segments sequentially: mono conversion, 16kHz resampling, whisper.cpp inference with hallucination mitigation (no_context, entropy/logprob thresholds, repetition loop removal)

### Event Flow

```
AudioBackend  -->  AudioLoop  -->  SpeechDetector  -->  TranscribeState  -->  Queue  -->  Transcriber
                       |                |                                                     |
                       v                v                                                     v
              VisualizationProcessor  EventHandler::on_event()                    EventHandler::on_event()
                       |              (SpeechStarted/Ended)                       (TranscriptionComplete)
                       v
              EventHandler::on_event()
              (VisualizationData)
```

In a Tauri app, the `EventHandler` implementation forwards events via `app_handle.emit()` to the frontend, where the vtx-viz renderers consume `VisualizationData` payloads.

## License

MIT
