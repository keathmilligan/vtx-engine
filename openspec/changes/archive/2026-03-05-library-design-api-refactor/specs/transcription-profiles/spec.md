## ADDED Requirements

### Requirement: TranscriptionProfile enum selects a preset parameter bundle
The library SHALL expose a `TranscriptionProfile` enum in `vtx-common` with variants `Dictation`, `Transcription`, and `Custom`. Each variant except `Custom` SHALL define a fixed set of preset values for: `vad_voiced_threshold_db`, `vad_whisper_threshold_db`, `vad_voiced_onset_ms`, `vad_whisper_onset_ms`, `segment_max_duration_ms`, `segment_word_break_grace_ms`, `word_break_segmentation_enabled`, and default `WhisperModel`. `Custom` SHALL carry no preset values and leave all `EngineConfig` fields at their `Default` values.

#### Scenario: Dictation profile applies short-burst defaults
- **WHEN** `EngineBuilder::new().with_profile(TranscriptionProfile::Dictation).build().await` is called
- **THEN** the resulting engine uses `segment_max_duration_ms = 4_000`, `word_break_segmentation_enabled = true`, `segment_word_break_grace_ms = 750`, and `model = WhisperModel::BaseEn`

#### Scenario: Transcription profile applies long-form defaults
- **WHEN** `EngineBuilder::new().with_profile(TranscriptionProfile::Transcription).build().await` is called
- **THEN** the resulting engine uses `segment_max_duration_ms = 15_000`, `word_break_segmentation_enabled = false`, and `model = WhisperModel::MediumEn`

#### Scenario: Custom profile leaves config at defaults
- **WHEN** `EngineBuilder::new().with_profile(TranscriptionProfile::Custom).build().await` is called
- **THEN** no preset values are applied; all `EngineConfig` fields remain at their `Default` values

### Requirement: Profile setter is composable with individual config setters
`EngineBuilder::with_profile(profile)` SHALL apply preset values first, allowing subsequent individual setter calls to override individual fields. Calling `with_profile` after individual setters SHALL overwrite those fields.

#### Scenario: Individual setter overrides profile value
- **WHEN** `EngineBuilder::new().with_profile(TranscriptionProfile::Dictation).segment_max_duration_ms(8_000).build().await` is called
- **THEN** `segment_max_duration_ms` is `8_000` (the individual setter wins because it was called after the profile)

#### Scenario: Profile called after individual setter overwrites it
- **WHEN** `EngineBuilder::new().segment_max_duration_ms(8_000).with_profile(TranscriptionProfile::Dictation).build().await` is called
- **THEN** `segment_max_duration_ms` is `4_000` (the profile overwrites the earlier setter)

### Requirement: TranscriptionProfile is serializable
`TranscriptionProfile` SHALL derive `serde::Serialize` and `serde::Deserialize` using snake_case variant names (`"dictation"`, `"transcription"`, `"custom"`).

#### Scenario: Profile round-trips through JSON
- **WHEN** `TranscriptionProfile::Transcription` is serialized to JSON and then deserialized
- **THEN** the deserialized value equals `TranscriptionProfile::Transcription`
