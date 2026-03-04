## Why

`vtx-engine` was extracted from FlowSTT but still reflects FlowSTT's internal conventions (Tauri-centric event delivery, hardcoded model paths, no crate versioning/publishing) rather than a general-purpose library contract. Making it a proper crates.io library with a clean, application-agnostic API allows FlowSTT to replace its in-tree engine (`flowstt-engine`) with `vtx-engine` as a versioned dependency, and enables future consumers without coupling them to any specific application model.

## What Changes

- **BREAKING** Redesign `EngineConfig` to expose all tunable parameters currently hardcoded: model path discovery strategy, VAD thresholds, segment timing (max duration, word-break grace, lookback), transcription queue capacity, and visualization frame rate
- **BREAKING** Replace the `EventHandler` callback trait with a `tokio::sync::broadcast` channel as the primary event delivery mechanism; retain the trait as an optional adapter
- Add `EngineBuilder` fluent API for constructing `AudioEngine` with optional subsystems (transcription, visualization, VAD)
- Add Push-to-Talk (PTT) support: a `PushToTalkController` that accepts a signal from the caller to open/close speech segments, matching FlowSTT's PTT mode
- Add hotkey/input-device-agnostic PTT signal interface (the engine accepts a boolean signal; the caller decides how to generate it)
- Add `RecordingMode` enum to `EngineConfig`: `Mixed` (combine sources) and `EchoCancel` (AEC, output only echo-cancelled primary)
- Add persistent configuration helpers (load/save `EngineConfig` to disk in a standard platform directory)
- Add transcription history store: optional bounded ring-buffer of `TranscriptionResult` entries with WAV file retention and TTL cleanup
- Add `AudioLevel` metering event for test-capture mode (already present; formalize in public API)
- Publish `vtx-common` and `vtx-engine` to crates.io with proper metadata (`description`, `keywords`, `categories`, `repository`, `documentation`, `license`)
- Add `CHANGELOG.md` and enforce semantic versioning

## Capabilities

### New Capabilities

- `engine-builder`: Fluent `EngineBuilder` API for constructing `AudioEngine` with opt-in subsystems and full configuration surface
- `ptt-control`: Push-to-Talk controller — application-supplied signal (bool) opens/closes speech segments; replaces VAD for segment boundaries when active
- `recording-mode`: `RecordingMode` configuration field (`Mixed` vs `EchoCancel`) controlling how dual audio sources are combined
- `engine-config-persistence`: Load/save `EngineConfig` to a platform-standard config directory (using `directories` crate); no-op if not requested
- `transcription-history`: Optional bounded history store of `TranscriptionResult` entries with per-entry WAV file paths and configurable TTL cleanup
- `broadcast-events`: Replace `EventHandler` trait-based delivery with a `tokio::sync::broadcast::Sender<EngineEvent>` as the canonical event bus; expose a `subscribe()` method on `AudioEngine`; provide `EventHandlerAdapter` for backward compat

### Modified Capabilities

<!-- No existing specs to delta against — this is the initial library design pass -->

## Impact

- **`vtx-engine` crate**: Major API surface changes to `AudioEngine`, `EngineConfig`, event delivery, and capture control
- **`vtx-common` crate**: Add `RecordingMode`, `TranscriptionMode`, `HotkeyCombination`/`KeyCode` types migrated from FlowSTT; align `AudioDevice`, `TranscriptionResult`, `VisualizationData`, `SpeechMetrics` field names with FlowSTT equivalents for drop-in compatibility
- **`vtx-demo` app**: Update to new builder API and broadcast event subscription model
- **crates.io publishing**: Both `vtx-common` and `vtx-engine` need `Cargo.toml` publishing metadata and a CI publishing workflow
- **FlowSTT (out of scope for implementation)**: Once published, FlowSTT can replace `flowstt-engine` with `vtx-engine = "<version>"` and adapt its IPC handlers to the new API
