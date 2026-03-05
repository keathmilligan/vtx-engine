## Why

vtx-engine was extracted from FlowSTT but its API still reflects FlowSTT's single use-case: real-time microphone dictation with short VAD-driven segments. OmniRec has independently reimplemented the same whisper.cpp FFI, VAD logic, and audio pipeline code — meaning two separate projects now maintain diverging copies of the same engine. This change refactors vtx-engine into a proper shared library whose public API can serve both use cases cleanly, eliminating the duplication and establishing vtx-engine as the authoritative voice processing component.

## What Changes

- **BREAKING** Introduce a `TranscriptionProfile` system replacing hardcoded VAD/segmentation defaults, allowing callers to select `Dictation` (short-burst, low-latency) or `Transcription` (long-form, timestamped) profiles or supply fully custom parameters.
- **BREAKING** Rename `AudioEngine::transcribe_file` to `AudioEngine::transcribe_audio_file` and change its return type to include optional timestamp data via a new `TimestampedTranscriptionResult`.
- Add `AudioEngine::transcribe_audio_stream` — a new method that accepts a channel of audio frames and produces timestamped segments incrementally, needed by OmniRec's encode-then-transcribe pipeline.
- Add `EngineEvent::TranscriptionSegment` variant carrying per-segment timestamp offset, enabling OmniRec's live transcript window to receive real-time updates via the existing broadcast channel.
- Add `EngineConfig::max_segment_duration_ms` field (currently hardcoded to 4000ms in FlowSTT, 15000ms in OmniRec).
- Add `EngineConfig::model` field (typed enum `WhisperModel`) replacing the opaque `model_path: Option<PathBuf>`, with automatic path resolution; the `WhisperModel` enum covers all variants (tiny-en through large-v3).
- Add `EngineConfig::word_break_segmentation_enabled` field — OmniRec currently discards word-break events; FlowSTT relies on them. This makes the behavior explicit.
- Add `TranscriptionResult::timestamp_offset_ms: Option<u64>` — relative offset from the recording/session start; `None` for real-time dictation sessions, `Some(ms)` for file/stream transcription.
- Extract `ModelManager` as a first-class public API: `ModelManager::download(model, on_progress)`, `ModelManager::is_available(model)`, `ModelManager::path(model)`, `ModelManager::list_cached()` — replacing ad-hoc download logic duplicated in both projects.
- Add `AudioEngine::from_audio_data` — static method that transcribes a pre-captured `Vec<f32>` (16kHz mono) directly without a running capture session, needed for OmniRec's post-recording transcription path.
- Publish vtx-engine and vtx-common to crates.io and @vtx-engine/viz to npm so FlowSTT and OmniRec can depend on released versions rather than path deps.
- Write a `USAGE.md` integration guide documenting both the dictation and transcription integration patterns.
- Write high-level change outlines for FlowSTT and OmniRec describing what each project needs to change to adopt vtx-engine.

## Capabilities

### New Capabilities

- `transcription-profiles`: A `TranscriptionProfile` enum (`Dictation`, `Transcription`, `Custom`) and associated preset parameter sets that configure VAD thresholds, segment duration, model defaults, and whisper params in one shot. Applied via `EngineBuilder::with_profile(profile)`.
- `model-manager`: Public `ModelManager` API for enumerating, downloading, checking, and resolving all whisper model variants. Replaces duplicated download logic in both consumer projects.
- `audio-stream-transcription`: `AudioEngine::transcribe_audio_stream(rx: Receiver<Vec<f32>>) -> impl Stream<Item = TranscriptionSegment>` — enables OmniRec's pattern of tee-ing audio from the encoder into the transcription pipeline without a live capture session.
- `timestamped-segments`: New `TranscriptionSegment` type (id, text, timestamp_offset_ms, duration_ms, audio_path) and `EngineEvent::TranscriptionSegment` variant, supporting timestamped output for OmniRec-style transcripts.
- `usage-documentation`: `USAGE.md` at crate root documenting the two primary integration patterns (real-time dictation, post-capture transcription) with full working code examples.
- `consumer-change-outlines`: `docs/flowstt-migration.md` and `docs/omnirec-integration.md` outlining at a high level what each project must change to adopt vtx-engine as a dependency.

### Modified Capabilities

- `engine-builder`: Add `with_profile(TranscriptionProfile)` setter; add `max_segment_duration_ms`, `word_break_segmentation_enabled`, `model` fields to `EngineConfig`; deprecate `model_path` in favour of `model`.
- `broadcast-events`: Add `TranscriptionSegment` variant to `EngineEvent`; add `timestamp_offset_ms: Option<u64>` to existing `TranscriptionComplete`.
- `engine-config-persistence`: `WhisperModel` enum must serialize/deserialize correctly; `model_path` → `model` migration on load.

## Impact

**vtx-engine / vtx-common crates:**
- `EngineConfig` struct gains new fields (additive, backward-compatible via serde defaults)
- `EngineEvent` enum gains a new variant (`TranscriptionSegment`) — **BREAKING** for exhaustive match arms in consumer code
- `TranscriptionResult` gains `timestamp_offset_ms: Option<u64>` (additive)
- `AudioEngine` gains new methods; `transcribe_file` renamed
- New `ModelManager` type in public API surface
- New `TranscriptionProfile` enum and presets
- Build system: add publish configuration (Cargo.toml `[package]` metadata, license, description, keywords)

**FlowSTT:**
- Remove `src-engine/`, `src-common/` (or keep as thin shims), replace with `vtx-engine` dependency
- Map FlowSTT's `TranscriptionMode`, `RecordingMode`, `AudioSourceType`, `HotkeyCombination`, `EngineEvent` usages to vtx-common equivalents
- FlowSTT config migration: existing `config.json` fields map to `EngineConfig` + app-level config
- IPC server and CLI remain FlowSTT-owned; only the engine layer is replaced
- Whisper FFI, VAD, segmentation, history, and model download code all removed from FlowSTT

**OmniRec:**
- Add `vtx-engine` dependency; remove `src-tauri/src/transcription/` module entirely
- Wire OmniRec's encoder audio tee into `AudioEngine::transcribe_audio_stream`
- Replace OmniRec's model download commands with `ModelManager` API
- `TranscriptionSegment` events replace the current polling-based `get_transcription_segments` command
- OmniRec config `transcription.model: WhisperModel` maps directly to the new `WhisperModel` enum
- CUDA DLL distribution consolidated: vtx-engine owns the binaries; OmniRec removes its own copies

**Dependencies:**
- `vtx-engine` gains no new external crates; reorganizes existing logic
- Both consumer projects drop: `libloading`, `reqwest` (for model download), `rustfft`, `hound` from their own dependency lists (moved to vtx-engine)
