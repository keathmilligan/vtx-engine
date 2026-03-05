## Context

vtx-engine was originally extracted from FlowSTT and its API surface mirrors exactly one use-case: short-burst real-time microphone dictation (2–4 second segments, auto-paste into the foreground window). OmniRec has since independently reimplemented the same whisper.cpp FFI, the same custom VAD, the same resampling and segmentation logic — differing only in parameter tuning (15-second max segments, no word-break splitting, `ggml-medium.en` as the default model).

Both projects today maintain separate, diverging codebases for the same core capability. The goal of this change is to make vtx-engine's public API expressive enough to serve both use-cases cleanly, then migrate each consumer onto it.

Key constraints:
- vtx-engine is a Rust library used as a Tauri backend dependency. It does not expose a Tauri command layer itself — consumers (FlowSTT, OmniRec) own their own Tauri command surfaces.
- The library must remain `async`-friendly (tokio-based) but must not force consumers into a specific async runtime configuration beyond what Cargo feature flags already provide.
- Breaking changes to `EngineEvent` are acceptable in this change since there are currently only two consumers, both owned by the same author.
- Publishing to crates.io / npm is a follow-on step; the primary goal is API correctness first.

Stakeholders: FlowSTT (real-time dictation), OmniRec (screen recording transcript), vtx-demo (developer reference app).

---

## Goals / Non-Goals

**Goals:**
- Define and implement `TranscriptionProfile` (`Dictation` / `Transcription` / `Custom`) as a first-class configuration axis that sets VAD thresholds, segment duration, word-break behaviour, and default model in one call.
- Introduce `ModelManager` as a standalone public type that both consumers can use for model enumeration, download, availability checks, and path resolution — replacing ad-hoc download code in each project.
- Add `AudioEngine::transcribe_audio_stream` accepting a `tokio::sync::mpsc::Receiver<Vec<f32>>` of 16kHz mono audio frames, emitting timestamped `TranscriptionSegment` results — enabling OmniRec's encoder-tee pattern without a live capture session.
- Add `TranscriptionSegment` as a new vtx-common type with `timestamp_offset_ms: u64`, and add `EngineEvent::TranscriptionSegment` variant alongside the existing `TranscriptionComplete`.
- Promote `max_segment_duration_ms` and `word_break_segmentation_enabled` to explicit `EngineConfig` fields (currently hardcoded or absent).
- Replace `EngineConfig::model_path: Option<PathBuf>` with `EngineConfig::model: WhisperModel` (typed enum). Auto-resolution of the physical path remains inside the library.
- Produce `USAGE.md` (integration guide) and `docs/flowstt-migration.md` + `docs/omnirec-integration.md` (consumer change outlines).

**Non-Goals:**
- Publishing to crates.io / npm is explicitly out of scope for this change.
- Changes to FlowSTT's IPC server, CLI, auto-paste, or clipboard logic — those are FlowSTT-owned concerns.
- Changes to OmniRec's video capture, FFmpeg encoding, or region-selection pipeline.
- Adding a new audio backend or platform.
- Real-time streaming transcription (token-by-token output); the unit of output remains a complete segment.
- Multi-language model support or language auto-detection configuration.

---

## Decisions

### Decision 1: TranscriptionProfile as an EngineBuilder preset, not a runtime switch

**Choice:** `TranscriptionProfile` is applied at `EngineBuilder` time and seeds `EngineConfig` with preset values. It is not a runtime-switchable mode.

**Rationale:** VAD thresholds, segment duration, and word-break behaviour are baked into the audio loop thread at `start_capture()` time. Changing them mid-capture would require stopping and restarting the audio loop, which is a disruptive operation. Making the profile a build-time concern keeps the running engine's state model simple.

**Alternatives considered:**
- Runtime profile switch: rejected — would require draining the segment ring buffer, reinitialising `SpeechDetector` state, and potentially losing in-flight audio. Complexity cost exceeds benefit.
- Separate `DictationEngine` and `TranscriptionEngine` types: rejected — doubles the API surface and makes it harder to support the `Custom` case or future profiles.

**`Dictation` preset** (matches current FlowSTT defaults):
- `vad_voiced_threshold_db: -42.0`
- `vad_whisper_threshold_db: -52.0`
- `vad_voiced_onset_ms: 80`
- `vad_whisper_onset_ms: 120`
- `segment_max_duration_ms: 4_000`
- `segment_word_break_grace_ms: 750`
- `word_break_segmentation_enabled: true`
- `model: WhisperModel::BaseEn`

**`Transcription` preset** (matches current OmniRec tuning):
- Same VAD thresholds and onset values as `Dictation`
- `segment_max_duration_ms: 15_000`
- `segment_word_break_grace_ms: 0` (unused; word-break splitting disabled)
- `word_break_segmentation_enabled: false`
- `model: WhisperModel::MediumEn`

The `Custom` variant carries no preset values and leaves `EngineConfig` fields at their `Default` values; consumers supply all values explicitly via builder setters.

---

### Decision 2: WhisperModel enum in vtx-common, model_path deprecated (not removed)

**Choice:** Add `WhisperModel` enum to `vtx-common` covering all 9 current whisper.cpp GGML model variants (TinyEn, Tiny, BaseEn, Base, SmallEn, Small, MediumEn, Medium, LargeV3). `EngineConfig` gains `model: WhisperModel` with default `WhisperModel::BaseEn`. `model_path` is retained as `#[deprecated]` and, if set, takes precedence over `model` so existing serialized configs continue to work.

**Rationale:** Both consumers already track a `WhisperModel` concept (FlowSTT has `ggml-base.en` hardcoded; OmniRec has its own `WhisperModel` enum with 9 variants). Centralising it in vtx-common avoids each consumer maintaining its own enum. Keeping `model_path` as a deprecated override preserves backward compatibility with persisted config files.

**Alternatives considered:**
- Remove `model_path` entirely: rejected — would break any existing saved `vtx-engine.toml` that contains `model_path`.
- Keep `model_path` as primary, add `model` as an alias: rejected — `model_path` is opaque (a filesystem path) and provides no type safety. The new `model` field is the canonical API.

**Path resolution rule** (inside `ModelManager::path(model)`):
`{cache_dir}/{app_name}/whisper/ggml-{model_slug}.bin`
where `model_slug` maps enum variants to the canonical whisper.cpp filenames (e.g. `WhisperModel::MediumEn` → `"medium.en"`).

---

### Decision 3: transcribe_audio_stream takes mpsc::Receiver, not an async Stream

**Choice:** `AudioEngine::transcribe_audio_stream(rx: mpsc::Receiver<Vec<f32>>, session_start: Instant) -> tokio::task::JoinHandle<Vec<TranscriptionSegment>>` — the method spawns a background task that drains the receiver and accumulates segments, returning them all when the channel closes (i.e. when the sender is dropped). Intermediate results are published on the engine's broadcast channel as `EngineEvent::TranscriptionSegment` events in real time.

**Rationale:** OmniRec's pattern is: start recording → encoder tees audio into a sender → recording stops → sender is dropped → transcription worker processes remaining queue → results written to file. The caller wants both real-time progress (for the live transcript window) and a final complete list (for writing the `.md` file). A `JoinHandle<Vec<TranscriptionSegment>>` satisfies the final-list need; the broadcast events satisfy the real-time need. Using `mpsc::Receiver` (not `futures::Stream`) avoids pulling in the `futures` crate as a public dependency.

**Alternatives considered:**
- Returning `impl Stream<Item = TranscriptionSegment>`: rejected — requires `futures` or `tokio-stream` in the public API; complicates cancellation.
- Polling-based approach (keeping OmniRec's current `get_transcription_segments` command): rejected — this is the status quo being replaced; polling adds latency and is wasteful.

**Input format contract:** Caller is responsible for supplying 16 kHz mono `f32` PCM. The engine will not resample or channel-convert inside `transcribe_audio_stream`. This matches the contract at the `Transcriber::transcribe()` boundary and avoids re-encoding an already-encoded stream. `ModelManager` documentation and `USAGE.md` will call this out explicitly.

---

### Decision 4: TranscriptionSegment is a new vtx-common type alongside TranscriptionResult

**Choice:** Add `TranscriptionSegment { id: String, text: String, timestamp_offset_ms: u64, duration_ms: u64, audio_path: Option<String> }` to `vtx-common`. The existing `TranscriptionResult` gains `timestamp_offset_ms: Option<u64>` (None for real-time dictation). A new `EngineEvent::TranscriptionSegment(TranscriptionSegment)` variant is added. `TranscriptionComplete` is **not** removed — it continues to be emitted for real-time dictation.

**Rationale:** The two event types serve fundamentally different consumers. `TranscriptionComplete` signals dictation mode: text is ready to paste. `TranscriptionSegment` signals transcription mode: a timestamped chunk is available to append to a live transcript view. Conflating them into one type would force every consumer to handle optional timestamp fields and switch on a mode flag inside their event handler — worse ergonomics than two distinct variants.

**Alternatives considered:**
- Replace `TranscriptionComplete` with `TranscriptionSegment` throughout: rejected — `TranscriptionComplete` is already part of the stable Tauri event surface in FlowSTT; a rename there requires frontend changes.
- Add `timestamp_offset_ms` only to `TranscriptionResult` and keep one variant: partially adopted — `TranscriptionResult` does gain `timestamp_offset_ms: Option<u64>`, but the dedicated `TranscriptionSegment` variant is still added for stream transcription so consumers can `match` cleanly on which mode they're in.

---

### Decision 5: ModelManager is a standalone public struct, not methods on AudioEngine

**Choice:** `ModelManager` is a `pub struct` in `vtx-engine` (or `vtx-common`) with constructor `ModelManager::new(app_name: &str) -> ModelManager` and methods: `is_available(model: WhisperModel) -> bool`, `path(model: WhisperModel) -> PathBuf`, `list_cached() -> Vec<WhisperModel>`, `async download(model: WhisperModel, on_progress: impl Fn(u8) + Send) -> Result<(), ModelError>`. It does not require an `AudioEngine` instance.

**Rationale:** Both FlowSTT and OmniRec manage model downloads from their settings UIs, before or independent of a running engine. Tying `ModelManager` to `AudioEngine` would force constructing an engine just to check if a model file exists. Standalone also means it can be used during first-run setup wizards before capture has ever been configured.

**Alternatives considered:**
- Static / free functions (`vtx_engine::download_model(...)`): workable but provides no encapsulation of the `app_name` context; callers would repeat it on every call.
- Methods on `AudioEngine`: rejected — see rationale above; also `AudioEngine` already has `check_model_status` and `download_model` methods that will be replaced by delegating to `ModelManager`.

---

### Decision 6: word_break_segmentation_enabled is an EngineConfig field, not a profile detail only

**Choice:** `EngineConfig` gains `word_break_segmentation_enabled: bool` (default `true`). When `false`, the audio loop's `TranscribeState` still detects word breaks internally but does not act on them — segment boundaries are determined solely by speech-end detection and `segment_max_duration_ms`. Profiles set this field as part of their presets.

**Rationale:** FlowSTT relies on word-break splitting to produce short segments (currently the only way segments stay under ~4 seconds). OmniRec explicitly discards word breaks (`let _ = self.voice_detector.take_word_break_event()`). Making this a first-class config field eliminates the silent behavioural difference between the two projects' copies of the VAD code and makes the behaviour testable.

---

## Risks / Trade-offs

- **`EngineEvent` exhaustive match breakage** → Mitigation: The new `TranscriptionSegment` variant is additive. Consumers using `match event { ... }` with a wildcard arm (`_ => {}`) are unaffected. Consumers with exhaustive matches (vtx-demo's `EventHandlerAdapter` closure in `lib.rs`) need a one-line addition. The vtx-demo app is in-repo and will be updated as part of this change.

- **`TranscriptionResult::timestamp_offset_ms` field is additive but changes struct layout** → Mitigation: The field is `#[serde(default)]` so any serialized `TranscriptionResult` (in history files) loaded by the new library will deserialize cleanly with `None`.

- **`model_path` deprecation may confuse consumers for a release cycle** → Mitigation: Add a clear `#[deprecated(since = "0.2.0", note = "Use EngineConfig::model instead")]` attribute and document the migration in `USAGE.md`.

- **`transcribe_audio_stream` input contract (16kHz mono pre-processed)** requires OmniRec to resample before sending → OmniRec already resamples (3:1 integer decimation from 48kHz to 16kHz). The contract matches what OmniRec already does. Document it in the method signature doc comment.

- **ModelManager::download is async and requires a tokio runtime** → Both consumers already use a tokio multi-thread runtime (Tauri's). No new runtime requirement is introduced.

- **No cancellation token on `transcribe_audio_stream`** → If OmniRec cancels a recording mid-way, it simply drops the sender; the background task exits naturally when the receiver returns `None`. No explicit cancellation API is needed for the initial implementation.

---

## Migration Plan

This is a library-only change — no deployment in the traditional sense. The migration is:

1. **vtx-engine changes** (this change): implement all new types, methods, config fields, and docs.
2. **vtx-demo update** (in-repo, part of this change): update exhaustive `EngineEvent` match arms; add `TranscriptionSegment` handler; exercise `ModelManager` in place of current `download_model` Tauri command.
3. **FlowSTT migration** (separate change in FlowSTT repo, outlined in `docs/flowstt-migration.md`): remove `src-engine` and `src-common`, add `vtx-engine` dependency, remap types.
4. **OmniRec integration** (separate change in OmniRec repo, outlined in `docs/omnirec-integration.md`): remove `src-tauri/src/transcription`, add `vtx-engine` dependency, wire encoder audio tee.

Rollback: both consumer projects keep their own engine implementations until their migration change is merged. vtx-engine's API additions are backward-compatible except for the `EngineEvent` new variant and `transcribe_file` rename (handled by updating vtx-demo in-repo).

---

## Open Questions

- **vtx-common vs vtx-engine placement for WhisperModel and TranscriptionSegment:** Currently `vtx-common` holds all shared types. `WhisperModel` and `TranscriptionSegment` logically belong there. `ModelManager` involves file I/O and async and should stay in `vtx-engine`. Confirm this split before implementation.

- **ModelError type:** `ModelManager::download` needs a typed error. Should this be a new `pub enum ModelError` in `vtx-engine`, or reuse/extend `ConfigError`? Leaning toward a new `ModelError` (variants: `Io`, `Network`, `Checksum`, `AlreadyDownloading`) since the concern is different from config persistence.

- **`transcribe_audio_stream` session_start parameter:** Should the method accept an `Instant` (to compute offsets) or should the caller be responsible for computing `timestamp_offset_ms` before sending frames? The current proposal has the engine own the offset calculation, which is cleaner but requires the caller to pass the session start time. Confirm.

- **FlowSTT history format:** `TranscriptionHistory` currently stores `HistoryEntry { id, text, timestamp, wav_path }`. The new `TranscriptionResult` gains `timestamp_offset_ms`. Does the history format need updating, or should `HistoryEntry` remain as-is and the `timestamp_offset_ms` field be ignored for dictation history? Lean toward keeping `HistoryEntry` unchanged for this change.
