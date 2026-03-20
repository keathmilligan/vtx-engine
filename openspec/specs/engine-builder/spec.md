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
`EngineBuilder` SHALL expose setter methods for all tunable `EngineConfig` fields, including but not limited to: `model` (type `WhisperModel`), `recording_mode`, `aec_enabled`, `vad_voiced_threshold_db`, `vad_whisper_threshold_db`, `vad_voiced_onset_ms`, `vad_whisper_onset_ms`, `segment_max_duration_ms`, `segment_word_break_grace_ms`, `segment_lookback_ms`, `word_break_segmentation_enabled`, `transcription_queue_capacity`, and `viz_frame_interval_ms`. Each setter SHALL return `Self` for method chaining. The deprecated `model_path` setter SHALL remain available for backward compatibility but SHALL be superseded by `model`. `EngineBuilder` SHALL also expose `with_profile(profile: TranscriptionProfile)` which applies the profile's preset values to the builder state.

#### Scenario: Method chaining configures the engine
- **WHEN** `EngineBuilder::new().model(WhisperModel::SmallEn).segment_max_duration_ms(20_000).word_break_segmentation_enabled(false).build().await` is called
- **THEN** the resulting engine uses the SmallEn model, 20-second max segments, and does not split on word breaks

#### Scenario: Default values are documented
- **WHEN** `EngineBuilder::new()` is called without any setters
- **THEN** the builder uses documented defaults: voiced threshold -42 dB, whisper threshold -52 dB, voiced onset 80 ms, whisper onset 120 ms, max segment 4000 ms, word-break grace 750 ms, lookback 200 ms, queue capacity 8, viz interval 16 ms, `word_break_segmentation_enabled = true`, `model = WhisperModel::BaseEn`

#### Scenario: with_profile applies preset values
- **WHEN** `EngineBuilder::new().with_profile(TranscriptionProfile::Transcription).build().await` is called
- **THEN** the resulting engine uses `segment_max_duration_ms = 15_000`, `word_break_segmentation_enabled = false`, and `model = WhisperModel::MediumEn`

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

### Requirement: AudioEngine::new() remains available as a convenience shortcut
`AudioEngine::new(config: EngineConfig)` SHALL remain in the public API as a backward-compatible constructor. It SHALL be equivalent to `EngineBuilder::from_config(config).build().await` and SHALL return `(AudioEngine, broadcast::Receiver<EngineEvent>)`.

#### Scenario: AudioEngine::new() produces a working engine
- **WHEN** `let (engine, rx) = AudioEngine::new(EngineConfig::default()).await?` is called
- **THEN** the engine behaves identically to one built with `EngineBuilder::new().build().await`

### Requirement: EngineConfig exposes word_break_segmentation_enabled
`EngineConfig` SHALL contain a `word_break_segmentation_enabled: bool` field with default value `true`. When `false`, the audio loop SHALL detect word-break events internally but SHALL NOT use them to split segments; segment boundaries SHALL be determined solely by speech-end detection and `segment_max_duration_ms`.

#### Scenario: Disabled word-break segmentation does not split long utterances at pauses
- **WHEN** `word_break_segmentation_enabled = false` and a continuous utterance exceeds 4 seconds with mid-utterance pauses
- **THEN** no segment is extracted at the pause boundaries; a single segment is produced when speech ends or `segment_max_duration_ms` is reached

#### Scenario: Enabled word-break segmentation splits utterances at pauses
- **WHEN** `word_break_segmentation_enabled = true` and a continuous utterance exceeds 4 seconds with a qualifying mid-utterance pause
- **THEN** a segment is extracted at the word-break boundary
