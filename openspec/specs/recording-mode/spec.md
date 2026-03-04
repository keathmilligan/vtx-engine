## Requirements

### Requirement: RecordingMode is part of EngineConfig and EngineBuilder
`EngineConfig` SHALL include a `recording_mode: RecordingMode` field with a default of `RecordingMode::Mixed`. `EngineBuilder` SHALL expose a `recording_mode(RecordingMode)` setter. The field SHALL derive `Serialize` and `Deserialize`.

#### Scenario: Default recording mode is Mixed
- **WHEN** `EngineConfig::default()` or `EngineBuilder::new()` is used without setting recording mode
- **THEN** `config.recording_mode` equals `RecordingMode::Mixed`

#### Scenario: Builder setter overrides recording mode
- **WHEN** `EngineBuilder::new().recording_mode(RecordingMode::EchoCancel).build().await` is called
- **THEN** the engine backend is configured with `RecordingMode::EchoCancel` before capture starts

### Requirement: Mixed mode combines both audio sources into a single stream
When `recording_mode` is `RecordingMode::Mixed` and two capture sources are active, the audio backend SHALL mix both streams before forwarding audio to the processing pipeline. The mixed stream SHALL be passed to the VAD, visualization processor, and transcription ring buffer.

#### Scenario: Two sources are mixed
- **WHEN** `start_capture(Some(mic_id), Some(system_id))` is called with `RecordingMode::Mixed`
- **THEN** the audio loop receives a single interleaved stream combining both sources

### Requirement: EchoCancel mode applies AEC and outputs the cleaned primary source
When `recording_mode` is `RecordingMode::EchoCancel` and two capture sources are active, the audio backend SHALL apply AEC3 echo cancellation using source 2 as the reference signal and SHALL output only the echo-cancelled source 1 to the processing pipeline.

#### Scenario: EchoCancel reduces echo from system audio in microphone
- **WHEN** `start_capture(Some(mic_id), Some(system_id))` is called with `RecordingMode::EchoCancel`
- **THEN** the audio pipeline receives only the echo-cancelled microphone stream
- **THEN** system audio is not present in the transcription input

#### Scenario: EchoCancel with only one source behaves as single-source capture
- **WHEN** `start_capture(Some(mic_id), None)` is called with `RecordingMode::EchoCancel`
- **THEN** the engine captures normally without AEC (no reference signal available)
- **THEN** no error is returned; capture proceeds as single-source

### Requirement: aec_enabled flag is superseded by recording_mode
The existing `EngineConfig::aec_enabled: bool` field SHALL be removed. Its function is fully covered by `RecordingMode::EchoCancel`. Code that previously set `aec_enabled = true` SHALL be migrated to use `RecordingMode::EchoCancel`.

#### Scenario: No separate aec_enabled field in public API
- **WHEN** a consumer constructs `EngineConfig` or uses `EngineBuilder`
- **THEN** there is no `aec_enabled` field; AEC is activated by setting `RecordingMode::EchoCancel`
