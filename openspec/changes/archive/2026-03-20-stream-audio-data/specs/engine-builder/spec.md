## MODIFIED Requirements

### Requirement: Builder supports optional subsystem disabling
`EngineBuilder` SHALL expose `without_transcription()`, `without_visualization()`, and `without_vad()` methods. When a subsystem is disabled, the engine SHALL not initialize it and SHALL not emit the events associated with that subsystem. `EngineBuilder` SHALL additionally expose `with_audio_streaming()` which enables emission of `EngineEvent::AudioData` events during live capture, and `with_raw_audio_streaming()` which enables emission of `EngineEvent::RawAudioData` events during live capture. Both audio streaming options SHALL be disabled by default and SHALL be independently controllable.

#### Scenario: Engine without transcription
- **WHEN** `EngineBuilder::new().without_transcription().build().await` is called and capture is started
- **THEN** no `TranscriptionComplete` events are emitted
- **THEN** no whisper.cpp FFI library is loaded

#### Scenario: Engine without visualization
- **WHEN** `EngineBuilder::new().without_visualization().build().await` is called and capture is started
- **THEN** no `VisualizationData` events are emitted

#### Scenario: Engine without VAD still captures audio
- **WHEN** `EngineBuilder::new().without_vad().build().await` is called and capture is started
- **THEN** no `SpeechStarted` or `SpeechEnded` events are emitted
- **THEN** audio capture proceeds normally and `AudioLevelUpdate` events are still emitted during test capture

#### Scenario: Engine with processed audio streaming enabled
- **WHEN** `EngineBuilder::new().with_audio_streaming().build().await` is called and capture is started
- **THEN** `EngineEvent::AudioData` events are emitted for every audio chunk during capture

#### Scenario: Engine with raw audio streaming enabled
- **WHEN** `EngineBuilder::new().with_raw_audio_streaming().build().await` is called and capture is started
- **THEN** `EngineEvent::RawAudioData` events are emitted for every audio chunk during capture

#### Scenario: Engine with both audio streams enabled
- **WHEN** `EngineBuilder::new().with_audio_streaming().with_raw_audio_streaming().build().await` is called and capture is started
- **THEN** both `EngineEvent::AudioData` and `EngineEvent::RawAudioData` events are emitted for every audio chunk during capture

#### Scenario: Both audio streaming options are disabled by default
- **WHEN** `EngineBuilder::new().build().await` is called and capture is started
- **THEN** no `EngineEvent::AudioData` or `EngineEvent::RawAudioData` events are emitted

#### Scenario: Audio streaming combines with other subsystem toggles
- **WHEN** `EngineBuilder::new().with_audio_streaming().with_raw_audio_streaming().without_visualization().without_transcription().build().await` is called and capture is started
- **THEN** both `AudioData` and `RawAudioData` events are emitted
- **THEN** no `VisualizationData` or `TranscriptionComplete` events are emitted
