## MODIFIED Requirements

### Requirement: All internal engine threads emit events through the broadcast sender
Every thread that previously called `event_handler.on_event()` SHALL instead call `sender.send()`. The `EventHandler` trait SHALL be removed from the public API. The audio loop, transcription worker, model download utility, and stream transcription task SHALL all hold a clone of `Arc<broadcast::Sender<EngineEvent>>`.

#### Scenario: Audio loop emits visualization data
- **WHEN** the audio loop processes a batch of samples and produces waveform data
- **THEN** a `VisualizationData` event is sent on the broadcast sender and received by all active subscribers

#### Scenario: Transcription worker emits results
- **WHEN** the whisper.cpp worker completes inference on a live-capture segment
- **THEN** a `TranscriptionComplete` event is sent on the broadcast sender and received by all active subscribers

#### Scenario: Stream transcription task emits segment events
- **WHEN** the `transcribe_audio_stream` background task completes inference on a segment
- **THEN** a `TranscriptionSegment` event is sent on the broadcast sender and received by all active subscribers

## ADDED Requirements

### Requirement: EngineEvent includes a TranscriptionSegment variant
`EngineEvent` SHALL include a `TranscriptionSegment(TranscriptionSegment)` variant. This variant SHALL be emitted by `transcribe_audio_stream` and `transcribe_audio_file` sessions. It SHALL NOT be emitted during live-capture dictation sessions (those continue to use `TranscriptionComplete`).

#### Scenario: TranscriptionSegment variant is reachable in a match expression
- **WHEN** a consumer writes `match event { EngineEvent::TranscriptionSegment(seg) => { ... }, _ => {} }`
- **THEN** the match arm compiles and receives segment events during stream transcription

#### Scenario: Exhaustive match requires handling the new variant
- **WHEN** a consumer has an exhaustive match on `EngineEvent` without a wildcard
- **THEN** the compiler reports a missing `TranscriptionSegment` arm, prompting the consumer to handle it
