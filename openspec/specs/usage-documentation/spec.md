## Requirements

### Requirement: USAGE.md documents the real-time dictation integration pattern
The repository SHALL contain a `USAGE.md` file at the crate workspace root. The file SHALL include a complete, working Rust code example demonstrating the `Dictation` profile integration pattern: constructing an `AudioEngine` with `TranscriptionProfile::Dictation`, subscribing to the broadcast channel, starting capture, handling `EngineEvent::TranscriptionComplete`, and shutting down. The example SHALL compile against the public API without requiring private imports.

#### Scenario: Dictation example builds without errors
- **WHEN** the code example in the `## Real-Time Dictation` section of `USAGE.md` is compiled as a standalone Rust binary with `vtx-engine` as a dependency
- **THEN** it compiles without errors

### Requirement: USAGE.md documents the stream transcription integration pattern
`USAGE.md` SHALL include a complete, working Rust code example for the stream transcription (OmniRec-style) pattern: constructing an `AudioEngine` with `TranscriptionProfile::Transcription`, creating an `mpsc` channel, calling `transcribe_audio_stream`, feeding 16 kHz mono audio frames from an external source, handling `EngineEvent::TranscriptionSegment` for live updates, and awaiting the `JoinHandle` for the final segment list.

#### Scenario: Stream transcription example builds without errors
- **WHEN** the code example in the `## Stream Transcription` section of `USAGE.md` is compiled as a standalone Rust binary
- **THEN** it compiles without errors

### Requirement: USAGE.md documents ModelManager usage
`USAGE.md` SHALL include a section covering `ModelManager`: how to check model availability, trigger a download with a progress callback, and list cached models. It SHALL call out the input audio contract for `transcribe_audio_stream` (16 kHz mono f32).

#### Scenario: ModelManager section covers all public methods
- **WHEN** the `## Model Management` section of `USAGE.md` is read
- **THEN** it references `ModelManager::is_available`, `ModelManager::download`, `ModelManager::list_cached`, and `ModelManager::path`
