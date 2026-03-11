## MODIFIED Requirements

### Requirement: File playback through full pipeline
Opening or reprocessing a WAV file SHALL feed the audio through the complete engine pipeline — visualization (waveform, spectrogram, speech activity), VAD, and transcription — and play it audibly through the system default output device. The source file SHALL always be the raw (unprocessed) WAV. If the provided path points to a processed variant (`-processed.wav`), the engine SHALL resolve it to the corresponding raw file (`<stem>.wav`) before reading. Audible output SHALL come from the engine's render pipeline (WASAPI render endpoint), not from a browser audio element.

#### Scenario: Playback sources raw WAV when given processed path
- **WHEN** `play_file()` is called with a path ending in `-processed.wav`
- **THEN** the engine reads the corresponding raw file (`<stem>.wav`) from the same directory

#### Scenario: Playback sources raw WAV when given raw path
- **WHEN** `play_file()` is called with a path ending in `<stem>.wav` (no `-processed` suffix)
- **THEN** the engine reads that file directly

#### Scenario: Playback fails gracefully when raw file missing
- **WHEN** `play_file()` is called with a processed path and the corresponding raw file does not exist
- **THEN** the engine falls back to reading the processed file and logs a warning

#### Scenario: Visualizations active during playback
- **WHEN** file playback is in progress
- **THEN** waveform, spectrogram, and speech activity renderers update in real time from the injected audio, reflecting current processing settings (mic gain, AGC)

#### Scenario: Audible output during playback
- **WHEN** file playback is in progress
- **THEN** the processed audio is audible through the system default output device via the engine render pipeline

#### Scenario: Audible output reflects current AGC settings
- **WHEN** a file recorded without AGC is played back with AGC enabled
- **THEN** the audible output and visualization both show the effect of AGC applied to the raw recording

#### Scenario: PTT mode submits whole file as one segment
- **WHEN** `ptt_mode` is `true` (auto-transcription disabled)
- **THEN** the entire file is accumulated as a single manual recording segment and submitted for transcription when playback ends

#### Scenario: VAD mode auto-segments
- **WHEN** `ptt_mode` is `false` (auto-transcription enabled)
- **THEN** speech detection drives automatic segmentation during playback, submitting segments as they are detected

### Requirement: Reprocess clears and replaces transcription output
When the Play button is clicked for an existing recording, the transcription output panel SHALL be cleared before the reprocess operation begins, and populated with new results as they arrive.

#### Scenario: Transcription output cleared on reprocess start
- **WHEN** the user clicks Play with an active document
- **THEN** the transcription output panel is cleared, visualizations are reset, and a `"Playing..."` status is shown

#### Scenario: Transcription output populated on reprocess complete
- **WHEN** the `playback-complete` event fires
- **THEN** the transcription output panel shows the results collected during playback and status returns to `"Ready"`

#### Scenario: Error shown on reprocess failure
- **WHEN** the playback command rejects
- **THEN** the status text shows an error message and the transcription output retains its cleared state

## REMOVED Requirements

### Requirement: Reprocess command
**Reason**: The separate `reprocess_file` Tauri command and `btn-reprocess` button are superseded by the Play button, which now always reprocesses from raw through the engine pipeline. The `open_file` command remains as the underlying mechanism.
**Migration**: Use `open_file` / Play button for all reprocessing. Remove `btn-reprocess` UI element and `reprocess_file` Tauri command.
