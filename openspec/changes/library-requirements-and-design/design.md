## Context

`vtx-engine` was extracted from FlowSTT and mirrors its internal engine crate (`flowstt-engine`). The current state has several characteristics that make it unsuitable as a standalone published library:

- **Event delivery** uses a synchronous `EventHandler` callback trait called directly from audio and transcription threads. This is fine for tight in-process coupling (Tauri) but is awkward for async consumers and only allows a single listener.
- **`EngineConfig`** is shallow: only `aec_enabled` and `recording_mode` are exposed. VAD thresholds, segment timing, queue capacity, model path strategy, and viz frame rate are all hardcoded inside `lib.rs`, `processor.rs`, and `transcription/`.
- **PTT mode** exists as `set_ptt_mode(bool)` + `finalize_segment()` on `AudioEngine`, but the higher-level PTT lifecycle — open session on key-down, close + submit on key-up, support auto-toggle — is not part of the public API.
- **`vtx-common`** is missing types that FlowSTT needs for a drop-in swap: `TranscriptionMode`, `HotkeyCombination`/`KeyCode`, `HistoryEntry`, and aligned field names (`TranscriptionResult.id`, `timestamp`).
- **crates.io metadata** is absent from both `Cargo.toml` files; neither crate can be published.
- **No config persistence**. FlowSTT manages its own config file via `flowstt-common`. `vtx-engine` needs a first-party persistence helper so applications don't have to re-implement it.
- **No history store**. FlowSTT manages `TranscriptionHistory` internally. Providing this in the library keeps the feature available without duplicating it per-app.

FlowSTT's integration path is: replace `flowstt-engine` with `vtx-engine` in `Cargo.toml`, adapt its `ipc/handlers.rs` to the `AudioEngine` API, and remove `flowstt-engine` entirely. This constrains the design: the new API surface must be capable of expressing everything FlowSTT's handlers currently do.

## Goals / Non-Goals

**Goals:**
- Publish `vtx-common` and `vtx-engine` to crates.io with semver versioning
- Replace single-listener callback delivery with a multi-consumer broadcast channel
- Expose full engine configuration surface (VAD, segment timing, queue, model, viz)
- Add `EngineBuilder` for ergonomic construction with opt-in subsystems
- Add `PushToTalkController` with application-agnostic signal interface
- Add `RecordingMode` (`Mixed`, `EchoCancel`) to public config
- Add config persistence via platform-standard directories
- Add optional bounded transcription history store with WAV TTL
- Align `vtx-common` types with FlowSTT's common types for drop-in compatibility

**Non-Goals:**
- Modifying FlowSTT's codebase (this library must be ready for adoption; migration is a separate effort)
- Replacing whisper.cpp with another backend
- Adding new platform audio backends (WASAPI, CoreAudio, PipeWire are the supported set)
- Providing a CLI or IPC server (that remains FlowSTT-specific)
- Hotkey detection (the library accepts a boolean signal; the app supplies it)
- Async audio backends (the platform layer remains synchronous + thread-based)

## Decisions

### D1: Broadcast channel as primary event bus

**Decision**: Replace `EventHandler` trait with `tokio::sync::broadcast::Sender<EngineEvent>` internally. Expose `AudioEngine::subscribe() -> broadcast::Receiver<EngineEvent>`. Provide `EventHandlerAdapter` that wraps a `broadcast::Receiver` and calls a `FnMut(EngineEvent)` in a spawned task, for consumers that prefer the old callback style.

**Rationale**: Broadcast enables multiple independent consumers (e.g., IPC server + logging + Tauri frontend) without any coordination. It fits naturally into `tokio` async contexts. The `EventHandler` trait pattern required all event routing to live in a single implementation, which forced every FlowSTT handler to use a shared `Arc<AppHandle>`. Broadcast lets each subsystem subscribe independently.

**Alternatives considered**:
- *Keep `EventHandler` trait*: Cannot support multiple listeners without an explicit fan-out inside the implementation.
- *`tokio::sync::mpsc`*: Single consumer only; same limitation as the trait.
- *`async-broadcast`* (third-party): Provides bounded broadcast but adds a dependency. `tokio::sync::broadcast` is already in the dependency tree and sufficient.

**Trade-off**: `broadcast::Receiver` returns `Err(RecvError::Lagged)` if a slow receiver falls behind the ring buffer capacity. Fast senders (viz at ~60 fps) can overrun slow receivers. Mitigation: set a generous buffer (e.g. 256 events) and document that `Lagged` should be treated as a dropped-frames condition, not an error.

---

### D2: `EngineBuilder` with opt-in subsystems

**Decision**: Introduce `EngineBuilder` as the primary construction path. `AudioEngine::new()` is retained for backward compatibility but becomes a thin wrapper around `EngineBuilder::default().build().await`. Builder fields map to `EngineConfig`; subsystems (`transcription`, `visualization`, `vad`) are enabled by default but can be disabled with `without_transcription()`, `without_visualization()`, etc.

**Rationale**: Many embedding scenarios only need a subset of capabilities (e.g., a dictation app needs transcription but not visualization; a meter app needs only `AudioLevel` events). Mandatory initialization of all subsystems wastes resources and makes testing harder. The builder also provides a natural place to configure the model path, VAD thresholds, and segment timing without requiring a fully-populated struct.

**Alternatives considered**:
- *Expand `EngineConfig`*: Sufficient for configuration, but doesn't solve optional subsystems.
- *Feature flags*: Compile-time opt-out is too coarse for runtime composition.

---

### D3: `PushToTalkController` as a separate type, not embedded in `AudioEngine`

**Decision**: `PushToTalkController` is a standalone struct obtained from `AudioEngine::ptt_controller() -> PushToTalkController`. It holds an `Arc` to internal engine state. Callers invoke `ptt_controller.press()` / `ptt_controller.release()` (or `set_active(bool)`). The controller is `Clone + Send` so it can be moved to a hotkey listener thread.

**Rationale**: Embedding PTT state in `AudioEngine` entangles the audio capture lifecycle with the input-device lifecycle. A separate controller type makes the boundary explicit. It also allows the caller to hold the controller independently (e.g., pass it to a hotkey thread) without requiring a reference to the full engine.

**Alternatives considered**:
- *Methods directly on `AudioEngine`*: `engine.ptt_press()` / `engine.ptt_release()`. Simpler, but requires `Arc<AudioEngine>` everywhere PTT signals originate.
- *Channel-based signal injection*: Caller sends a `bool` into a `mpsc::Sender`. More loosely coupled but harder to use ergonomically.

---

### D4: `EngineConfig` expanded with `serde` support and platform-standard persistence

**Decision**: `EngineConfig` derives `Serialize`/`Deserialize`. Add `EngineConfig::load(app_name: &str) -> Result<EngineConfig, ConfigError>` and `config.save(app_name: &str) -> Result<(), ConfigError>` as methods. The storage path is `{config_dir}/{app_name}/vtx-engine.toml` where `config_dir` is resolved by the `directories` crate. The load path is the same directory. The `app_name` parameter decouples the library from any particular application name.

**Rationale**: FlowSTT has `config::load_config()` / `config::save_config()` using the `directories` crate. If `vtx-engine` provides the same pattern, FlowSTT's config module can delegate to it (or be replaced entirely). Requiring TOML (via `toml` crate) rather than JSON keeps configs human-editable.

**Alternatives considered**:
- *No persistence in the library*: Simpler, but every consumer re-implements it. Inconsistent storage locations across apps.
- *JSON via `serde_json`*: Already in the dependency tree, but less human-readable than TOML for config.

---

### D5: `TranscriptionHistory` as an opt-in, app-namespaced store

**Decision**: Add `TranscriptionHistory` as a separate type (not embedded in `AudioEngine`). It is constructed with `TranscriptionHistory::open(app_name: &str, max_entries: usize) -> Result<Self, HistoryError>`. WAV files are stored under `{data_dir}/{app_name}/recordings/`. A `cleanup_wav_files(ttl: Duration)` method removes files older than the TTL. The history store integrates with the engine via a `TranscriptionHistoryRecorder` that subscribes to the broadcast channel and appends entries automatically.

**Rationale**: History is optional. A metering-only integration should not pay for history storage. Keeping it as a separate type also allows it to be opened/queried independently of whether the engine is running (e.g., for a history viewer that doesn't start capture). The auto-recorder pattern (subscribe + record) mirrors how FlowSTT's `TranscriptionHistory` is used today.

---

### D6: `vtx-common` type alignment with FlowSTT

**Decision**: Add `TranscriptionMode` (`Automatic`, `PushToTalk`), `HotkeyCombination`, `KeyCode` to `vtx-common`. Expand `TranscriptionResult` to include optional `id: Option<String>`, `timestamp: Option<String>`. Add `HistoryEntry`. Rename nothing — existing field names are compatible.

**Rationale**: FlowSTT's IPC handlers reference these types directly. Adding them to `vtx-common` means FlowSTT can swap `use flowstt_common::*` to `use vtx_common::*` with minimal churn. The `HotkeyCombination`/`KeyCode` types are platform-agnostic and belong in the shared types layer.

---

### D7: Crates.io publishing via `cargo publish` in CI

**Decision**: Add publishing metadata to both `Cargo.toml` files. Use a GitHub Actions workflow (`.github/workflows/publish.yml`) triggered on version tags (`v*`). Publish `vtx-common` first, then `vtx-engine` (dependency order). No `publish = false` override; both crates are public.

**Rationale**: Manual publishing is error-prone. Tag-triggered CI is the standard crates.io pattern and ensures only tagged commits become releases.

## Risks / Trade-offs

**[Risk: Broadcast lag for high-frequency viz events]** → Mitigation: Use a broadcast channel capacity of 256+. Document that `Lagged` errors on viz receivers should be handled as dropped frames, not fatal errors. Consumers that cannot keep up should use a dedicated task to drain the channel.

**[Risk: Breaking API changes affect vtx-demo]** → Mitigation: Update `vtx-demo` in the same PR as the API changes. The demo serves as the integration test.

**[Risk: `EngineConfig` TOML serialization breaks across versions]** → Mitigation: Use `#[serde(default)]` on all new fields so old config files load without error. Document a migration note in `CHANGELOG.md` for any field rename.

**[Risk: `PushToTalkController` state diverges from `AudioEngine` internal state]** → Mitigation: The controller holds an `Arc<Mutex<PttState>>` that the audio loop reads directly. There is no separate copy of state to diverge.

**[Risk: `TranscriptionHistory` WAV file accumulation on long-running deployments]** → Mitigation: `cleanup_wav_files` is exposed and documented. The `TranscriptionHistoryRecorder` can be configured with an auto-cleanup interval.

**[Risk: `vtx-common` type additions require semver bump]** → Mitigation: Adding new types is semver-compatible (non-breaking additive change). New fields on existing structs require `#[serde(default)]` for deserialization compatibility and are considered additive.

## Migration Plan

1. All changes are in `vtx-engine` and `vtx-common`; `vtx-demo` is updated in-place to use the new API.
2. `vtx-demo` acts as the smoke test for the new builder API and broadcast subscription model.
3. Once all specs are implemented and `vtx-demo` compiles and runs, bump crate versions to `0.1.0` and publish to crates.io.
4. FlowSTT migration (out of scope): replace `flowstt-engine` in `Cargo.toml` with `vtx-engine = "0.1"`, update `use` imports, adapt IPC handlers to the new API.

## Open Questions

- **Model name/URL configurability**: Should `EngineConfig` expose the model download URL and filename (currently hardcoded in `transcriber.rs`), or is `model_path: Option<PathBuf>` override sufficient for v0.1?
- **`TranscriptionHistory` serialization format**: TOML is chosen for config; should history use JSON (for structured array queries) or SQLite (for large histories)? For v0.1, JSON newline-delimited (`ndjson`) is the simplest.
- **`vtx-viz` publishing**: The TypeScript `@vtx-engine/viz` package is out of scope for the Rust crates.io publish, but should it be published to npm concurrently? Deferred.
