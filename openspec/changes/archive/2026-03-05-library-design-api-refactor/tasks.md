## 1. vtx-common: New Types

- [x] 1.1 Add `WhisperModel` enum to `vtx-common/src/types.rs` with all 9 variants and snake_case serde
- [x] 1.2 Add `TranscriptionProfile` enum to `vtx-common/src/types.rs` with `Dictation`, `Transcription`, `Custom` variants and snake_case serde
- [x] 1.3 Add `TranscriptionSegment` struct to `vtx-common/src/types.rs` with `id`, `text`, `timestamp_offset_ms`, `duration_ms`, `audio_path` fields
- [x] 1.4 Add `timestamp_offset_ms: Option<u64>` field with `#[serde(default)]` to existing `TranscriptionResult`
- [x] 1.5 Add `TranscriptionSegment(TranscriptionSegment)` variant to `EngineEvent` enum

## 2. vtx-engine: EngineConfig Updates

- [x] 2.1 Add `model: WhisperModel` field (default `WhisperModel::BaseEn`) to `EngineConfig` with `#[serde(default)]`
- [x] 2.2 Add `word_break_segmentation_enabled: bool` field (default `true`) to `EngineConfig` with `#[serde(default)]`
- [x] 2.3 Mark `model_path` as `#[deprecated]` and add `#[serde(default, skip_serializing_if = "Option::is_none")]`
- [x] 2.4 Update `EngineConfig::Default` impl to include new fields

## 3. vtx-engine: ModelManager

- [x] 3.1 Create `crates/vtx-engine/src/model_manager.rs` with `ModelManager` struct and `ModelError` enum
- [x] 3.2 Implement `ModelManager::new(app_name: &str) -> ModelManager` constructor
- [x] 3.3 Implement `ModelManager::path(model: WhisperModel) -> PathBuf` with correct `ggml-{slug}.bin` convention
- [x] 3.4 Implement `ModelManager::is_available(model: WhisperModel) -> bool`
- [x] 3.5 Implement `ModelManager::list_cached() -> Vec<WhisperModel>` returning models in size order
- [x] 3.6 Implement `ModelManager::download(model, on_progress) -> async Result<(), ModelError>` with atomic rename and `AlreadyDownloading` guard
- [x] 3.7 Export `ModelManager` and `ModelError` from `vtx-engine/src/lib.rs`

## 4. vtx-engine: EngineBuilder Updates

- [x] 4.1 Add `model(model: WhisperModel)` setter to `EngineBuilder`
- [x] 4.2 Add `word_break_segmentation_enabled(enabled: bool)` setter to `EngineBuilder`
- [x] 4.3 Add `with_profile(profile: TranscriptionProfile) -> Self` method applying preset values
- [x] 4.4 Update `EngineBuilder::build` to use `model` field (with `model_path` override fallback and tracing warn)

## 5. vtx-engine: AudioEngine API Changes

- [x] 5.1 Rename `transcribe_file` to `transcribe_audio_file`, change return type to `Result<Vec<TranscriptionSegment>, String>`, emit `EngineEvent::TranscriptionSegment` per segment
- [x] 5.2 Implement `AudioEngine::transcribe_audio_stream(rx: Receiver<Vec<f32>>, session_start: Instant) -> JoinHandle<Vec<TranscriptionSegment>>`
- [x] 5.3 Update audio loop to check `config.word_break_segmentation_enabled` before calling `ts.on_word_break`

## 6. vtx-demo: Event Handler Update

- [x] 6.1 Add `EngineEvent::TranscriptionSegment` arm to the `EventHandlerAdapter` match in `vtx-demo/src-tauri/src/lib.rs`
- [x] 6.2 Update `transcribe_file` Tauri command to call `transcribe_audio_file` and adapt result type

## 7. Documentation

- [x] 7.1 Create `USAGE.md` at workspace root with real-time dictation example, stream transcription example, and ModelManager section
- [x] 7.2 Create `docs/flowstt-migration.md` covering dependency replacement, type mapping, config migration, and IPC/CLI boundary
- [x] 7.3 Create `docs/omnirec-integration.md` covering transcription module removal, dependency addition, audio stream wiring, model management, event-driven segments, and CUDA consolidation
