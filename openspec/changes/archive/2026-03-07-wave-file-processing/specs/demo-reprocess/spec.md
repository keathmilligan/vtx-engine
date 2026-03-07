## ADDED Requirements

### Requirement: Reprocess command
The demo app SHALL provide a `reprocess_file` Tauri command that accepts a WAV file path and runs it through the transcription pipeline, returning a `Vec<TranscriptionSegment>`. This command is functionally equivalent to `transcribe_file` but is a distinct command to allow independent evolution.

#### Scenario: Reprocess returns segments
- **WHEN** `invoke("reprocess_file", { path })` is called with a valid WAV file path
- **THEN** the command returns a `Vec<TranscriptionSegment>` with one or more segments

#### Scenario: Reprocess returns error for missing file
- **WHEN** `invoke("reprocess_file", { path })` is called with a path that does not exist
- **THEN** the command rejects with an error string

### Requirement: Reprocess clears and replaces transcription output
When the Reprocess button is clicked, the transcription output panel SHALL be cleared before the reprocess operation begins, and populated with the new results on completion.

#### Scenario: Transcription output cleared on reprocess start
- **WHEN** the user clicks `btn-reprocess`
- **THEN** the transcription output panel is cleared and a `"Reprocessing..."` status is shown

#### Scenario: Transcription output populated on reprocess complete
- **WHEN** the `reprocess_file` command returns successfully
- **THEN** the transcription output panel shows the new results and status returns to `"Ready"`

#### Scenario: Error shown on reprocess failure
- **WHEN** the `reprocess_file` command rejects
- **THEN** the status text shows an error message and the transcription output retains its cleared state

### Requirement: Reprocess button disabled during reprocessing
The `btn-reprocess` button SHALL be disabled while a reprocess operation is in progress to prevent concurrent reprocess calls.

#### Scenario: Button disabled during reprocess
- **WHEN** `invoke("reprocess_file", ...)` is in-flight
- **THEN** `btn-reprocess` is disabled

#### Scenario: Button re-enabled after reprocess
- **WHEN** `invoke("reprocess_file", ...)` completes (success or error)
- **THEN** `btn-reprocess` is re-enabled (provided `activeDocumentPath` is still set and not recording)
