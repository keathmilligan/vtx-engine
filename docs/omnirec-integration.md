# OmniRec Integration with vtx-engine

This document outlines the high-level changes required in the OmniRec project
to adopt `vtx-engine` for transcription, replacing OmniRec's internal
`src-tauri/src/transcription/` module.

---

## Responsibility Boundary

### What vtx-engine Owns

- Whisper FFI (`libwhisper.so` / `whisper.dll` loading via `libloading`)
- VAD and audio segmentation
- Audio resampling to 16 kHz mono (for `transcribe_audio_file`; callers own
  resampling before `transcribe_audio_stream`)
- `ModelManager`: model enumeration, availability checks, async download
- Broadcast event channel (`EngineEvent`) including `TranscriptionSegment`
- CUDA DLL distribution (on Windows, vtx-engine ships the CUDA binaries)

### What OmniRec Keeps

- FFmpeg encoding pipeline, screen/region capture, video muxing
- Encoder audio tee (the `Sender<Vec<f32>>` fed into `transcribe_audio_stream`)
- Resampling from 48 kHz stereo to 16 kHz mono before feeding the channel
- Tauri command layer (IPC commands for recording control, settings, etc.)
- UI, transcript display, export to Markdown/SRT
- OmniRec-specific `AppConfig` fields (region, hotkeys, output format, etc.)

---

## 1. Transcription Module Removal

Remove `src-tauri/src/transcription/` entirely. This includes:

- `src-tauri/src/transcription/mod.rs`
- `src-tauri/src/transcription/whisper.rs` (or equivalent FFI wrapper)
- `src-tauri/src/transcription/download.rs` (model download logic)
- Any other files under that directory

Remove the corresponding Cargo.toml dependencies that were only needed by
the internal transcription module (e.g. `libloading`, `reqwest` for model
download, `rustfft` if it was used for VAD, `hound` if it was used for WAV
handling).

---

## 2. Dependency Addition

Add `vtx-engine` and `vtx-common` to `src-tauri/Cargo.toml`:

```toml
[dependencies]
vtx-engine = { version = "0.2.0" }
vtx-common = { version = "0.2.0" }
```

Initialise the engine in Tauri setup:

```rust
use vtx_engine::EngineBuilder;
use vtx_common::TranscriptionProfile;

let (engine, rx) = EngineBuilder::new()
    .with_profile(TranscriptionProfile::Transcription)
    .build()
    .await?;
```

---

## 3. Audio Stream Wiring

OmniRec's encoder already tees audio from the recording pipeline. The tee
sender must supply **16 kHz mono f32 PCM** — OmniRec currently performs a 3:1
integer decimation from 48 kHz to 16 kHz, which satisfies this contract.

```rust
use tokio::sync::mpsc;
use std::time::Instant;

// Create the channel at the start of a recording session.
let (tx, rx_audio) = mpsc::channel::<Vec<f32>>(64);
let session_start = Instant::now();

// Hand the receiver to the engine.
let transcription_handle = engine.transcribe_audio_stream(rx_audio, session_start);

// In the encoder thread / task, send resampled mono frames:
//   tx.send(resampled_mono_frames).await.ok();

// When recording stops, drop `tx`.
// The engine flushes the remaining audio and resolves the JoinHandle.
let segments = transcription_handle.await?;
```

The engine emits `EngineEvent::TranscriptionSegment` in real time as each
segment is processed, allowing the live transcript window to update without
waiting for the full `JoinHandle` to resolve.

---

## 4. Model Management

Replace OmniRec's internal model download Tauri commands with `ModelManager`:

```rust
use vtx_engine::ModelManager;
use vtx_common::WhisperModel;

// In Tauri command state setup:
let mgr = ModelManager::new("OmniRec");

// Check availability:
mgr.is_available(WhisperModel::MediumEn)

// Download with progress:
mgr.download(WhisperModel::MediumEn, move |pct| {
    let _ = app_handle.emit("model-download-progress", pct);
}).await?;

// List cached:
let cached = mgr.list_cached();
```

OmniRec's `transcription.model` config field (currently its own `WhisperModel`
enum) maps directly to `vtx_common::WhisperModel`. The `EngineBuilder::model`
setter or `TranscriptionProfile` can be used to configure which model the
engine loads.

---

## 5. Event-Driven Transcript Updates

Replace the polling-based `get_transcription_segments` Tauri command with a
subscription to `EngineEvent::TranscriptionSegment` on the broadcast channel:

```rust
use vtx_common::EngineEvent;

let ah = app_handle.clone();
vtx_engine::EventHandlerAdapter::new(rx, move |event| {
    if let EngineEvent::TranscriptionSegment(seg) = event {
        // Emit to frontend in real time.
        let _ = ah.emit("transcription-segment", &seg);
    }
}).spawn();
```

The `TranscriptionSegment` type carries:
- `id: String` — UUID v4
- `text: String` — transcribed text
- `timestamp_offset_ms: u64` — position within the recording session
- `duration_ms: u64` — length of the audio segment
- `audio_path: Option<String>` — path to saved WAV (if applicable)

The frontend can use `timestamp_offset_ms` to display segments in a timeline
and to render the final `.md` / `.srt` export with accurate timestamps.

---

## 6. CUDA Binary Consolidation

On Windows, vtx-engine ships the CUDA DLLs alongside the whisper.cpp shared
library. OmniRec should remove its own copies of these binaries:

- `src-tauri/cuda/*.dll` (cuBLAS, cuDNN, etc.)
- The Tauri `externalBin` / `resources` entries that bundled them

vtx-engine's build script handles CUDA DLL placement. OmniRec only needs to
ensure vtx-engine is a Cargo dependency — no additional binary bundling is
required.
