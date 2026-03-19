## 1. Backend Tauri Commands

- [x] 1.1 Add `get_model_status` Tauri command returning array of model status objects
- [x] 1.2 Add `download_model` Tauri command with async download and progress events
- [x] 1.3 Add `cancel_download` Tauri command to stop in-progress downloads
- [x] 1.4 Add model size constants (MB) for each WhisperModel variant
- [x] 1.5 Add display name mapping for each WhisperModel variant

## 2. Frontend Model Section UI

- [x] 2.1 Add Model section HTML structure to config panel (before Audio Input)
- [x] 2.2 Add CSS styles for model list rows and progress bars
- [x] 2.3 Implement `get_model_status` call on panel open
- [x] 2.4 Render model list with name, size, status, and action buttons
- [x] 2.5 Add radio button selection for downloaded models
- [x] 2.6 Add Download button for not-downloaded models
- [x] 2.7 Add Cancel button and progress bar for downloading models

## 3. Download Progress Handling

- [x] 3.1 Add event listener for `model-download-progress` events
- [x] 3.2 Add event listener for `model-download-error` events
- [x] 3.3 Add event listener for `model-download-cancelled` events
- [x] 3.4 Update progress bar in real-time from progress events
- [x] 3.5 Show error message and retry button on download error
- [x] 3.6 Reset row state on download cancellation

## 4. Model Selection Persistence

- [x] 4.1 Track selected model in form state
- [x] 4.2 Include model field in Save button handler
- [x] 4.3 Update model name badge in status bar after save
- [x] 4.4 Pre-select current model from `get_engine_config` on panel open

## 5. Capture State Warning

- [x] 5.1 Add warning banner to Model section when capture is active
- [x] 5.2 Toggle warning visibility based on capture state
