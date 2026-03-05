## ADDED Requirements

### Requirement: FlowSTT migration outline is documented
The repository SHALL contain `docs/flowstt-migration.md`. The document SHALL outline, at a high level, the changes required in the FlowSTT project to replace its internal engine with `vtx-engine`. It SHALL cover: removing `src-engine` and `src-common` crates, adding `vtx-engine` as a Cargo dependency, mapping FlowSTT-internal types to their `vtx-common` equivalents, migrating the config file format, and preserving the IPC server and CLI as FlowSTT-owned components. It SHALL NOT contain line-by-line implementation instructions.

#### Scenario: Migration outline covers the four major change areas
- **WHEN** `docs/flowstt-migration.md` is read
- **THEN** it addresses each of: dependency replacement, type mapping, config migration, and IPC/CLI ownership boundary

### Requirement: OmniRec integration outline is documented
The repository SHALL contain `docs/omnirec-integration.md`. The document SHALL outline the changes required in the OmniRec project to adopt `vtx-engine` for transcription. It SHALL cover: removing `src-tauri/src/transcription`, adding `vtx-engine` as a Cargo dependency, wiring the encoder's audio tee into `AudioEngine::transcribe_audio_stream`, replacing OmniRec's model download Tauri commands with `ModelManager`, mapping OmniRec's `WhisperModel` enum to `vtx-common::WhisperModel`, replacing the polling-based `get_transcription_segments` command with `EngineEvent::TranscriptionSegment` broadcast events, and consolidating CUDA DLL distribution. It SHALL NOT contain line-by-line implementation instructions.

#### Scenario: Integration outline covers the six major change areas
- **WHEN** `docs/omnirec-integration.md` is read
- **THEN** it addresses each of: transcription module removal, dependency addition, audio stream wiring, model management, event-driven transcript updates, and CUDA binary consolidation

### Requirement: Both outlines identify the boundary of vtx-engine responsibility
Each consumer outline document SHALL explicitly state which concerns remain owned by the consumer app (Tauri command layer, UI, IPC, video encoding, clipboard, auto-paste, etc.) and which are delegated to vtx-engine (whisper FFI, VAD, segmentation, model download, history). This boundary statement SHALL appear in a dedicated section in each document.

#### Scenario: Boundary section is present in both documents
- **WHEN** either `docs/flowstt-migration.md` or `docs/omnirec-integration.md` is read
- **THEN** there is a section explicitly titled or headed to describe what vtx-engine owns vs what the consumer owns
