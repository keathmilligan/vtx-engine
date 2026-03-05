## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: EngineConfig exposes word_break_segmentation_enabled
`EngineConfig` SHALL contain a `word_break_segmentation_enabled: bool` field with default value `true`. When `false`, the audio loop SHALL detect word-break events internally but SHALL NOT use them to split segments; segment boundaries SHALL be determined solely by speech-end detection and `segment_max_duration_ms`.

#### Scenario: Disabled word-break segmentation does not split long utterances at pauses
- **WHEN** `word_break_segmentation_enabled = false` and a continuous utterance exceeds 4 seconds with mid-utterance pauses
- **THEN** no segment is extracted at the pause boundaries; a single segment is produced when speech ends or `segment_max_duration_ms` is reached

#### Scenario: Enabled word-break segmentation splits utterances at pauses
- **WHEN** `word_break_segmentation_enabled = true` and a continuous utterance exceeds 4 seconds with a qualifying mid-utterance pause
- **THEN** a segment is extracted at the word-break boundary
