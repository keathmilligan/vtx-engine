## 1. vtx-common Type Additions

- [x] 1.1 Add `TranscriptionMode` enum (`Automatic`, `PushToTalk`) to `vtx-common/src/types.rs` with `Serialize`, `Deserialize`, `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Default`
- [x] 1.2 Add `KeyCode` enum (full set matching FlowSTT) to `vtx-common/src/types.rs` with `Serialize`, `Deserialize`, `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Hash` and `display_name()` / `is_modifier()` methods
- [x] 1.3 Add `HotkeyCombination` struct with order-independent `PartialEq`, `Eq`, `Hash`, `display()` method, and `new()` / `single()` constructors
- [x] 1.4 Expand `TranscriptionResult` to add `id: Option<String>` and `timestamp: Option<String>` fields (both `#[serde(skip_serializing_if = "Option::is_none")]`)
- [x] 1.5 Add `HistoryEntry` struct (`id`, `text`, `timestamp`, `wav_path: Option<String>`) to `vtx-common/src/types.rs`
- [x] 1.6 Re-export all new types from `vtx-common/src/lib.rs`

## 2. EngineConfig and RecordingMode Refactor

- [x] 2.1 Remove `aec_enabled` field from `EngineConfig`; confirm `recording_mode: RecordingMode` (already exists) covers both `Mixed` and `EchoCancel`
- [x] 2.2 Expand `EngineConfig` with all tunable fields: `model_path: Option<PathBuf>`, `transcription_mode: TranscriptionMode`, `vad_voiced_threshold_db: f32`, `vad_whisper_threshold_db: f32`, `vad_voiced_onset_ms: u64`, `vad_whisper_onset_ms: u64`, `segment_max_duration_ms: u64`, `segment_word_break_grace_ms: u64`, `segment_lookback_ms: u64`, `transcription_queue_capacity: usize`, `viz_frame_interval_ms: u64`
- [x] 2.3 Apply `#[serde(default)]` to all `EngineConfig` fields; derive `Serialize`, `Deserialize`
- [x] 2.4 Update `Default` impl to use previously hardcoded values as named defaults
- [x] 2.5 Propagate new config fields from `EngineConfig` through `AudioEngine` construction to `SpeechDetector`, `VisualizationProcessor`, `TranscriptionQueue`, and `TranscribeState`
- [x] 2.6 Update `AudioBackend` trait and platform implementations to accept `RecordingMode` instead of separate `set_aec_enabled` / `set_recording_mode` calls; consolidate into single `set_recording_mode(RecordingMode)` call before `start_capture_sources`

## 3. Broadcast Event Channel

- [x] 3.1 Add `tokio::sync::broadcast` sender to `AudioEngine` internal state (`Arc<broadcast::Sender<EngineEvent>>`) with capacity 256
- [x] 3.2 Implement `AudioEngine::subscribe() -> broadcast::Receiver<EngineEvent>`
- [x] 3.3 Replace all `event_handler.on_event(...)` calls in the audio loop thread with `sender.send(...)`
- [x] 3.4 Replace all `event_handler.on_event(...)` calls in the transcription worker with `sender.send(...)`
- [x] 3.5 Replace all `event_handler.on_event(...)` calls in model download with `sender.send(...)`
- [x] 3.6 Remove `EventHandler` trait from `vtx-engine/src/lib.rs` and all internal usages
- [x] 3.7 Add `EventHandlerAdapter` struct that wraps a `broadcast::Receiver` and a `FnMut(EngineEvent) + Send + 'static` closure; implement `spawn()` to drive it in a tokio task with `Lagged` warning logging

## 4. EngineBuilder

- [x] 4.1 Create `vtx-engine/src/builder.rs` with `EngineBuilder` struct holding all `EngineConfig` fields plus subsystem enable flags (`transcription_enabled`, `visualization_enabled`, `vad_enabled`)
- [x] 4.2 Implement `EngineBuilder::new()` returning defaults with all subsystems enabled
- [x] 4.3 Implement all setter methods (one per `EngineConfig` field + `without_transcription()`, `without_visualization()`, `without_vad()`) each returning `Self`
- [x] 4.4 Implement `EngineBuilder::from_config(config: EngineConfig) -> Self`
- [x] 4.5 Implement `EngineBuilder::build(self) -> Result<(AudioEngine, broadcast::Receiver<EngineEvent>), String>` — initializes platform backend, conditionally initializes transcription worker, returns engine + receiver
- [x] 4.6 Update `AudioEngine::new(config: EngineConfig) -> Result<(AudioEngine, broadcast::Receiver<EngineEvent>), String>` to delegate to `EngineBuilder::from_config(config).build().await`
- [x] 4.7 Update audio loop to skip VAD processing when `vad_enabled = false`
- [x] 4.8 Update audio loop to skip visualization processing when `visualization_enabled = false`
- [x] 4.9 Update `AudioEngine` initialization to skip transcription worker when `transcription_enabled = false`
- [x] 4.10 Export `EngineBuilder` from `vtx-engine/src/lib.rs`

## 5. PushToTalkController

- [x] 5.1 Create `vtx-engine/src/ptt.rs` with `PttState` struct (`is_active: bool`) and `PushToTalkController` holding `Arc<Mutex<PttState>>` and a clone of the broadcast sender
- [x] 5.2 Implement `PushToTalkController::press()` — sets `is_active = true`, emits `SpeechStarted`, signals `TranscribeState` to open a segment; no-op if already pressed
- [x] 5.3 Implement `PushToTalkController::release()` — sets `is_active = false`, emits `SpeechEnded`, signals `TranscribeState` to finalize and submit segment; no-op if not pressed
- [x] 5.4 Implement `PushToTalkController::set_active(bool)` as thin wrapper over `press()` / `release()`
- [x] 5.5 Derive `Clone` for `PushToTalkController`; ensure `Send + 'static`
- [x] 5.6 Implement `AudioEngine::ptt_controller() -> PushToTalkController`
- [x] 5.7 Update audio loop: when `transcription_mode == TranscriptionMode::PushToTalk`, suppress VAD-based `SpeechStarted` / `SpeechEnded` events and segment submission
- [x] 5.8 Export `PushToTalkController` from `vtx-engine/src/lib.rs`

## 6. Engine Config Persistence

- [x] 6.1 Add `toml` crate dependency to `vtx-engine/Cargo.toml` (for serialization)
- [x] 6.2 Create `vtx-engine/src/config_persistence.rs` with `ConfigError` enum (`Io(std::io::Error)`, `Parse(String)`, `NoProjectDir`, `Serialize(String)`)
- [x] 6.3 Implement `EngineConfig::load(app_name: &str) -> Result<EngineConfig, ConfigError>` using `directories::ProjectDirs`; return `EngineConfig::default()` when file absent
- [x] 6.4 Implement `EngineConfig::save(&self, app_name: &str) -> Result<(), ConfigError>`; create parent directories if absent; serialize to TOML
- [x] 6.5 Export `ConfigError` from `vtx-engine/src/lib.rs`
- [x] 6.6 Add `directories` crate to `vtx-engine/Cargo.toml` if not already present (it is — verify)

## 7. Transcription History

- [x] 7.1 Add `uuid` crate (`features = ["v4"]`) and `chrono` (already present) to `vtx-engine/Cargo.toml`
- [x] 7.2 Create `vtx-engine/src/history.rs` with `HistoryError` enum (`Io(std::io::Error)`, `Parse(String)`, `NoProjectDir`)
- [x] 7.3 Implement `TranscriptionHistory::open(app_name: &str, max_entries: usize) -> Result<Self, HistoryError>`; resolve data dir via `directories::ProjectDirs`, create if absent, read NDJSON file if present
- [x] 7.4 Implement `TranscriptionHistory::append(&mut self, entry: HistoryEntry)` — adds entry to in-memory VecDeque, evicts oldest if over capacity, appends to NDJSON file (rewrite on eviction)
- [x] 7.5 Implement `TranscriptionHistory::entries(&self) -> &[HistoryEntry]`
- [x] 7.6 Implement `TranscriptionHistory::delete(&mut self, id: &str) -> bool` — removes entry, deletes WAV file, rewrites NDJSON file
- [x] 7.7 Implement `TranscriptionHistory::cleanup_wav_files(&mut self, ttl: std::time::Duration)` — parse ISO 8601 timestamps via `chrono`, delete old WAV files, clear `wav_path`, rewrite NDJSON
- [x] 7.8 Create `TranscriptionHistoryRecorder` struct holding `broadcast::Receiver<EngineEvent>` and `Arc<Mutex<TranscriptionHistory>>`
- [x] 7.9 Implement `TranscriptionHistoryRecorder::start(self)` as a `tokio::spawn`-ed task that listens for `TranscriptionComplete` events and appends `HistoryEntry` (UUID v4 id, ISO 8601 timestamp, text, wav from `audio_path`)
- [x] 7.10 Export `TranscriptionHistory`, `TranscriptionHistoryRecorder`, `HistoryError` from `vtx-engine/src/lib.rs`

## 8. vtx-demo Update

- [x] 8.1 Update `apps/vtx-demo/src-tauri` to use `EngineBuilder` construction and `engine.subscribe()` for events
- [x] 8.2 Replace all `EventHandler` trait impl usage in vtx-demo with `EventHandlerAdapter` or direct `broadcast::Receiver` handling in a Tauri command context
- [x] 8.3 Update vtx-demo to use `PushToTalkController` for any PTT demonstration flow
- [x] 8.4 Verify `cargo check` passes for the full workspace after all changes

## 9. crates.io Publishing Metadata

- [x] 9.1 Add publishing metadata to `crates/vtx-common/Cargo.toml`: `description`, `keywords`, `categories`, `repository`, `documentation`, `homepage`, `license = "MIT"`; remove any `publish = false`
- [x] 9.2 Add publishing metadata to `crates/vtx-engine/Cargo.toml`: same fields; add `exclude` list for large binaries
- [x] 9.3 Create `CHANGELOG.md` at workspace root with an initial `## [Unreleased]` section
- [x] 9.4 Create `.github/workflows/publish.yml` triggered on `v*` tags; publish `vtx-common` then `vtx-engine` using `cargo publish` with `CARGO_REGISTRY_TOKEN` secret
- [x] 9.5 Verify `cargo publish --dry-run` succeeds for both crates (check for missing files, bad metadata)
