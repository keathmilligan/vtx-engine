## Purpose

Reprocess capability for the vtx-demo application. Replays the active WAV document through the full engine pipeline (visualization + VAD + transcription), clearing previous results and producing a fresh run.

## Requirements

### Requirement: Reprocess command
The demo app SHALL provide an `open_file` Tauri command (and aliased `reprocess_file` command) that accepts a WAV file path and `ptt_mode` flag, playing it through the full engine pipeline (visualization, VAD, transcription) via the audio injection channel. Results are delivered via broadcast events, not as a direct return value.

#### Scenario: Reprocess triggers playback
- **WHEN** `invoke("reprocess_file", { path, pttMode })` is called with a valid WAV file path
- **THEN** the engine begins feeding audio through the pipeline and emits visualization and transcription events

#### Scenario: Reprocess returns error for missing file
- **WHEN** `invoke("reprocess_file", { path, pttMode })` is called with a path that does not exist
- **THEN** the command rejects with an error string

### Requirement: Reprocess clears and replaces transcription output
When the Reprocess button is clicked, the transcription output panel SHALL be cleared before the reprocess operation begins, and populated with new results as they arrive.

#### Scenario: Transcription output cleared on reprocess start
- **WHEN** the user clicks `btn-reprocess`
- **THEN** the transcription output panel is cleared, visualizations are reset, and a `"Playing..."` status is shown

#### Scenario: Transcription output populated on reprocess complete
- **WHEN** the `playback-complete` event fires
- **THEN** the transcription output panel shows the results collected during playback and status returns to `"Ready"`

#### Scenario: Error shown on reprocess failure
- **WHEN** the `reprocess_file` command rejects
- **THEN** the status text shows an error message and the transcription output retains its cleared state

### Requirement: Reprocess button disabled during reprocessing
The `btn-reprocess` button SHALL be disabled while a reprocess/playback operation is in progress to prevent concurrent calls.

#### Scenario: Button disabled during reprocess
- **WHEN** file playback is in-flight
- **THEN** `btn-reprocess` is disabled

#### Scenario: Button re-enabled after reprocess
- **WHEN** the `playback-complete` event fires (success or error)
- **THEN** `btn-reprocess` is re-enabled (provided `activeDocumentPath` is still set and not recording)

### Requirement: File playback through full pipeline
Opening or reprocessing a WAV file SHALL feed the audio through the complete engine pipeline — visualization (waveform, spectrogram, speech activity), VAD, and transcription — and play it audibly through the default system output device.

#### Scenario: Visualizations active during playback
- **WHEN** file playback is in progress
- **THEN** waveform, spectrogram, and speech activity renderers update in real time from the injected audio

#### Scenario: Audible output during playback
- **WHEN** file playback is in progress
- **THEN** the WAV file audio is audible through the system default output device

#### Scenario: PTT mode submits whole file as one segment
- **WHEN** `ptt_mode` is `true` (auto-transcription disabled)
- **THEN** the entire file is accumulated as a single manual recording segment and submitted for transcription when playback ends

#### Scenario: VAD mode auto-segments
- **WHEN** `ptt_mode` is `false` (auto-transcription enabled)
- **THEN** speech detection drives automatic segmentation during playback, submitting segments as they are detected
