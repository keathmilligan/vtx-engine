## Requirements

### Requirement: EngineBuilder is the primary construction path
The library SHALL expose an `EngineBuilder` struct. `EngineBuilder::new()` SHALL return a builder with all subsystems enabled and default configuration values. Calling `builder.build().await` SHALL produce an `AudioEngine` and its associated `broadcast::Receiver<EngineEvent>` as a tuple `(AudioEngine, broadcast::Receiver<EngineEvent>)`.

#### Scenario: Build with defaults
- **WHEN** `EngineBuilder::new().build().await` is called
- **THEN** an `AudioEngine` is returned with transcription, visualization, and VAD all enabled
- **THEN** a `broadcast::Receiver` is returned that receives all engine events

#### Scenario: Build returns receiver alongside engine
- **WHEN** `let (engine, rx) = EngineBuilder::new().build().await?` is called
- **THEN** `rx` immediately receives any `StatusChanged` or `GpuStatusChanged` events emitted during initialization

### Requirement: Builder exposes full engine configuration surface
`EngineBuilder` SHALL expose setter methods for all tunable `EngineConfig` fields, including but not limited to: `model_path`, `recording_mode`, `aec_enabled`, `vad_voiced_threshold_db`, `vad_whisper_threshold_db`, `vad_onset_ms`, `segment_max_duration_ms`, `segment_word_break_grace_ms`, `segment_lookback_ms`, `transcription_queue_capacity`, and `viz_frame_interval_ms`. Each setter SHALL return `Self` for method chaining.

#### Scenario: Method chaining configures the engine
- **WHEN** `EngineBuilder::new().model_path(path).aec_enabled(true).segment_max_duration_ms(20_000).build().await` is called
- **THEN** the resulting engine uses the specified model path, has AEC enabled, and uses 20-second max segment duration

#### Scenario: Default values are documented
- **WHEN** `EngineBuilder::new()` is called without any setters
- **THEN** the builder uses documented defaults matching the previous hardcoded values (voiced threshold -42 dB, whisper threshold -52 dB, onset 80 ms / 120 ms, max segment 30 s, word-break grace 750 ms, lookback 200 ms, queue capacity 8, viz interval 16 ms)

### Requirement: Builder supports optional subsystem disabling
`EngineBuilder` SHALL expose `without_transcription()`, `without_visualization()`, and `without_vad()` methods. When a subsystem is disabled, the engine SHALL not initialize it and SHALL not emit the events associated with that subsystem.

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

### Requirement: AudioEngine::new() remains available as a convenience shortcut
`AudioEngine::new(config: EngineConfig)` SHALL remain in the public API as a backward-compatible constructor. It SHALL be equivalent to `EngineBuilder::from_config(config).build().await` and SHALL return `(AudioEngine, broadcast::Receiver<EngineEvent>)`.

#### Scenario: AudioEngine::new() produces a working engine
- **WHEN** `let (engine, rx) = AudioEngine::new(EngineConfig::default()).await?` is called
- **THEN** the engine behaves identically to one built with `EngineBuilder::new().build().await`
