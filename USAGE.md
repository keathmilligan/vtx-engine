# vtx-engine Usage Guide

This guide covers the primary integration patterns for vtx-engine:

1. **Real-Time Dictation** — short-burst VAD-driven microphone dictation (FlowSTT-style).
2. **Stream Transcription** — post-capture or encoder-tee transcription with timestamped segments (OmniRec-style).
3. **Push-to-Talk** — manual PTT segmentation as an alternative to VAD.
4. **Model Management** — checking availability and downloading Whisper models.
5. **Config Persistence** — loading and saving `EngineConfig` to disk.
6. **Subsystem Configuration** — disabling unused subsystems at build time.

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

Use this pattern when you want short-burst microphone transcription — each spoken
utterance is transcribed and delivered as a `TranscriptionComplete` event.

```rust
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_common::{EngineEvent, TranscriptionProfile, WhisperModel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the model is available before building the engine.
    let mgr = ModelManager::new("my-app");
    if !mgr.is_available(WhisperModel::BaseEn) {
        println!("Downloading base.en model...");
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
        println!("Downloading medium.en model...");
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

## Push-to-Talk

Use `TranscriptionMode::PushToTalk` when segment boundaries should be determined
by application logic (a hotkey, button, or gamepad input) rather than VAD.

```rust
use vtx_engine::EngineBuilder;
use vtx_common::{EngineEvent, TranscriptionMode};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, mut rx) = EngineBuilder::new()
        .transcription_mode(TranscriptionMode::PushToTalk)
        .build()
        .await?;

    // Get a cloneable, Send controller.
    let ptt = engine.ptt_controller();

    // Forward events.
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::TranscriptionComplete(result) => {
                    println!("[PTT] {}", result.text);
                }
                EngineEvent::SpeechStarted => println!("[PTT] recording..."),
                EngineEvent::SpeechEnded { duration_ms } => {
                    println!("[PTT] captured {}ms", duration_ms);
                }
                _ => {}
            }
        }
    });

    let devices = engine.list_input_devices();
    engine.start_capture(devices.first().map(|d| d.id.clone()), None).await?;

    // Simulate PTT press/release (replace with your actual input handler).
    ptt.press();
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    ptt.release();   // Submits segment for transcription.

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    engine.stop_capture().await?;
    Ok(())
}
```

### `PushToTalkController` API

| Method | Description |
|---|---|
| `press()` | Signal PTT key-down. Emits `SpeechStarted`. No-op if session already open. |
| `release()` | Signal PTT key-up. Submits audio, emits `SpeechEnded`. No-op if no session open. |
| `set_active(bool)` | Convenience: `true` → `press()`, `false` → `release()`. |
| `is_active()` | Whether a PTT session is currently open. |

`PushToTalkController` is `Clone + Send + 'static` — safe to share across threads and Tauri commands.

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
        println!("base.en not found — downloading...");

        // Download with a progress callback (called with 0..=100).
        mgr.download(WhisperModel::BaseEn, |pct| {
            print!("\rDownloading... {}%   ", pct);
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
| `path(model)` | Returns `PathBuf` to `ggml-{slug}.bin` (file need not exist) |
| `is_available(model)` | `true` if file exists and has non-zero size |
| `list_cached()` | All available variants in ascending size order |
| `download(model, on_progress)` | Async download from Hugging Face with progress callback (0–100) |

### `transcribe_audio_stream` Input Contract

- Audio must be **16 kHz mono f32 PCM** — no resampling is done inside the engine.
- Frames may be any length; the engine accumulates them into an internal buffer.
- Drop the sender (`tx`) to signal end of stream. The `JoinHandle` resolves with the complete segment list.
- `TranscriptionSegment` events are also emitted on the broadcast channel in real time as each segment completes.

---

## Config Persistence

`EngineConfig` can be persisted to a TOML file in the platform-standard config
directory (`~/.config/{app_name}/` on Linux, `%APPDATA%\{app_name}\` on Windows,
`~/Library/Application Support/{app_name}/` on macOS).

```rust
use vtx_engine::EngineConfig;
use vtx_common::{TranscriptionProfile, WhisperModel};

fn load_or_default() -> EngineConfig {
    // Loads from `{config_dir}/vtx-engine.toml` if it exists; returns default otherwise.
    EngineConfig::load("my-app").unwrap_or_default()
}

fn save_config(config: &EngineConfig) -> Result<(), vtx_engine::ConfigError> {
    config.save("my-app")
}
```

All `EngineConfig` fields use `#[serde(default)]`, so configs written by an
earlier version of the library are safe to load — missing fields are populated
with their current defaults.

The deprecated `model_path` field is honoured on load (it takes precedence over
`model` with a warning), preserving backward compatibility with any existing
serialized config files. Prefer `model` in new code.

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

## Subsystem Configuration

Disable unused subsystems at construction time to reduce resource usage:

```rust
use vtx_engine::EngineBuilder;

// Visualization only — no transcription, no VAD (e.g. audio level meter)
let (engine, mut rx) = EngineBuilder::new()
    .without_transcription()
    .without_vad()
    .build()
    .await?;

// Transcription only — no visualization (e.g. headless server)
let (engine, mut rx) = EngineBuilder::new()
    .without_visualization()
    .build()
    .await?;
```

| Toggle | Effect |
|---|---|
| `without_transcription()` | No `TranscriptionComplete` or `TranscriptionSegment` events; whisper.cpp not loaded |
| `without_visualization()` | No `VisualizationData` events |
| `without_vad()` | No `SpeechStarted`/`SpeechEnded` events from VAD; PTT still works |

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

---

## Full `EngineConfig` Fields

All fields can be set via `EngineBuilder` setters or by constructing `EngineConfig` directly.

| Field | Type | Default | Description |
|---|---|---|---|
| `model` | `WhisperModel` | `BaseEn` | Whisper model to use for transcription |
| `model_path` | `Option<PathBuf>` | `None` | **Deprecated** — explicit path override; takes precedence over `model` |
| `recording_mode` | `RecordingMode` | `Mixed` | `Mixed`: mix sources; `EchoCancel`: AEC on primary source |
| `transcription_mode` | `TranscriptionMode` | `Automatic` | `Automatic`: VAD-driven; `PushToTalk`: manual press/release |
| `vad_voiced_threshold_db` | `f32` | `-42.0` | Voiced speech detection threshold in dB |
| `vad_whisper_threshold_db` | `f32` | `-52.0` | Whisper/soft speech detection threshold in dB |
| `vad_voiced_onset_ms` | `u64` | `80` | Minimum voiced speech duration to confirm onset (ms) |
| `vad_whisper_onset_ms` | `u64` | `120` | Minimum whisper speech duration to confirm onset (ms) |
| `segment_max_duration_ms` | `u64` | `4 000` | Maximum segment duration before seeking a split (ms) |
| `segment_word_break_grace_ms` | `u64` | `750` | Grace period after max duration before forced submission (ms) |
| `segment_lookback_ms` | `u64` | `200` | Pre-speech lookback buffer duration (ms) |
| `transcription_queue_capacity` | `usize` | `8` | Maximum segments queued awaiting transcription |
| `viz_frame_interval_ms` | `u64` | `16` | Visualization frame interval (~60 fps) |
| `word_break_segmentation_enabled` | `bool` | `true` | Split segments at word-break pauses; set `false` for long-form transcription |
