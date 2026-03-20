# vtx-engine Usage Guide

This guide covers the primary integration patterns for vtx-engine:

**Rust engine (`vtx-engine`)**

1. **Real-Time Dictation** — short-burst VAD-driven microphone dictation.
2. **Stream Transcription** — post-capture or encoder-tee transcription with timestamped segments.
3. **File Transcription** — transcribe a WAV file and get timestamped segments.
4. **File Playback** — route a WAV file through the full engine pipeline.
5. **Manual Recording** — application-controlled start/stop recording as an alternative to VAD.
6. **Model Management** — checking availability and downloading Whisper models.
7. **Config Persistence** — loading and saving `EngineConfig` to disk.
8. **Transcription History** — recording and managing a persistent history of transcription results.
9. **Subsystem Configuration** — disabling unused subsystems at build time.
10. **Device Testing** — testing audio input levels before starting a capture session.
11. **Audio Data Streaming** — real-time streaming of processed and/or raw audio samples for A/V muxing or custom processing.

**TypeScript visualization (`@vtx-engine/viz`)**

12. **Speech Activity Renderer** — scrollable canvas widget showing VAD state, signal metrics, and segment markers with full history scrollback.

---

## Quick Start: Adding vtx-engine as a Dependency

```toml
# Cargo.toml
[dependencies]
vtx-engine = "0.1"
tokio = { version = "1", features = ["full"] }
```

All public types (`EngineEvent`, `TranscriptionProfile`, `WhisperModel`, `RecordingMode`,
`EngineConfig`, etc.) are exported directly from the `vtx_engine` crate root.

---

## Real-Time Dictation

Use this pattern when you want short-burst microphone transcription — each spoken
utterance is transcribed and delivered as a `TranscriptionComplete` event.

```rust
use vtx_engine::{EngineBuilder, ModelManager};
use vtx_engine::{EngineEvent, TranscriptionProfile, WhisperModel};

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
        .app_name("my-app")
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
use vtx_engine::{EngineEvent, TranscriptionProfile, WhisperModel};

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
        .app_name("my-app")
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

### `transcribe_audio_stream` Input Contract

- Audio must be **16 kHz mono f32 PCM** — no resampling is done inside the engine.
- Frames may be any length; the engine accumulates them into an internal buffer.
- Drop the sender (`tx`) to signal end of stream. The `JoinHandle` resolves with the complete segment list.
- `TranscriptionSegment` events are also emitted on the broadcast channel in real time as each segment completes.
- Minimum buffer length to attempt transcription is ~500 ms (8 000 samples at 16 kHz).

---

## File Transcription

Use `transcribe_audio_file` to transcribe a WAV file directly without a capture session.
The file is loaded, resampled to 16 kHz mono, and run through a single Whisper inference pass.

```rust
use vtx_engine::EngineBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, _rx) = EngineBuilder::new()
        .app_name("my-app")
        .build()
        .await?;

    let segments = engine.transcribe_audio_file("recording.wav").await?;
    for seg in &segments {
        println!("[{:.1}s] {}", seg.timestamp_offset_ms as f64 / 1000.0, seg.text);
    }

    Ok(())
}
```

Returns `Ok(vec![])` for a silent file. Each segment carries a `timestamp_offset_ms`
relative to the start of the file and its `duration_ms`.

---

## File Playback

`play_file` routes a WAV file through the full engine pipeline — visualization,
VAD, and transcription — exactly as if the audio were being captured live.

Two modes are available:

- **VAD mode** (`ptt_mode = false`): The VAD drives automatic segmentation, just like live capture.
- **PTT mode** (`ptt_mode = true`): The entire file is submitted as a single recording segment.

```rust
use vtx_engine::{EngineBuilder, EngineEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, mut rx) = EngineBuilder::new()
        .app_name("my-app")
        .build()
        .await?;

    // Listen for transcription and playback-complete events.
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::TranscriptionComplete(result) => {
                    println!("[Transcription] {}", result.text);
                }
                EngineEvent::PlaybackComplete => {
                    println!("[Playback] Done");
                }
                _ => {}
            }
        }
    });

    // Play a file through the engine; VAD drives segmentation.
    engine.play_file("recording.wav", false)?;

    // Wait for playback to finish.
    while engine.is_playing_back() {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Ok(())
}
```

### `play_file` Notes

- Returns as soon as the feeder thread is spawned; poll `is_playing_back()` or listen for `PlaybackComplete` to detect completion.
- Calling `play_file` while playback is already in progress cancels the previous playback first.
- If no capture session is active, `play_file` starts an internal audio loop at the WAV file's native sample rate (no hardware device required).
- Call `stop_playback()` to cancel an active playback.

---

## Manual Recording

Use `start_recording()` / `stop_recording()` when segment boundaries should be
determined by application logic (a button press, a hotkey, etc.) rather than VAD.
While a manual recording session is active, VAD-driven segmentation is suppressed
and audio accumulates in a growable buffer (up to 30 minutes).

```rust
use vtx_engine::{EngineBuilder, EngineEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, mut rx) = EngineBuilder::new()
        .app_name("my-app")
        .build()
        .await?;

    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::RecordingStarted => {
                    println!("[Recording] Started");
                }
                EngineEvent::RecordingStopped { duration_ms } => {
                    println!("[Recording] Stopped after {}ms", duration_ms);
                }
                EngineEvent::TranscriptionComplete(result) => {
                    println!("[Transcription] {}", result.text);
                }
                _ => {}
            }
        }
    });

    let devices = engine.list_input_devices();
    engine.start_capture(devices.first().map(|d| d.id.clone()), None).await?;

    // Simulate PTT press/release (replace with your actual input handler).
    engine.start_recording();
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    engine.stop_recording(); // Submits accumulated audio for transcription.

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    engine.stop_capture().await?;
    Ok(())
}
```

### Manual Recording API

| Method | Description |
|---|---|
| `start_recording()` | Begin accumulating audio. Emits `RecordingStarted`. No-op if already recording. |
| `stop_recording()` | Stop and submit audio for transcription. Emits `RecordingStopped`. No-op if not recording. |
| `is_recording()` | Whether a manual recording session is currently active. |
| `get_last_recording_path()` | Path to the WAV file saved by the most recently completed recording, if any. |
| `finalize_segment()` | Stop recording (if active) and submit, or finalize any in-progress VAD segment. |

To disable VAD-driven auto-transcription entirely while still capturing audio, call
`engine.set_transcription_enabled(false)` before starting capture, then call
`engine.set_transcription_enabled(true)` before calling `start_recording()` /
`stop_recording()`.

---

## Model Management

`ModelManager` is a standalone struct — it does not require a running engine.
Use it in your settings UI or first-run setup wizard to manage Whisper models.

```rust
use vtx_engine::ModelManager;
use vtx_engine::WhisperModel;

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

### `ModelManager` API

| Method | Description |
|---|---|
| `ModelManager::new(app_name)` | Construct. Cache root: `{platform_cache}/{app_name}/whisper/` |
| `path(model)` | Returns `PathBuf` to `ggml-{slug}.bin` (file need not exist) |
| `is_available(model)` | `true` if file exists and has non-zero size |
| `list_cached()` | All available variants in ascending size order |
| `download(model, on_progress)` | Async download from Hugging Face with progress callback (0–100) |

Downloads write to a `.part` temporary file and atomically rename to the final path
on success — a partial download never appears as available. Returns
`ModelError::AlreadyDownloading` if a download for the same model is already in
progress on that `ModelManager` instance.

---

## Config Persistence

`EngineConfig` can be persisted to a TOML file in the platform-standard config
directory (`~/.config/{app_name}/` on Linux, `%APPDATA%\{app_name}\` on Windows,
`~/Library/Application Support/{app_name}/` on macOS).

```rust
use vtx_engine::EngineConfig;
use vtx_engine::{TranscriptionProfile, WhisperModel};

fn load_or_default() -> EngineConfig {
    // Loads from `{config_dir}/{app_name}/vtx-engine.toml` if it exists;
    // returns default otherwise.
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

## Transcription History

`TranscriptionHistory` is a bounded NDJSON-backed store for persisting dictation
results. `TranscriptionHistoryRecorder` subscribes to the engine event channel
and appends an entry for every `TranscriptionComplete` event automatically.

```rust
use std::sync::{Arc, Mutex};
use vtx_engine::{EngineBuilder, TranscriptionHistory, TranscriptionHistoryRecorder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, rx) = EngineBuilder::new()
        .app_name("my-app")
        .build()
        .await?;

    // Open (or create) the history store — bounded to 500 entries.
    let history = Arc::new(Mutex::new(
        TranscriptionHistory::open("my-app", 500)?
    ));

    // Spawn a recorder that writes an entry for every TranscriptionComplete event.
    TranscriptionHistoryRecorder::new(rx, history.clone()).start();

    // ... start capture, run dictation ...

    // Read history entries (most recent last).
    let h = history.lock().unwrap();
    for entry in h.entries() {
        println!("[{}] {}", entry.timestamp, entry.text);
    }

    Ok(())
}
```

### `TranscriptionHistory` API

| Method | Description |
|---|---|
| `TranscriptionHistory::open(app_name, max_entries)` | Open or create the history store |
| `entries()` | All entries in insertion order |
| `append(entry)` | Append a new entry; evicts oldest if at capacity |
| `delete(id)` | Remove entry by ID and delete its WAV file (if any) |
| `cleanup_wav_files(ttl)` | Delete WAV files older than `ttl`; clears `wav_path` on affected entries |

History is stored at `{data_dir}/{app_name}/history.ndjson`. WAV recordings
referenced by entries are stored under `{data_dir}/{app_name}/recordings/`.

---

## Profiles Reference

```rust
use vtx_engine::TranscriptionProfile;
use vtx_engine::EngineBuilder;

// Dictation (short-burst VAD-driven)
let builder = EngineBuilder::new().with_profile(TranscriptionProfile::Dictation);

// Long-form transcription
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
| `without_vad()` | No `SpeechStarted`/`SpeechEnded` events from VAD; manual recording still works |
| `with_audio_streaming()` | Emit `AudioData` events with processed mono samples (post-gain, post-AGC) |
| `with_raw_audio_streaming()` | Emit `RawAudioData` events with raw mono samples (pre-gain, pre-AGC) |

---

## Device Testing

Use `start_test_capture` to measure audio input levels on a device before starting
a full capture session. Level updates are emitted as `AudioLevelUpdate` events.

```rust
use vtx_engine::{EngineBuilder, EngineEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (engine, mut rx) = EngineBuilder::new()
        .without_transcription()
        .without_vad()
        .without_visualization()
        .build()
        .await?;

    let devices = engine.list_input_devices();
    let device_id = devices.first().map(|d| d.id.clone()).ok_or("No devices")?;

    engine.start_test_capture(device_id.clone())?;

    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let EngineEvent::AudioLevelUpdate { device_id, level_db } = event {
                println!("[{}] {:.1} dB", device_id, level_db);
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    engine.stop_test_capture()?;

    Ok(())
}
```

---

## Audio Data Streaming

Stream real-time audio samples from the engine's capture pipeline to your
application via the broadcast event channel. Two independent streams are
available:

- **Processed audio** (`AudioData`) — mono f32 samples after mic-gain and AGC
- **Raw audio** (`RawAudioData`) — mono f32 samples before mic-gain and AGC

Both are opt-in and disabled by default.

### Enabling audio streaming

```rust
use vtx_engine::{EngineBuilder, EngineEvent, StreamingAudioData};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Enable processed audio streaming (or use with_raw_audio_streaming(),
    // or both)
    let (engine, mut rx) = EngineBuilder::new()
        .with_audio_streaming()
        .without_transcription()
        .without_visualization()
        .build()
        .await?;

    let devices = engine.list_input_devices();
    engine.start_capture(devices.first().map(|d| d.id.clone()), None).await?;

    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::AudioData(data) => {
                    println!(
                        "chunk: {} samples @ {}Hz, offset={}",
                        data.samples.len(),
                        data.sample_rate,
                        data.sample_offset,
                    );
                }
                _ => {}
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    engine.stop_capture().await?;
    Ok(())
}
```

### Choosing between processed and raw streams

| Stream | Builder method | Event variant | Samples |
|---|---|---|---|
| Processed | `with_audio_streaming()` | `AudioData` | After mic-gain and AGC — matches what the user hears |
| Raw | `with_raw_audio_streaming()` | `RawAudioData` | Before mic-gain and AGC — unmodified capture data |

Enable both simultaneously to receive two independent streams from the same
audio source:

```rust
let (engine, mut rx) = EngineBuilder::new()
    .with_audio_streaming()
    .with_raw_audio_streaming()
    .build()
    .await?;
```

### A/V synchronization

Each audio chunk carries a `sample_offset` field — a cumulative count of
samples emitted since capture started (starting at 0). Compute the chunk
timestamp with:

```
timestamp_seconds = sample_offset / sample_rate
```

**Sync workflow for video recording apps:**

1. Observe `CaptureStateChanged { capturing: true }` — this is T=0 for the
   audio timeline.
2. Receive `AudioData` (or `RawAudioData`) events — each chunk's position is
   `sample_offset / sample_rate` seconds from T=0.
3. In your own video pipeline, record the wall-clock time of the first video
   frame relative to T=0.
4. Apply the delta as an A/V offset when muxing.

When both streams are enabled, `AudioData` and `RawAudioData` events for the
same audio chunk carry identical `sample_offset` values, allowing correlation.

### Audio format

| Property | Value |
|---|---|
| Sample rate | 48 000 Hz |
| Channels | 1 (mono) |
| Sample format | f32, range -1.0 to 1.0 |

---

## `AudioEngine` API Reference

| Method | Description |
|---|---|
| `list_input_devices()` | List available microphone/input devices |
| `list_system_devices()` | List available system audio (loopback/monitor) devices |
| `start_capture(source1, source2)` | Start capture from primary (and optional secondary) source |
| `stop_capture()` | Stop audio capture |
| `is_capturing()` | Whether audio capture is currently active |
| `subscribe()` | Obtain an additional broadcast receiver |
| `start_recording()` | Begin manual recording session |
| `stop_recording()` | Stop and submit manual recording for transcription |
| `is_recording()` | Whether a manual recording session is active |
| `get_last_recording_path()` | Path to the most recently saved recording WAV, if any |
| `finalize_segment()` | Stop recording (if active) or finalize the current VAD segment |
| `play_file(path, ptt_mode)` | Route a WAV file through the engine pipeline |
| `is_playing_back()` | Whether file playback is active |
| `stop_playback()` | Cancel active file playback |
| `transcribe_audio_file(path)` | Load a WAV, resample, and transcribe; returns `Vec<TranscriptionSegment>` |
| `transcribe_audio_stream(rx, start)` | Transcribe a channel of 16 kHz mono f32 PCM frames |
| `set_transcription_enabled(bool)` | Enable/disable real-time transcription without stopping capture |
| `is_transcription_enabled()` | Whether real-time transcription is currently enabled |
| `check_model_status()` | Check whether the configured Whisper model file is available |
| `check_gpu_status()` | Check CUDA/Metal acceleration status |
| `get_status()` | Current engine status snapshot |
| `start_test_capture(device_id)` | Start a lightweight level-reporting capture on a device |
| `stop_test_capture()` | Stop any active test capture |
| `shutdown()` | Request engine shutdown (also called automatically on `Drop`) |

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

---

## `EngineBuilder` Setters

| Setter | Description |
|---|---|
| `app_name(name)` | Set the application name for data directory resolution (default: `"vtx-engine"`) |
| `with_profile(profile)` | Apply a `TranscriptionProfile` preset |
| `model(model)` | Set Whisper model variant |
| `recording_mode(mode)` | Set `RecordingMode` |
| `vad_voiced_threshold_db(db)` | Set voiced speech threshold |
| `vad_whisper_threshold_db(db)` | Set whisper speech threshold |
| `vad_voiced_onset_ms(ms)` | Set voiced onset duration |
| `vad_whisper_onset_ms(ms)` | Set whisper onset duration |
| `segment_max_duration_ms(ms)` | Set maximum segment duration |
| `segment_word_break_grace_ms(ms)` | Set word-break grace period |
| `segment_lookback_ms(ms)` | Set lookback buffer duration |
| `transcription_queue_capacity(n)` | Set transcription queue depth |
| `viz_frame_interval_ms(ms)` | Set visualization frame interval |
| `word_break_segmentation_enabled(bool)` | Enable/disable word-break segmentation |
| `without_transcription()` | Disable transcription subsystem |
| `without_visualization()` | Disable visualization subsystem |
| `without_vad()` | Disable VAD subsystem |
| `with_audio_streaming()` | Enable processed audio data streaming (`AudioData` events) |
| `with_raw_audio_streaming()` | Enable raw audio data streaming (`RawAudioData` events) |

---

## Local Development Against an Unpublished Version

When you need to make changes to vtx-engine and test them in your application
simultaneously — without publishing to crates.io — use Cargo's
[`[patch.crates-io]`](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html)
mechanism.

In your **application's** root `Cargo.toml`, add a `[patch.crates-io]` table
that points the crate at its local path:

```toml
# my-app/Cargo.toml

[dependencies]
vtx-engine = "0.1"

# --- Local development override ---
# Point at a local checkout of vtx-engine while iterating on the library.
# Remove this section before committing or cutting a release.
[patch.crates-io]
vtx-engine = { path = "../vtx-engine/crates/vtx-engine" }
```

Adjust the relative path so it resolves correctly from your application's
workspace root. `[patch.crates-io]` must be declared at the workspace root
(the `Cargo.toml` that contains `[workspace]`), not in an individual crate.

### How it works

Cargo replaces the crates.io version of `vtx-engine` with the local source tree
for every build in the workspace. The version declared in `[dependencies]` is
still checked for compatibility with the local crate's `[package] version`, so
keep them in sync. No other changes to your code are required — `use vtx_engine::...`
imports continue to work unchanged.

### Workflow

```
my-app/                         vtx-engine/
  Cargo.toml  ──[patch]──▶        crates/vtx-engine/
  src/
```

1. Edit vtx-engine source normally.
2. Run `cargo build` (or `cargo tauri build`) in your application — Cargo picks
   up the local changes automatically.
3. When you are done, remove the `[patch.crates-io]` section and bump the
   version in `[dependencies]` to the newly published release.

---

## Speech Activity Renderer (`@vtx-engine/viz`)

`SpeechActivityRenderer` is a self-contained canvas widget that visualizes VAD
state, signal metrics, and segment markers in real time. It maintains a scrollable
history buffer so the user can pan back through the entire recording session.

### Installation

```sh
npm install @vtx-engine/viz
# or
pnpm add @vtx-engine/viz
```

### Importing

```ts
import { SpeechActivityRenderer } from "@vtx-engine/viz";
import type { SpeechMetrics } from "@vtx-engine/viz";

// Optional: import bundled CSS custom-property defaults
import "@vtx-engine/viz/styles";
```

### Construction

```ts
const canvas = document.getElementById("speech-canvas") as HTMLCanvasElement;

const renderer = new SpeechActivityRenderer(
  canvas,          // HTMLCanvasElement — required
  256,             // bufferSize: visible window width in frames (default 256, ~4s at 16ms/frame)
  108_000          // maxHistoryFrames: scrollback cap (default 108 000, ~30 min at 16ms/frame)
);
```

`maxHistoryFrames` is clamped to at least `bufferSize * 2`. When the cap is
reached the oldest `bufferSize` frames are dropped in a single batch, amortizing
the copy cost.

### Lifecycle

```ts
renderer.drawIdle();   // draw empty background immediately (before first data arrives)

renderer.start();      // begin requestAnimationFrame draw loop
// ... receive and push frames ...
renderer.stop();       // cancel rAF loop; triggers one final draw

renderer.clear();      // zero all history and scroll state; redraws idle background
renderer.resize();     // re-size canvas to match its CSS rect; call on window resize
```

`renderer.active` is `true` while the rAF loop is running.

### Feeding data

```ts
// Called once per visualization frame from the engine event channel.
// metrics must be 16 kHz, normalized per the SpeechMetrics contract.
renderer.pushMetrics(metrics);         // SpeechMetrics

// Call whenever a VAD segment is submitted for transcription.
renderer.markSegmentSubmitted();
```

Internally, `pushMetrics` writes into a 20-frame delay buffer before appending
to the history arrays. This lookback ensures that the speech bar accurately
reflects the VAD's 200 ms pre-speech buffer.

#### `SpeechMetrics` fields

| Field | Type | Description |
|---|---|---|
| `amplitude_db` | `number` | RMS amplitude in dB |
| `zcr` | `number` | Zero-crossing rate (0.0–0.5) |
| `centroid_hz` | `number` | Spectral centroid in Hz |
| `is_speaking` | `boolean` | VAD confirmed speech |
| `voiced_onset_pending` | `boolean` | Voiced onset detection in progress |
| `whisper_onset_pending` | `boolean` | Whisper onset detection in progress |
| `is_transient` | `boolean` | Frame flagged as transient (rejected by VAD) |
| `is_lookback_speech` | `boolean` | Frame is in the lookback pre-speech buffer |
| `is_word_break` | `boolean` | Frame is an inter-word pause during speech |

### Configuration

```ts
// Call whenever the visualization frame interval changes (e.g. from VisualizationData payload).
renderer.configure(frameIntervalMs);
```

`frameIntervalMs` defaults to `16`. It controls the x-axis time labels (computed
dynamically as `t = -(col + scrollOffset) * frameIntervalMs / 1000`).

### Scroll API

```ts
renderer.scrollBy(deltaFrames);  // positive = into history; negative = toward live
renderer.resetToLive();          // snap back to the live (rightmost) edge
renderer.isLive;                 // true when scrollOffset === 0
renderer.bufferFrames;           // read-only: visible window width (= bufferSize)
```

`scrollBy` clamps to `[0, totalFrames − bufferSize]`, so scrolling past the
oldest available frame is not possible. While `isLive` is `true` the canvas
automatically advances with each new frame; once scrolled back the view is
anchored in history.

### Wiring scroll controls

The renderer exposes a `scrollAccum` field to accumulate sub-frame fractional
pixel deltas from wheel and pointer-drag handlers. Pattern used in `vtx-demo`:

```ts
function setupScrollHandlers(canvas: HTMLCanvasElement) {
  const framesPerPixel = () => renderer.bufferFrames / canvas.clientWidth;

  // Mouse wheel
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const pixels = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    renderer.scrollAccum += pixels * framesPerPixel();
    const whole = Math.trunc(renderer.scrollAccum);
    if (whole !== 0) {
      renderer.scrollAccum -= whole;
      renderer.scrollBy(whole);
    }
  }, { passive: false });

  // Pointer drag
  let dragStartX = 0;
  canvas.addEventListener("pointerdown", (e) => {
    dragStartX = e.clientX;
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!(e.buttons & 1)) return;
    const delta = dragStartX - e.clientX;  // drag left = positive = into history
    dragStartX = e.clientX;
    renderer.scrollAccum += delta * framesPerPixel();
    const whole = Math.trunc(renderer.scrollAccum);
    if (whole !== 0) {
      renderer.scrollAccum -= whole;
      renderer.scrollBy(whole);
    }
  });
  canvas.addEventListener("pointerup", () => {
    if (renderer.isLive) renderer.resetToLive();
  });
}
```

Button controls are simpler:

```ts
btnScrollBack.addEventListener("click", () =>
  renderer.scrollBy(Math.round(renderer.bufferFrames / 4))
);
btnScrollFwd.addEventListener("click", () =>
  renderer.scrollBy(-Math.round(renderer.bufferFrames / 4))
);
btnScrollLive.addEventListener("click", () => renderer.resetToLive());
```

### Visual layers

The canvas is partitioned top-to-bottom:

| Layer | Height | What is drawn |
|---|---|---|
| Speech bar | top 8% | Confirmed speech (green) and lookback regions (blue) |
| Word-break bar | next 8% | Inter-word gaps during speech (orange) |
| Metrics area | remaining 84% | Amplitude line (amber), ZCR line (cyan), spectral centroid line (fuchsia); voiced, whisper, and transient onset markers as dots; voiced/whisper threshold dashed lines |
| Segment markers | full height | Dashed white vertical line + downward triangle at each `markSegmentSubmitted()` call |
| Scroll indicator | top-right overlay | `● LIVE` (green) when live; `◀ -Xs` when scrolled into history |

### Scroll indicator

The overlay is drawn automatically by `draw()` and requires no DOM elements:

- At the live edge: `● LIVE` — green text
- When scrolled back: `◀ -Xs` where X is the scroll depth in seconds

### CSS theming

All colors are read from CSS custom properties at draw time so they adapt to
your application's theme:

| Variable | Default | Element |
|---|---|---|
| `--vtx-waveform-bg` | `#1e293b` | Canvas background |
| `--vtx-waveform-grid` | `rgba(255,255,255,0.08)` | Grid lines |
| `--vtx-waveform-text` | `rgba(255,255,255,0.5)` | Axis labels and scroll indicator |
| `--vtx-threshold-line` | `rgba(255,255,255,0.15)` | Voiced/whisper threshold lines |
| `--vtx-speech-confirmed` | `rgba(34,197,94,0.5)` | Confirmed speech bar |
| `--vtx-speech-lookback` | `rgba(59,130,246,0.7)` | Lookback speech bar |
| `--vtx-speech-word-break` | `rgba(249,115,22,0.85)` | Word-break bar |
| `--vtx-metric-amplitude` | `rgba(245,158,11,0.75)` | Amplitude line |
| `--vtx-metric-zcr` | `rgba(6,182,212,0.75)` | ZCR line |
| `--vtx-metric-centroid` | `rgba(217,70,239,0.75)` | Spectral centroid line |
| `--vtx-marker-voiced` | `rgba(34,197,94,0.7)` | Voiced onset dots |
| `--vtx-marker-whisper` | `rgba(59,130,246,0.7)` | Whisper onset dots |
| `--vtx-marker-transient` | `rgba(239,68,68,0.7)` | Transient dots |
| `--vtx-segment-marker` | `rgba(255,255,255,0.85)` | Segment submission markers |

Override any variable on the canvas element or a parent:

```css
#speech-canvas {
  --vtx-speech-confirmed: rgba(16, 185, 129, 0.6);
  --vtx-waveform-bg: #0f172a;
}
```

### `SpeechActivityRenderer` API reference

#### Constructor

| Parameter | Type | Default | Description |
|---|---|---|---|
| `canvas` | `HTMLCanvasElement` | — | Target canvas element |
| `bufferSize` | `number` | `256` | Visible window width in frames |
| `maxHistoryFrames` | `number` | `108_000` | Maximum scrollback depth in frames |

#### Properties

| Name | Type | Description |
|---|---|---|
| `frameIntervalMs` | `number` | Expected ms between frames; controls x-axis labels |
| `scrollAccum` | `number` | Fractional frame accumulator for sub-pixel scroll handlers |
| `active` | `boolean` (read-only) | `true` while the rAF loop is running |
| `isLive` | `boolean` (read-only) | `true` when scroll offset is 0 (live edge) |
| `bufferFrames` | `number` (read-only) | Visible window width (`bufferSize`) |

#### Methods

| Method | Description |
|---|---|
| `start()` | Start the rAF draw loop. No-op if already active |
| `stop()` | Stop the rAF loop and trigger one final draw |
| `clear()` | Zero all history, reset scroll state, redraw idle background |
| `resize()` | Resize canvas to its CSS layout rect and redraw idle background |
| `drawIdle()` | Draw the empty background state immediately |
| `configure(frameIntervalMs)` | Update the frame interval used for x-axis labels |
| `pushMetrics(metrics)` | Feed one frame of `SpeechMetrics` into the history buffer |
| `markSegmentSubmitted()` | Record a segment-submission marker at the current frame position |
| `scrollBy(deltaFrames)` | Scroll by `deltaFrames` (clamped to valid range) |
| `resetToLive()` | Snap scroll offset back to the live edge |
