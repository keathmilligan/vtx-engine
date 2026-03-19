## Purpose

Specification for the Whisper model management UI in the vtx-engine demo application, allowing users to view available models, download models, and select the active transcription model.

## Requirements

### Requirement: get_model_status Tauri command returns all models with availability
The demo backend SHALL expose a `get_model_status` Tauri command that returns an array of objects, one for each `WhisperModel` variant. Each object SHALL contain: `model` (snake_case string), `name` (display name like "Tiny En"), `size_mb` (approximate file size), and `downloaded` (boolean indicating if the model file exists in cache).

#### Scenario: get_model_status returns all WhisperModel variants
- **WHEN** the frontend calls `invoke("get_model_status")`
- **THEN** an array of 9 model status objects is returned (one per WhisperModel variant)
- **THEN** each object has `model`, `name`, `size_mb`, and `downloaded` fields

#### Scenario: Downloaded field reflects cache state
- **WHEN** `WhisperModel::BaseEn` is cached but `WhisperModel::LargeV3` is not
- **THEN** the object for `base_en` has `downloaded: true`
- **THEN** the object for `large_v3` has `downloaded: false`

### Requirement: download_model Tauri command starts async download with progress events
The demo backend SHALL expose a `download_model` Tauri command accepting a `model` parameter (snake_case string). The command SHALL start an async download via `ModelManager` and return immediately. Progress SHALL be emitted via Tauri events named `model-download-progress` with payload `{ model: string, progress: number }` where progress is 0-100. Completion SHALL emit a final event with `progress: 100`. Failure SHALL emit `model-download-error` with payload `{ model: string, error: string }`.

#### Scenario: download_model starts download and returns immediately
- **WHEN** the frontend calls `invoke("download_model", { model: "tiny_en" })`
- **THEN** the call returns without waiting for download completion
- **THEN** `model-download-progress` events are emitted during download

#### Scenario: Progress events include model identifier
- **WHEN** downloading `tiny_en` model
- **THEN** each `model-download-progress` event payload has `model: "tiny_en"`

#### Scenario: Download completion emits progress 100
- **WHEN** a download completes successfully
- **THEN** a `model-download-progress` event with `progress: 100` is emitted

#### Scenario: Download failure emits error event
- **WHEN** a download fails due to network error
- **THEN** a `model-download-error` event is emitted with the model name and error message

### Requirement: cancel_download Tauri command stops in-progress download
The demo backend SHALL expose a `cancel_download` Tauri command accepting a `model` parameter. If a download for that model is in progress, it SHALL be cancelled and the temp file cleaned up. The command SHALL emit `model-download-cancelled` event with payload `{ model: string }`.

#### Scenario: cancel_download stops active download
- **WHEN** `tiny_en` is downloading and the frontend calls `invoke("cancel_download", { model: "tiny_en" })`
- **THEN** the download is cancelled
- **THEN** a `model-download-cancelled` event is emitted

#### Scenario: cancel_download for non-downloading model is a no-op
- **WHEN** no download is in progress for `base_en` and the frontend calls `invoke("cancel_download", { model: "base_en" })`
- **THEN** no error is returned
- **THEN** no event is emitted

### Requirement: Model section displays all models with status and actions
The configuration panel SHALL include a Model section as the first section. It SHALL display a list of all available `WhisperModel` variants with their display name, size, and current status. Each model row SHALL show: a radio button or selection indicator, the model name, the size in MB, and an action button (Download if not cached, Cancel if downloading, or just the selection indicator if already downloaded).

#### Scenario: Model section is visible in config panel
- **WHEN** the configuration panel is opened
- **THEN** a Model section is visible as the first section
- **THEN** all 9 WhisperModel variants are listed

#### Scenario: Downloaded models show as available
- **WHEN** `WhisperModel::BaseEn` is cached
- **THEN** the Base En row shows a selection indicator and no download button

#### Scenario: Not-downloaded models show download button
- **WHEN** `WhisperModel::LargeV3` is not cached
- **THEN** the Large V3 row shows a Download button

### Requirement: Selecting a model updates EngineConfig
When the user selects a model that is already downloaded, the selection SHALL be stored in the form state. When the user clicks Save, the selected model SHALL be persisted via `set_engine_config` and the model name badge in the status bar SHALL update.

#### Scenario: Selecting downloaded model updates form state
- **WHEN** the user selects `Small En` (already downloaded) in the Model section
- **THEN** the form state records `model: "small_en"`
- **THEN** the selection is visually indicated

#### Scenario: Save persists selected model
- **WHEN** the user selects `Small En` and clicks Save
- **THEN** `set_engine_config` is called with `model: "small_en"`
- **THEN** the model name badge in the status bar shows "Small En"

### Requirement: Downloading a model shows progress bar
When the user clicks Download for a model, a progress bar SHALL appear in that model's row. The progress bar SHALL update in real-time as `model-download-progress` events are received. When download completes, the progress bar SHALL be replaced with the downloaded state and the model SHALL become selectable.

#### Scenario: Download button starts download and shows progress
- **WHEN** the user clicks Download for `Medium En`
- **THEN** a download is started
- **THEN** a progress bar appears in the Medium En row
- **THEN** the Download button is replaced with a Cancel button

#### Scenario: Progress bar updates with events
- **WHEN** a `model-download-progress` event with `progress: 50` is received for `medium_en`
- **THEN** the progress bar for Medium En shows 50% filled

#### Scenario: Download completion enables selection
- **WHEN** a download for `Medium En` completes (progress 100)
- **THEN** the progress bar is removed
- **THEN** Medium En shows as downloaded and is selectable

### Requirement: Download errors are displayed inline
When a `model-download-error` event is received, an error message SHALL be displayed in the affected model's row. A retry button SHALL be shown to allow re-attempting the download.

#### Scenario: Network error shows error message
- **WHEN** a download for `Large V3` fails with a network error
- **THEN** an error message appears in the Large V3 row
- **THEN** a Retry button is shown

### Requirement: Cancel button stops download and resets state
When the user clicks Cancel during a download, the download SHALL be cancelled, the progress bar removed, and the row reset to show the Download button.

#### Scenario: Cancel button stops download
- **WHEN** the user clicks Cancel while `Small En` is downloading
- **THEN** `cancel_download` is invoked
- **THEN** the progress bar is removed
- **THEN** the Download button reappears

### Requirement: Model section shows warning during active capture
When the configuration panel is opened while audio capture is active, the Model section SHALL display a warning that model changes will take effect on the next capture session.

#### Scenario: Warning shown during active capture
- **WHEN** the configuration panel is opened while capture is running
- **THEN** a warning is displayed in the Model section indicating changes apply on next capture start
