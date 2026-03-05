# FlowSTT Migration to vtx-engine

This document outlines the high-level changes required in the FlowSTT project
to replace its internal engine implementation with `vtx-engine` as a shared
library dependency.

---

## Responsibility Boundary

### What vtx-engine Owns

- Whisper FFI (`libwhisper.so` / `whisper.dll` loading via `libloading`)
- VAD (voiced / whisper speech detection, word-break segmentation)
- Audio resampling and channel conversion to 16 kHz mono
- `EngineConfig` serialization and config file loading
- Model download and path resolution (`ModelManager`)
- Transcription history types (`HistoryEntry`, `TranscriptionHistory`)
- Audio ring buffer and segment extraction (`TranscribeState`)
- Broadcast event channel (`EngineEvent`)
- Push-to-talk state (`PushToTalkController`)

### What FlowSTT Keeps

- Tauri command layer (IPC server) — all `#[tauri::command]` definitions
- CLI entry point and argument parsing
- Auto-paste / clipboard integration
- Hotkey capture and global shortcut registration
- Application-level window management and tray icon
- FlowSTT-specific `AppConfig` fields not related to the engine
  (e.g. `auto_paste_enabled`, `tray_icon`, window geometry)
- Frontend (TypeScript/JavaScript UI)

---

## 1. Dependency Replacement

Remove FlowSTT's internal engine crates from its workspace:

```
# Remove from the workspace Cargo.toml members list:
"src-engine"
"src-common"
```

Add `vtx-engine` and `vtx-common` as Cargo dependencies:

```toml
[dependencies]
vtx-engine = { version = "0.2.0" }
vtx-common = { version = "0.2.0" }
```

All downstream crates that previously imported from `src-engine` or
`src-common` should be updated to import from `vtx_engine` and `vtx_common`.

---

## 2. Type Mapping

The following FlowSTT-internal types map directly to `vtx-common` equivalents:

| FlowSTT type | vtx-common equivalent |
|---|---|
| `src-common::EngineEvent` | `vtx_common::EngineEvent` |
| `src-common::TranscriptionResult` | `vtx_common::TranscriptionResult` |
| `src-common::AudioDevice` | `vtx_common::AudioDevice` |
| `src-common::RecordingMode` | `vtx_common::RecordingMode` |
| `src-common::TranscriptionMode` | `vtx_common::TranscriptionMode` |
| `src-common::AudioSourceType` | `vtx_common::AudioSourceType` |
| `src-common::HotkeyCombination` | `vtx_common::HotkeyCombination` |
| `src-common::KeyCode` | `vtx_common::KeyCode` |
| `src-common::ModelStatus` | `vtx_common::ModelStatus` |
| `src-common::GpuStatus` | `vtx_common::GpuStatus` |
| FlowSTT's internal `WhisperModel` | `vtx_common::WhisperModel` |
| FlowSTT's `HistoryEntry` | `vtx_common::HistoryEntry` |

**New `EngineEvent` variant:** `vtx_common::EngineEvent` now includes
`TranscriptionSegment(TranscriptionSegment)`. Any exhaustive `match` on
`EngineEvent` in FlowSTT code must add a handler arm for this variant
(typically `_ => {}` is sufficient for FlowSTT, which only uses dictation mode
and will never receive this event during normal operation).

---

## 3. Config Migration

### EngineConfig

`vtx_engine::EngineConfig` now includes all fields previously duplicated in
FlowSTT's internal config:

- `model: WhisperModel` — replaces FlowSTT's hardcoded `ggml-base.en.bin`
- `word_break_segmentation_enabled: bool` — controls word-break splitting
  (FlowSTT should set this to `true`, matching current behaviour)
- `segment_max_duration_ms`, `segment_word_break_grace_ms` — already exist

The deprecated `model_path` field is still read from serialised configs for
backward compatibility. Existing `vtx-engine.toml` files containing `model_path`
will continue to work. New files written by vtx-engine will use `model` instead.

### FlowSTT AppConfig

FlowSTT's `AppConfig` (window geometry, auto-paste, tray, etc.) is entirely
unrelated to `EngineConfig` and requires no changes. The two configs should
continue to live in separate TOML files:

- `{config_dir}/FlowSTT/vtx-engine.toml` — loaded by `EngineConfig::load("FlowSTT")`
- `{config_dir}/FlowSTT/app-config.toml` — FlowSTT-managed

### Model Download

Replace FlowSTT's current `download_model` Tauri command implementation
(which calls `AudioEngine::download_model`) with `ModelManager`:

```rust
// Before (internal engine API)
engine.download_model().await

// After (ModelManager)
use vtx_engine::ModelManager;
use vtx_common::WhisperModel;

let mgr = ModelManager::new("FlowSTT");
mgr.download(WhisperModel::BaseEn, |pct| {
    // emit progress event to frontend
}).await?;
```

`ModelManager::new("FlowSTT")` uses the same cache root that
`EngineConfig::load("FlowSTT")` / `EngineBuilder` will use when resolving the
model path, so there is no path mismatch.

---

## 4. IPC Server and CLI Ownership

The IPC server (named pipe / Unix socket listener) and CLI argument parser
remain fully owned by FlowSTT. vtx-engine has no IPC surface. FlowSTT's
Tauri commands (`start_capture`, `stop_capture`, `transcribe_file`, etc.)
continue to exist — they simply call `vtx_engine::AudioEngine` methods instead
of the old internal engine.

The rename `transcribe_file` → `transcribe_audio_file` affects the Tauri
command handler. The return type changes from `TranscriptionResult` to
`Vec<TranscriptionSegment>`. FlowSTT's frontend should be updated to handle
the new segment-list response (or the command can convert back to a single
`TranscriptionResult` by joining segment texts if needed).
