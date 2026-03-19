## Why

Users currently cannot select or download whisper transcription models from the settings dialog. The `ModelManager` API exists for downloading models, and `EngineConfig` already has a `model` field, but there's no UI to interact with these capabilities. Users must manually download model files or rely on default models, limiting flexibility and ease of use.

## What Changes

- Add a "Model" section to the configuration panel showing available whisper models
- Display download status for each model (downloaded, not downloaded, downloading with progress)
- Allow users to select which model to use for transcription
- Allow users to download missing models directly from the settings dialog
- Show download progress with a progress bar during model downloads
- Persist the selected model in `EngineConfig` and apply it on engine restart

## Capabilities

### New Capabilities
- `whisper-model-ui`: UI for selecting and downloading whisper models in the settings dialog

### Modified Capabilities
- `demo-configuration-ui`: Add Model section to the configuration panel
- `engine-config-persistence`: Ensure model selection persists and loads correctly

## Impact

- Frontend: New Model section in config panel with model list, download buttons, progress indicators
- Backend: Expose `ModelManager` functionality via Tauri commands (list models, check availability, download with progress)
- Config: `EngineConfig.model` field already exists; UI will now expose it
- User experience: Users can switch between models (tiny, base, small, medium, large) and download new ones without leaving the app
