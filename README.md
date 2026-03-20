https://github.com/user-attachments/assets/1182e5c8-c51f-414a-bd21-2957388f5f1e

# vtx-engine

[![CI](https://github.com/keathmilligan/vtx-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/keathmilligan/vtx-engine/actions/workflows/ci.yml)
[![Publish](https://github.com/keathmilligan/vtx-engine/actions/workflows/publish.yml/badge.svg)](https://github.com/keathmilligan/vtx-engine/actions/workflows/publish.yml)
[![Release](https://github.com/keathmilligan/vtx-engine/actions/workflows/release.yml/badge.svg)](https://github.com/keathmilligan/vtx-engine/actions/workflows/release.yml)

A reusable voice processing and transcription library built in Rust.

Provides platform-native audio capture, real-time speech detection, audio visualization, and Whisper-based transcription as composable libraries. Supports Windows, Linux, and macOS.

## Features

### Rust engine (`vtx-engine`)

- **Platform-native audio capture** — WASAPI, CoreAudio/ScreenCaptureKit, PipeWire
- **Audio processing** — Echo cancellation (AEC3), automatic gain control, and real-time speech detection with dual-mode VAD
- **Live transcription** — Whisper.cpp via dynamic FFI with VAD-driven segmentation and hallucination mitigation
- **GPU acceleration** — CUDA on Windows, Metal on macOS
- **Flexible input** — Live microphone capture, manual recording (up to 30 min), file playback, and raw audio stream transcription
- **Visualization data** — Waveform, FFT spectrogram, and per-frame speech activity metrics
- **Audio data streaming** — Opt-in real-time streaming of processed and/or raw audio samples with sample-accurate timing for A/V synchronization
- **Model management** — Async download of all Whisper ggml model variants with progress callbacks
- **Config & history** — TOML-based config persistence and bounded transcription history with WAV cleanup

### TypeScript visualization (`@vtx-engine/viz`)

- **Canvas renderers** — Real-time waveform, spectrogram, mini-waveform thumbnail, and scrollable speech activity history

## Adding to Your Project

### Rust engine

```sh
cargo add vtx-engine
```

Or add to your `Cargo.toml` manually:

```toml
[dependencies]
vtx-engine = "0.1"
tokio = { version = "1", features = ["full"] }
```

### TypeScript visualization library

```sh
npm install @vtx-engine/viz
# or
pnpm add @vtx-engine/viz
```

Then import the renderers and (optionally) the bundled stylesheet:

```ts
import { SpeechActivityRenderer, WaveformRenderer, SpectrogramRenderer } from "@vtx-engine/viz";
import "@vtx-engine/viz/styles";
```

## Quick Start

### Real-time dictation

```rust
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_engine::{EngineEvent, TranscriptionProfile, WhisperModel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Download model if needed
    let mgr = ModelManager::new("my-app");
    if !mgr.is_available(WhisperModel::BaseEn) {
        mgr.download(WhisperModel::BaseEn, |pct| print!("\r{}%", pct)).await?;
    }

    let (engine, mut rx) = EngineBuilder::new()
        .app_name("my-app")
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

### Stream transcription

```rust
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_engine::{TranscriptionProfile, WhisperModel};
use tokio::sync::mpsc;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, _rx) = EngineBuilder::new()
        .app_name("my-app")
        .with_profile(TranscriptionProfile::Transcription)
        .build()
        .await?;

    let (tx, rx_audio) = mpsc::channel::<Vec<f32>>(64);
    let handle = engine.transcribe_audio_stream(rx_audio, Instant::now());

    // Send 16 kHz mono f32 PCM frames; drop tx to signal end of stream
    drop(tx);

    let segments = handle.await?;
    for seg in &segments {
        println!("[{:.1}s] {}", seg.timestamp_offset_ms as f64 / 1000.0, seg.text);
    }
    Ok(())
}
```

See [USAGE.md](USAGE.md) for full examples covering manual recording, file playback, model management, config persistence, subsystem configuration, and the speech activity visualization renderer.

### Speech activity visualization (quick start)

```ts
import { SpeechActivityRenderer } from "@vtx-engine/viz";
import type { SpeechMetrics } from "@vtx-engine/viz";

const canvas = document.getElementById("speech-canvas") as HTMLCanvasElement;

// bufferSize = visible window (frames); maxHistoryFrames = scroll depth (~30 min at 16ms/frame)
const renderer = new SpeechActivityRenderer(canvas, 256, 108_000);
renderer.drawIdle();

// Wire up scroll controls
btnScrollBack.addEventListener("click", () =>
  renderer.scrollBy(Math.round(renderer.bufferFrames / 4))
);
btnScrollFwd.addEventListener("click", () =>
  renderer.scrollBy(-Math.round(renderer.bufferFrames / 4))
);
btnScrollLive.addEventListener("click", () => renderer.resetToLive());

// Feed visualization data from the engine event channel
engine.on("visualization-data", (payload) => {
  if (payload.frame_interval_ms) renderer.configure(payload.frame_interval_ms);
  if (payload.speech_metrics)    renderer.pushMetrics(payload.speech_metrics);
});

// Lifecycle
renderer.start();    // begin rAF draw loop
// ...
renderer.stop();     // stop loop; one final draw
renderer.clear();    // reset all history and scroll state
```

## Whisper Models

| Variant | Size | Language |
|---|---|---|
| `TinyEn` / `Tiny` | ~39 MB | English / Multilingual |
| `BaseEn` / `Base` | ~74 MB | English / Multilingual |
| `SmallEn` / `Small` | ~244 MB | English / Multilingual |
| `MediumEn` / `Medium` | ~769 MB | English / Multilingual |
| `LargeV3` | ~1.5 GB | Multilingual |

`BaseEn` is the default. Use `ModelManager` to download models at runtime.

## Running the Demo App

A Tauri-based demo application is included in `apps/vtx-demo`:

```sh
# Install dependencies
make demo-dev
```

## Prerequisites

- Rust stable toolchain
- **Windows**: Visual Studio Build Tools
- **macOS**: Xcode Command Line Tools
- **Linux**: PipeWire development headers, CMake

## Local Development Against an Unpublished Version

To use a local checkout while iterating on the library, add a `[patch.crates-io]` section to your application's `Cargo.toml`:

```toml
[dependencies]
vtx-engine = "0.1"

[patch.crates-io]
vtx-engine = { path = "../vtx-engine/crates/vtx-engine" }
```

Remove the `[patch.crates-io]` block before committing or releasing.

## License

[MIT](LICENSE)
