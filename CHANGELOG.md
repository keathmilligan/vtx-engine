# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `EngineBuilder` fluent API for constructing `AudioEngine` with opt-in subsystems and full configuration surface
- `PushToTalkController` — application-supplied `press()`/`release()`/`set_active()` signals for PTT segmentation
- `broadcast::Sender<EngineEvent>` as internal event bus; `AudioEngine::subscribe()` for multi-consumer event delivery
- `EventHandlerAdapter` for callback-style event consumption
- `TranscriptionHistory` — bounded NDJSON-backed history store with WAV TTL cleanup
- `TranscriptionHistoryRecorder` — auto-records `TranscriptionComplete` events into history
- `EngineConfig::load()` / `EngineConfig::save()` TOML persistence via platform-standard config directories
- `TranscriptionMode` enum (`Automatic`, `PushToTalk`) in `vtx-common`
- `KeyCode` and `HotkeyCombination` types in `vtx-common`
- `HistoryEntry` type in `vtx-common`
- `TranscriptionResult.id` and `TranscriptionResult.timestamp` optional fields

### Changed
- **BREAKING**: `AudioEngine::new()` now returns `(AudioEngine, broadcast::Receiver<EngineEvent>)` instead of taking an `EventHandler`
- **BREAKING**: Removed `EventHandler` trait — use `subscribe()` or `EventHandlerAdapter` instead
- **BREAKING**: Removed `aec_enabled` from `EngineConfig` — use `RecordingMode::EchoCancel` instead
- `EngineConfig` now derives `Serialize`/`Deserialize` with `#[serde(default)]` on all fields
- Expanded `EngineConfig` with all tunable VAD, segment, queue, and visualization parameters
