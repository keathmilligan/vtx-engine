## Requirements

### Requirement: Engine exposes a broadcast event channel
The engine SHALL use `tokio::sync::broadcast::Sender<EngineEvent>` as its internal event bus. `AudioEngine` SHALL expose a `subscribe()` method returning a `broadcast::Receiver<EngineEvent>` that any number of callers may hold simultaneously.

#### Scenario: Multiple independent subscribers receive the same event
- **WHEN** two callers each call `engine.subscribe()` and then a `TranscriptionComplete` event is emitted
- **THEN** both receivers receive the event independently without either blocking the other

#### Scenario: Subscribing after engine construction
- **WHEN** `engine.subscribe()` is called after `start_capture()` has already been called
- **THEN** the returned receiver receives all events emitted from that point forward

### Requirement: Lagged receivers do not block the engine
The broadcast channel SHALL have a capacity of at least 256 events. When a slow receiver falls behind the channel capacity, the engine SHALL continue emitting events and the slow receiver SHALL receive a `RecvError::Lagged(n)` indicating how many events were dropped.

#### Scenario: Slow receiver lags behind
- **WHEN** a receiver does not drain its buffer and the channel fills to capacity
- **THEN** subsequent `recv()` calls on that receiver return `Err(RecvError::Lagged(n))` where `n` is the number of dropped events
- **THEN** the engine and other non-lagged receivers are unaffected

### Requirement: EventHandlerAdapter bridges broadcast to callback
The library SHALL provide `EventHandlerAdapter`, a helper that wraps a `broadcast::Receiver` and calls a user-supplied `FnMut(EngineEvent) + Send + 'static` closure on each event in a spawned `tokio` task. The adapter SHALL log and skip `Lagged` errors rather than panicking.

#### Scenario: Adapter forwards events to a closure
- **WHEN** an `EventHandlerAdapter` is constructed with a receiver and a closure and `spawn()` is called
- **THEN** each event emitted by the engine is delivered to the closure in arrival order

#### Scenario: Adapter handles lag gracefully
- **WHEN** the adapter's receiver receives `RecvError::Lagged(n)`
- **THEN** the adapter logs a warning with `n` and continues processing subsequent events without panicking

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

### Requirement: EngineEvent includes a TranscriptionSegment variant
`EngineEvent` SHALL include a `TranscriptionSegment(TranscriptionSegment)` variant. This variant SHALL be emitted by `transcribe_audio_stream` and `transcribe_audio_file` sessions. It SHALL NOT be emitted during live-capture dictation sessions (those continue to use `TranscriptionComplete`).

#### Scenario: TranscriptionSegment variant is reachable in a match expression
- **WHEN** a consumer writes `match event { EngineEvent::TranscriptionSegment(seg) => { ... }, _ => {} }`
- **THEN** the match arm compiles and receives segment events during stream transcription

#### Scenario: Exhaustive match requires handling the new variant
- **WHEN** a consumer has an exhaustive match on `EngineEvent` without a wildcard
- **THEN** the compiler reports a missing `TranscriptionSegment` arm, prompting the consumer to handle it

### Requirement: EngineEvent includes an AgcGainChanged variant
`EngineEvent` SHALL include an `AgcGainChanged(f32)` variant. The `f32` value carries the current AGC gain in decibels at the time of emission. This event SHALL be emitted by the capture loop at most once per 100 milliseconds when AGC is enabled and active.

#### Scenario: AgcGainChanged is emitted during active AGC
- **WHEN** AGC is enabled and audio capture is running
- **THEN** `AgcGainChanged(gain_db)` events are emitted on the broadcast channel at most every 100 ms

#### Scenario: AgcGainChanged is not emitted when AGC is disabled
- **WHEN** `AgcConfig::enabled` is `false`
- **THEN** no `AgcGainChanged` events are emitted on the broadcast channel

#### Scenario: AgcGainChanged variant is reachable in a match expression
- **WHEN** a consumer writes `match event { EngineEvent::AgcGainChanged(db) => { ... }, _ => {} }`
- **THEN** the match arm compiles and receives events during active AGC capture

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
