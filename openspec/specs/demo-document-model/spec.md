## Purpose

Document-oriented state management for the vtx-demo application. Tracks the currently open WAV file as the active document, reflects it in the UI title, and gates the Reprocess button on document availability.

## Requirements

### Requirement: Active document state
The demo app SHALL maintain an active document — the currently open WAV file — as module-level state (`activeDocumentPath: string | null`). The active document is set when a WAV file is opened via the Open button or when a recording session ends and a WAV file has been saved. The active document is `null` on app startup.

#### Scenario: No document on startup
- **WHEN** the app initializes
- **THEN** `activeDocumentPath` is `null` and the title reads `"VTX Engine Demo"`

#### Scenario: Document set on open
- **WHEN** the user selects a WAV file via the Open dialog
- **THEN** `activeDocumentPath` is set to the full path of the selected file

#### Scenario: Document set on recording stop
- **WHEN** a recording session stops and a WAV file was saved
- **THEN** `activeDocumentPath` is set to the path of the saved WAV file

### Requirement: Title reflects active document
The `<h1>` element in the app header and the browser `<title>` SHALL display the filename of the active document when one is open.

#### Scenario: Title with active document
- **WHEN** `activeDocumentPath` is set to a file path
- **THEN** the `<h1>` text reads `"VTX Engine Demo: <filename>"` where `<filename>` is the base filename (without directory) of the active document path

#### Scenario: Title without active document
- **WHEN** `activeDocumentPath` is `null`
- **THEN** the `<h1>` text reads `"VTX Engine Demo"`

### Requirement: Open button label and color
The button with id `btn-open-file` SHALL be labeled `"Open"` and styled blue using the existing `--btn-primary-bg` token (`#396cd8`), replacing the current purple styling.

#### Scenario: Open button appearance
- **WHEN** the app renders
- **THEN** the Open button label is `"Open"` and its background color is `#396cd8` (blue)

### Requirement: Record button always red
The button with id `btn-capture` SHALL always display with a red background (`--btn-recording-bg`) regardless of recording state. Its label SHALL toggle between `"Record"` (idle) and `"Stop"` (recording).

#### Scenario: Record button idle appearance
- **WHEN** the app is not recording
- **THEN** the Record button label is `"Record"` and its background is red (`#dc3545`)

#### Scenario: Record button active appearance
- **WHEN** recording is active
- **THEN** the Record button label is `"Stop"` and its background remains red

### Requirement: Reprocess button present and conditionally enabled
A button with id `btn-reprocess` and label `"Reprocess"` SHALL be present in the action buttons row. It SHALL be disabled when `activeDocumentPath` is `null` or when recording is active, and enabled otherwise.

#### Scenario: Reprocess disabled with no document
- **WHEN** `activeDocumentPath` is `null`
- **THEN** `btn-reprocess` is disabled

#### Scenario: Reprocess disabled while recording
- **WHEN** recording is active
- **THEN** `btn-reprocess` is disabled

#### Scenario: Reprocess enabled with document and not recording
- **WHEN** `activeDocumentPath` is set and recording is not active
- **THEN** `btn-reprocess` is enabled

### Requirement: Engine exposes last recording path
The `AudioEngine` SHALL expose a `get_last_recording_path() -> Option<PathBuf>` method that returns the file path of the most recently saved WAV file from a manual recording session. This value is updated each time `stop_recording()` completes and a WAV file is written.

#### Scenario: Path available after recording
- **WHEN** `stop_recording()` is called and a WAV file was successfully saved
- **THEN** `get_last_recording_path()` returns `Some(path)` pointing to the saved file

#### Scenario: Path unavailable before first recording
- **WHEN** no recording has yet completed
- **THEN** `get_last_recording_path()` returns `None`

### Requirement: stop_recording Tauri command returns WAV path
The `stop_recording` Tauri command SHALL return `Result<Option<String>, String>` where the `Ok` value is the absolute path of the saved WAV file (or `None` if no file was written).

#### Scenario: Stop recording returns path
- **WHEN** `invoke("stop_recording")` is called after a recording session
- **THEN** the resolved value is the absolute path string of the saved WAV file

#### Scenario: Stop recording returns None when no file saved
- **WHEN** `invoke("stop_recording")` is called but no audio was captured
- **THEN** the resolved value is `null`
