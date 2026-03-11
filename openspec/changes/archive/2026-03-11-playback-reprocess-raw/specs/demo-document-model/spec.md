## MODIFIED Requirements

### Requirement: Active document state
The demo app SHALL maintain an active document — the currently open WAV file — as module-level state (`activeDocumentPath: string | null`). The active document is set when a WAV file is opened via the Open button or when a recording session ends and a WAV file has been saved. The active document path SHALL always point to the **raw** (unprocessed) WAV file. When a recording stops, `stop_recording` returns the raw WAV path. When a file is opened via the Open dialog, the path is used as-is (the engine resolves to raw internally during playback). The active document is `null` on app startup.

#### Scenario: No document on startup
- **WHEN** the app initializes
- **THEN** `activeDocumentPath` is `null` and the title reads `"VTX Engine Demo"`

#### Scenario: Document set on open
- **WHEN** the user selects a WAV file via the Open dialog
- **THEN** `activeDocumentPath` is set to the full path of the selected file

#### Scenario: Document set on recording stop
- **WHEN** a recording session stops and a WAV file was saved
- **THEN** `activeDocumentPath` is set to the path of the **raw** WAV file (not the processed variant)

### Requirement: Engine exposes last recording path
The `AudioEngine` SHALL expose a `get_last_recording_path() -> Option<PathBuf>` method that returns the file path of the most recently saved WAV file from a manual recording session. This value SHALL point to the **raw** WAV file path. It is updated each time `stop_recording()` completes and a WAV file is written.

#### Scenario: Path available after recording
- **WHEN** `stop_recording()` is called and a WAV file was successfully saved
- **THEN** `get_last_recording_path()` returns `Some(path)` pointing to the **raw** WAV file (no `-processed` suffix)

#### Scenario: Path unavailable before first recording
- **WHEN** no recording has yet completed
- **THEN** `get_last_recording_path()` returns `None`

### Requirement: stop_recording Tauri command returns WAV path
The `stop_recording` Tauri command SHALL return `Result<Option<String>, String>` where the `Ok` value is the absolute path of the saved **raw** WAV file (or `None` if no file was written).

#### Scenario: Stop recording returns raw path
- **WHEN** `invoke("stop_recording")` is called after a recording session
- **THEN** the resolved value is the absolute path string of the raw WAV file

#### Scenario: Stop recording returns None when no file saved
- **WHEN** `invoke("stop_recording")` is called but no audio was captured
- **THEN** the resolved value is `null`

### Requirement: No HTMLAudioElement for playback
The demo app SHALL NOT use an `HTMLAudioElement` for audible playback of recordings. All audible output during file playback SHALL come from the engine's render pipeline. The `startFilePlayback()` function SHALL only invoke the engine command and manage UI state.

#### Scenario: No browser audio element created during playback
- **WHEN** `startFilePlayback()` is called
- **THEN** no `HTMLAudioElement` is created or played
- **THEN** the engine command `open_file` is invoked and handles both processing and audible output
