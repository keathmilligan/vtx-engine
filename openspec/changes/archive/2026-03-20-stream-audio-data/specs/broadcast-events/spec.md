## MODIFIED Requirements

### Requirement: Engine exposes a broadcast event channel
The engine SHALL use `tokio::sync::broadcast::Sender<EngineEvent>` as its internal event bus. `AudioEngine` SHALL expose a `subscribe()` method returning a `broadcast::Receiver<EngineEvent>` that any number of callers may hold simultaneously.

#### Scenario: Multiple independent subscribers receive the same event
- **WHEN** two callers each call `engine.subscribe()` and then a `TranscriptionComplete` event is emitted
- **THEN** both receivers receive the event independently without either blocking the other

#### Scenario: Subscribing after engine construction
- **WHEN** `engine.subscribe()` is called after `start_capture()` has already been called
- **THEN** the returned receiver receives all events emitted from that point forward

### Requirement: EngineEvent includes an AudioData variant
`EngineEvent` SHALL include an `AudioData` variant carrying a struct with three fields: `samples: Vec<f32>` (processed mono audio samples after gain and AGC), `sample_rate: u32` (sample rate in Hz), and `sample_offset: u64` (cumulative sample count since capture session start). This variant SHALL only be emitted when processed audio streaming is enabled via the builder.

#### Scenario: AudioData variant is reachable in a match expression
- **WHEN** a consumer writes `match event { EngineEvent::AudioData(data) => { ... }, _ => {} }`
- **THEN** the match arm compiles and receives processed audio data events when audio streaming is enabled

#### Scenario: Exhaustive match requires handling the AudioData variant
- **WHEN** a consumer has an exhaustive match on `EngineEvent` without a wildcard
- **THEN** the compiler reports a missing `AudioData` arm, prompting the consumer to handle it

#### Scenario: AudioData is not emitted when processed audio streaming is disabled
- **WHEN** the engine is built without calling `with_audio_streaming()` and capture is active
- **THEN** no `AudioData` events appear on the broadcast channel

### Requirement: EngineEvent includes a RawAudioData variant
`EngineEvent` SHALL include a `RawAudioData` variant carrying a struct with three fields: `samples: Vec<f32>` (raw mono audio samples before gain and AGC), `sample_rate: u32` (sample rate in Hz), and `sample_offset: u64` (cumulative sample count since capture session start). This variant SHALL only be emitted when raw audio streaming is enabled via the builder.

#### Scenario: RawAudioData variant is reachable in a match expression
- **WHEN** a consumer writes `match event { EngineEvent::RawAudioData(data) => { ... }, _ => {} }`
- **THEN** the match arm compiles and receives raw audio data events when raw audio streaming is enabled

#### Scenario: Exhaustive match requires handling the RawAudioData variant
- **WHEN** a consumer has an exhaustive match on `EngineEvent` without a wildcard
- **THEN** the compiler reports a missing `RawAudioData` arm, prompting the consumer to handle it

#### Scenario: RawAudioData is not emitted when raw audio streaming is disabled
- **WHEN** the engine is built without calling `with_raw_audio_streaming()` and capture is active
- **THEN** no `RawAudioData` events appear on the broadcast channel
