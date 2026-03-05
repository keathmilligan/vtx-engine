## Requirements

### Requirement: AudioEngine transcribes an external audio stream
`AudioEngine` SHALL expose `transcribe_audio_stream(rx: tokio::sync::mpsc::Receiver<Vec<f32>>, session_start: std::time::Instant) -> tokio::task::JoinHandle<Vec<TranscriptionSegment>>`. The method SHALL spawn a background tokio task that reads 16 kHz mono f32 PCM frames from `rx`, applies VAD segmentation (using the engine's configured VAD and segment parameters), runs whisper inference on each completed segment, and emits `EngineEvent::TranscriptionSegment` on the engine's broadcast channel for each result. When `rx` is closed (sender dropped), the task SHALL flush any in-progress segment, complete all queued inference, and resolve its `JoinHandle` with the full ordered `Vec<TranscriptionSegment>`.

#### Scenario: Segments are emitted on the broadcast channel in real time
- **WHEN** `transcribe_audio_stream` is running and inference completes for a segment
- **THEN** an `EngineEvent::TranscriptionSegment` event is sent on the broadcast channel before the `JoinHandle` resolves

#### Scenario: JoinHandle resolves with all segments when sender is dropped
- **WHEN** the mpsc sender is dropped and all queued inference completes
- **THEN** `JoinHandle::await` returns `Vec<TranscriptionSegment>` containing every segment that was emitted during the session, in timestamp order

#### Scenario: Empty stream produces empty result
- **WHEN** the sender is dropped immediately without sending any frames
- **THEN** `JoinHandle::await` returns an empty `Vec<TranscriptionSegment>`

### Requirement: transcribe_audio_stream does not require an active capture session
Calling `transcribe_audio_stream` SHALL NOT require that `start_capture()` has been called. The method SHALL work on a freshly built `AudioEngine` that has never started capture.

#### Scenario: Stream transcription on idle engine succeeds
- **WHEN** `engine.transcribe_audio_stream(rx, start)` is called on an engine where `is_capturing()` returns `false`
- **THEN** the method starts and processes audio from `rx` without error

### Requirement: Input audio contract is 16 kHz mono f32
The caller is responsible for supplying audio that is already resampled to 16 kHz and converted to mono (single-channel) f32 PCM. The engine SHALL NOT resample or channel-convert inside `transcribe_audio_stream`. If frames contain unexpected lengths, the engine SHALL silently accumulate them into the internal ring buffer without error.

#### Scenario: Frames of arbitrary length are accepted
- **WHEN** frames of length 160, 480, and 1024 samples are sent consecutively
- **THEN** the engine processes them all as a contiguous stream without error

### Requirement: timestamp_offset_ms is computed relative to session_start
Each `TranscriptionSegment` emitted during a `transcribe_audio_stream` session SHALL have `timestamp_offset_ms` set to the number of milliseconds elapsed between `session_start` and the beginning of the audio that produced that segment (i.e. the segment's position in the stream).

#### Scenario: First segment offset reflects its position in the stream
- **WHEN** speech begins 5 seconds after the session starts (5000 ms of audio sent before speech onset)
- **THEN** the resulting `TranscriptionSegment::timestamp_offset_ms` is approximately `5_000` (within the VAD onset window margin)

### Requirement: AudioEngine::transcribe_audio_file transcribes a WAV file
`AudioEngine` SHALL expose `async fn transcribe_audio_file(path: impl AsRef<Path>) -> Result<Vec<TranscriptionSegment>, String>`. The method SHALL load the WAV file, resample to 16 kHz mono, run VAD segmentation, and return timestamped segments. This replaces the previous `transcribe_file` method.

#### Scenario: WAV file produces at least one segment with a timestamp
- **WHEN** `engine.transcribe_audio_file("recording.wav").await` is called on a WAV containing speech
- **THEN** the result is `Ok(segments)` where each segment has `timestamp_offset_ms` set relative to the start of the file

#### Scenario: Silent WAV produces empty segment list
- **WHEN** `engine.transcribe_audio_file("silence.wav").await` is called on a WAV with no detectable speech
- **THEN** the result is `Ok(vec![])` (not an error)
