# vtx-engine Usage Guide

This guide covers the two primary integration patterns for vtx-engine:

1. **Real-Time Dictation** — short-burst VAD-driven microphone dictation (FlowSTT-style).
2. **Stream Transcription** — post-capture or encoder-tee transcription with timestamped segments (OmniRec-style).

---

## Quick Start: Adding vtx-engine as a Dependency

```toml
# Cargo.toml
[dependencies]
vtx-engine = { path = "../vtx-engine" }   # or version = "0.2.0" once published
vtx-common = { path = "../vtx-common" }
tokio = { version = "1", features = ["full"] }
```

---

## Real-Time Dictation

Use this pattern when you want short-burst microphone transcription — each
spoken utterance is transcribed and delivered as a `TranscriptionComplete` event.

```rust
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_common::{EngineEvent, TranscriptionProfile, WhisperModel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the model is available before building the engine.
    let mgr = ModelManager::new("my-app");
    if !mgr.is_available(WhisperModel::BaseEn) {
        println!("Downloading base.en model…");
        mgr.download(WhisperModel::BaseEn, |pct| print!("\r{}%  ", pct))
            .await?;
        println!("\nDone.");
    }

    // Build the engine with the Dictation profile.
    let (engine, mut rx) = EngineBuilder::new()
        .with_profile(TranscriptionProfile::Dictation)
        .build()
        .await?;

    // Subscribe to events in a background task.
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::TranscriptionComplete(result) => {
                    // `result.text` is the transcribed utterance.
                    // Paste it, log it, display it — whatever your app needs.
                    println!("[Dictation] {}", result.text);
                }
                EngineEvent::SpeechStarted => {
                    println!("[VAD] Speech started");
                }
                EngineEvent::SpeechEnded { duration_ms } => {
                    println!("[VAD] Speech ended ({}ms)", duration_ms);
                }
                _ => {}
            }
        }
    });

    // List available input devices and start capture on the first one.
    let devices = engine.list_input_devices();
    let device_id = devices
        .first()
        .map(|d| d.id.clone())
        .ok_or("No input devices found")?;

    engine.start_capture(Some(device_id), None).await?;

    println!("Capturing — speak into the microphone. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    engine.stop_capture().await?;
    Ok(())
}
```

### Dictation Profile Defaults

| Parameter | Value |
|---|---|
| `segment_max_duration_ms` | 4 000 ms |
| `word_break_segmentation_enabled` | `true` |
| `segment_word_break_grace_ms` | 750 ms |
| `model` | `WhisperModel::BaseEn` |
| VAD voiced threshold | -42 dB |
| VAD whisper threshold | -52 dB |

---

## Stream Transcription

Use this pattern when you have an existing audio source (encoder output,
pre-recorded buffer, etc.) and want timestamped segment output without a live
capture session.

**Input contract:** You are responsible for supplying 16 kHz mono f32 PCM.
The engine does not resample inside `transcribe_audio_stream`.

```rust
use std::time::Instant;
use tokio::sync::mpsc;
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_common::{EngineEvent, TranscriptionProfile, WhisperModel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the model is available.
    let mgr = ModelManager::new("my-app");
    if !mgr.is_available(WhisperModel::MediumEn) {
        println!("Downloading medium.en model…");
        mgr.download(WhisperModel::MediumEn, |pct| print!("\r{}%  ", pct))
            .await?;
        println!("\nDone.");
    }

    // Build the engine with the Transcription profile (no capture session needed).
    let (engine, mut rx) = EngineBuilder::new()
        .with_profile(TranscriptionProfile::Transcription)
        .build()
        .await?;

    // Subscribe to live segment events for a real-time transcript view.
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let EngineEvent::TranscriptionSegment(seg) = event {
                println!(
                    "[+{:.1}s] {}",
                    seg.timestamp_offset_ms as f64 / 1000.0,
                    seg.text
                );
            }
        }
    });

    // Create the channel. Your encoder/recorder sends 16kHz mono f32 frames here.
    let (tx, rx_audio) = mpsc::channel::<Vec<f32>>(64);

    let session_start = Instant::now();
    let handle = engine.transcribe_audio_stream(rx_audio, session_start);

    // ---- Simulate feeding audio frames (replace with your actual audio source) ----
    tokio::spawn(async move {
        // 1 second of silence at 16kHz = 16000 f32 samples
        let silence: Vec<f32> = vec![0.0f32; 16_000];
        // In a real integration this would come from your encoder's audio tee.
        for _ in 0..5 {
            if tx.send(silence.clone()).await.is_err() {
                break;
            }
        }
        // Dropping `tx` signals end of stream → engine flushes and resolves.
    });
    // -------------------------------------------------------------------------------

    // Await the final complete list of segments.
    let segments = handle.await?;
    println!("\n--- Final Transcript ---");
    for seg in &segments {
        println!("[{:.1}s] {}", seg.timestamp_offset_ms as f64 / 1000.0, seg.text);
    }

    Ok(())
}
```

### Transcription Profile Defaults

| Parameter | Value |
|---|---|
| `segment_max_duration_ms` | 15 000 ms |
| `word_break_segmentation_enabled` | `false` |
| `model` | `WhisperModel::MediumEn` |
| VAD voiced threshold | -42 dB |
| VAD whisper threshold | -52 dB |

---

## Model Management

`ModelManager` is a standalone struct — it does not require a running engine.
Use it in your settings UI or first-run setup wizard to manage Whisper models.

```rust
use vtx_engine::ModelManager;
use vtx_common::WhisperModel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mgr = ModelManager::new("my-app");

    // Resolve the expected file path (file need not exist).
    let path = mgr.path(WhisperModel::BaseEn);
    println!("base.en model path: {}", path.display());

    // Check availability.
    if mgr.is_available(WhisperModel::BaseEn) {
        println!("base.en is ready.");
    } else {
        println!("base.en not found — downloading…");

        // Download with a progress callback (called with 0..=100).
        mgr.download(WhisperModel::BaseEn, |pct| {
            print!("\rDownloading… {}%   ", pct);
        })
        .await?;
        println!("\nDownload complete.");
    }

    // List all models that are currently available on disk.
    let cached = mgr.list_cached();
    println!("Cached models: {:?}", cached);

    Ok(())
}
```

### `ModelManager` API Summary

| Method | Description |
|---|---|
| `ModelManager::new(app_name)` | Construct. Cache root: `{platform_cache}/{app_name}/whisper/` |
| `mgr.path(model)` | Returns `PathBuf` to `ggml-{slug}.bin` (file need not exist) |
| `mgr.is_available(model)` | `true` if file exists and has non-zero size |
| `mgr.list_cached()` | All available variants, in ascending size order |
| `mgr.download(model, on_progress)` | Async download from Hugging Face with progress callback |

### `transcribe_audio_stream` Input Contract

- Audio must be **16 kHz mono f32 PCM** — no resampling is done inside the engine.
- Frames may be any length; the engine accumulates them into an internal buffer.
- Drop the sender (`tx`) to signal end of stream. The `JoinHandle` resolves with the complete segment list.

---

## Profiles Reference

```rust
use vtx_common::TranscriptionProfile;
use vtx_engine::EngineBuilder;

// Dictation (FlowSTT-style short-burst)
let builder = EngineBuilder::new().with_profile(TranscriptionProfile::Dictation);

// Long-form transcription (OmniRec-style)
let builder = EngineBuilder::new().with_profile(TranscriptionProfile::Transcription);

// Custom — no presets, set everything manually
let builder = EngineBuilder::new()
    .with_profile(TranscriptionProfile::Custom)
    .segment_max_duration_ms(10_000)
    .word_break_segmentation_enabled(false);
```

Individual setter calls placed **after** `with_profile` override the profile's
preset values. Setter calls placed **before** `with_profile` are overwritten by
the profile.

---

## WhisperModel Variants

| Variant | Size | Language |
|---|---|---|
| `TinyEn` | ~39 MB | English only |
| `Tiny` | ~39 MB | Multilingual |
| `BaseEn` | ~74 MB | English only (default) |
| `Base` | ~74 MB | Multilingual |
| `SmallEn` | ~244 MB | English only |
| `Small` | ~244 MB | Multilingual |
| `MediumEn` | ~769 MB | English only |
| `Medium` | ~769 MB | Multilingual |
| `LargeV3` | ~1.5 GB | Multilingual |
