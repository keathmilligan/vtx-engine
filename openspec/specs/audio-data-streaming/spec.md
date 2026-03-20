## Requirements

### Requirement: Engine streams processed audio samples when audio streaming is enabled
When processed audio streaming is enabled via the builder, the audio loop SHALL emit an `EngineEvent::AudioData` event for every chunk of processed audio during live capture. Each event SHALL carry the mono f32 PCM samples after gain and AGC processing, the sample rate, and a cumulative sample offset. Events SHALL NOT be emitted when processed audio streaming is disabled.

#### Scenario: Processed audio data events are emitted during capture with streaming enabled
- **WHEN** `EngineBuilder::new().with_audio_streaming().build().await` is called and `start_capture()` succeeds
- **THEN** `EngineEvent::AudioData` events are emitted on the broadcast channel for every audio chunk received from the platform backend

#### Scenario: Processed audio data events are not emitted when streaming is disabled
- **WHEN** `EngineBuilder::new().build().await` is called (without `with_audio_streaming()`) and `start_capture()` succeeds
- **THEN** no `EngineEvent::AudioData` events are emitted on the broadcast channel

#### Scenario: Processed audio data events stop when capture stops
- **WHEN** processed audio streaming is enabled and `stop_capture()` is called
- **THEN** no further `EngineEvent::AudioData` events are emitted after the `CaptureStateChanged { capturing: false }` event

### Requirement: Engine streams raw audio samples when raw audio streaming is enabled
When raw audio streaming is enabled via the builder, the audio loop SHALL emit an `EngineEvent::RawAudioData` event for every chunk of audio during live capture. Each event SHALL carry the mono f32 PCM samples before gain and AGC processing (immediately after mono conversion), the sample rate, and a cumulative sample offset. Events SHALL NOT be emitted when raw audio streaming is disabled.

#### Scenario: Raw audio data events are emitted during capture with raw streaming enabled
- **WHEN** `EngineBuilder::new().with_raw_audio_streaming().build().await` is called and `start_capture()` succeeds
- **THEN** `EngineEvent::RawAudioData` events are emitted on the broadcast channel for every audio chunk received from the platform backend

#### Scenario: Raw audio data events are not emitted when raw streaming is disabled
- **WHEN** `EngineBuilder::new().build().await` is called (without `with_raw_audio_streaming()`) and `start_capture()` succeeds
- **THEN** no `EngineEvent::RawAudioData` events are emitted on the broadcast channel

#### Scenario: Raw audio data events stop when capture stops
- **WHEN** raw audio streaming is enabled and `stop_capture()` is called
- **THEN** no further `EngineEvent::RawAudioData` events are emitted after the `CaptureStateChanged { capturing: false }` event

### Requirement: Processed and raw audio streaming are independently controllable
A consumer SHALL be able to enable processed audio streaming, raw audio streaming, or both, independently. Enabling one stream SHALL NOT require enabling the other.

#### Scenario: Only processed audio streaming enabled
- **WHEN** `EngineBuilder::new().with_audio_streaming().build().await` is called and capture is active
- **THEN** `EngineEvent::AudioData` events are emitted
- **THEN** no `EngineEvent::RawAudioData` events are emitted

#### Scenario: Only raw audio streaming enabled
- **WHEN** `EngineBuilder::new().with_raw_audio_streaming().build().await` is called and capture is active
- **THEN** `EngineEvent::RawAudioData` events are emitted
- **THEN** no `EngineEvent::AudioData` events are emitted

#### Scenario: Both streams enabled simultaneously
- **WHEN** `EngineBuilder::new().with_audio_streaming().with_raw_audio_streaming().build().await` is called and capture is active
- **THEN** both `EngineEvent::AudioData` and `EngineEvent::RawAudioData` events are emitted for every audio chunk

### Requirement: Processed audio data contains post-processing mono samples
Each `EngineEvent::AudioData` event SHALL carry a `samples: Vec<f32>` field containing mono audio samples that have been processed through the full pipeline (mono conversion, software mic gain, and AGC). The samples SHALL be in the range -1.0 to 1.0. The samples SHALL be the same processed mono samples that are passed to the VAD and visualization subsystems.

#### Scenario: Processed samples reflect gain and AGC processing
- **WHEN** mic gain is set to a non-zero value and AGC is enabled, and processed audio streaming is enabled
- **THEN** the `samples` field in each `AudioData` event contains samples with both gain and AGC applied

### Requirement: Raw audio data contains pre-processing mono samples
Each `EngineEvent::RawAudioData` event SHALL carry a `samples: Vec<f32>` field containing mono audio samples that have been converted to mono but have NOT had software mic gain or AGC applied. The samples SHALL be in the range -1.0 to 1.0.

#### Scenario: Raw samples do not reflect gain or AGC processing
- **WHEN** mic gain is set to a non-zero value and AGC is enabled, and raw audio streaming is enabled
- **THEN** the `samples` field in each `RawAudioData` event contains samples without gain or AGC applied

#### Scenario: Raw samples are mono
- **WHEN** the platform backend delivers stereo audio and raw audio streaming is enabled
- **THEN** the `samples` field in each `RawAudioData` event contains mono (single-channel) samples

### Requirement: Both audio data variants include the sample rate
Each `EngineEvent::AudioData` and `EngineEvent::RawAudioData` event SHALL carry a `sample_rate: u32` field indicating the sample rate of the audio samples in Hz. During live capture this SHALL be the engine's target capture sample rate (48000 Hz).

#### Scenario: Sample rate is reported correctly for processed audio
- **WHEN** processed audio streaming is enabled and capture is active
- **THEN** every `AudioData` event has `sample_rate` equal to 48000

#### Scenario: Sample rate is reported correctly for raw audio
- **WHEN** raw audio streaming is enabled and capture is active
- **THEN** every `RawAudioData` event has `sample_rate` equal to 48000

### Requirement: Both audio data variants include a cumulative sample offset for timing
Each `EngineEvent::AudioData` and `EngineEvent::RawAudioData` event SHALL carry a `sample_offset: u64` field representing the total number of audio samples emitted for that stream since the current capture session began, not counting the samples in the current event. The first event of a capture session SHALL have `sample_offset` equal to 0. Each subsequent event's `sample_offset` SHALL equal the previous event's `sample_offset` plus the previous event's `samples.len()`. The counter SHALL reset to 0 when a new capture session starts. When both streams are enabled, both SHALL use the same `sample_offset` sequence since they are derived from the same audio chunks.

#### Scenario: First chunk has sample offset zero
- **WHEN** either audio streaming option is enabled and `start_capture()` is called
- **THEN** the first event of that stream has `sample_offset` equal to 0

#### Scenario: Sample offset increments by chunk size
- **WHEN** two consecutive events of the same stream type are received, where the first has `sample_offset = N` and `samples.len() = L`
- **THEN** the second event has `sample_offset = N + L`

#### Scenario: Sample offset resets on new capture session
- **WHEN** `stop_capture()` is called followed by a new `start_capture()` with audio streaming enabled
- **THEN** the first event of each enabled stream in the new session has `sample_offset` equal to 0

#### Scenario: Both streams share the same sample offset sequence
- **WHEN** both processed and raw audio streaming are enabled
- **THEN** the `AudioData` and `RawAudioData` events emitted for the same audio chunk have identical `sample_offset` values

### Requirement: Consumers can compute audio timestamp from sample offset
A consumer SHALL be able to compute the precise timestamp of any audio chunk relative to the start of the capture session using the formula `timestamp_seconds = sample_offset / sample_rate`. This enables A/V synchronization when the consumer correlates the `CaptureStateChanged { capturing: true }` event with the start of their own video timeline.

#### Scenario: Timestamp computation yields correct value
- **WHEN** an `AudioData` or `RawAudioData` event has `sample_offset = 480000` and `sample_rate = 48000`
- **THEN** the consumer computes the chunk timestamp as 10.0 seconds from capture start
