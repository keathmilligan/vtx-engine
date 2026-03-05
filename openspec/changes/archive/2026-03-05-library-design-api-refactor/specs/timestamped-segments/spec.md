## ADDED Requirements

### Requirement: TranscriptionSegment is a distinct public type with timestamp
The library SHALL expose a `TranscriptionSegment` struct in `vtx-common` with fields: `id: String`, `text: String`, `timestamp_offset_ms: u64`, `duration_ms: u64`, and `audio_path: Option<String>`. It SHALL derive `serde::Serialize`, `serde::Deserialize`, `Clone`, and `Debug`.

#### Scenario: TranscriptionSegment serializes to JSON with all fields
- **WHEN** a `TranscriptionSegment` is serialized to JSON
- **THEN** the JSON object contains the keys `id`, `text`, `timestamp_offset_ms`, `duration_ms`, and `audio_path`

### Requirement: EngineEvent gains a TranscriptionSegment variant
`EngineEvent` SHALL gain a `TranscriptionSegment(TranscriptionSegment)` variant. This variant SHALL be emitted during `transcribe_audio_stream` and `transcribe_audio_file` sessions. The existing `TranscriptionComplete(TranscriptionResult)` variant SHALL continue to be emitted for real-time live-capture dictation sessions and SHALL NOT be emitted during stream/file transcription.

#### Scenario: Stream transcription emits TranscriptionSegment, not TranscriptionComplete
- **WHEN** `transcribe_audio_stream` completes inference on a segment
- **THEN** an `EngineEvent::TranscriptionSegment` event is broadcast
- **THEN** no `EngineEvent::TranscriptionComplete` event is broadcast for the same text

#### Scenario: Live capture dictation emits TranscriptionComplete, not TranscriptionSegment
- **WHEN** a VAD-driven segment is transcribed during an active `start_capture()` session
- **THEN** an `EngineEvent::TranscriptionComplete` event is broadcast
- **THEN** no `EngineEvent::TranscriptionSegment` event is broadcast for the same text

### Requirement: TranscriptionResult gains optional timestamp_offset_ms
`TranscriptionResult` in `vtx-common` SHALL gain a field `timestamp_offset_ms: Option<u64>`. The field SHALL be `#[serde(default)]` so existing serialized results (in history files) deserialize to `None` without error. For live-capture dictation sessions the field SHALL be `None`. For file-based transcription via `transcribe_audio_file` the field SHALL be `Some(ms)`.

#### Scenario: Existing history file entry deserializes with None timestamp
- **WHEN** a `TranscriptionResult` JSON object without the `timestamp_offset_ms` key is deserialized
- **THEN** the resulting struct has `timestamp_offset_ms = None`

#### Scenario: File transcription result carries timestamp
- **WHEN** `transcribe_audio_file` returns a segment at 3500 ms into the file
- **THEN** the corresponding `TranscriptionResult::timestamp_offset_ms` is `Some(3_500)`
