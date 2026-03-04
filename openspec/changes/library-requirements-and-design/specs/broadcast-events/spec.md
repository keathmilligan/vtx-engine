## ADDED Requirements

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
Every thread that previously called `event_handler.on_event()` SHALL instead call `sender.send()`. The `EventHandler` trait SHALL be removed from the public API. The audio loop, transcription worker, and model download utility SHALL all hold a clone of `Arc<broadcast::Sender<EngineEvent>>`.

#### Scenario: Audio loop emits visualization data
- **WHEN** the audio loop processes a batch of samples and produces waveform data
- **THEN** a `VisualizationData` event is sent on the broadcast sender and received by all active subscribers

#### Scenario: Transcription worker emits results
- **WHEN** the whisper.cpp worker completes inference on a segment
- **THEN** a `TranscriptionComplete` event is sent on the broadcast sender and received by all active subscribers
