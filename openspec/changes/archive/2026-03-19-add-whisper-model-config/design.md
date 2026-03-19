## Context

The `ModelManager` API in `vtx-engine` already provides model path resolution, availability checking, listing cached models, and async downloads with progress callbacks. The `EngineConfig` already has a `model: WhisperModel` field that persists. What's missing is the UI layer to expose these capabilities to users.

The demo app uses Tauri for frontend-backend communication. The configuration panel already exists and handles `EngineConfig` fields. We need to extend it with model selection and download functionality.

## Goals / Non-Goals

**Goals:**
- Expose `ModelManager` functionality via Tauri commands
- Add Model section to the configuration panel
- Show all available `WhisperModel` variants with their download status
- Allow downloading models with progress indication
- Persist selected model via existing `EngineConfig.model` field

**Non-Goals:**
- Auto-downloading models on first use (user must explicitly download)
- Model deletion from the UI
- Custom model paths (use canonical `ModelManager` paths only)
- Changing models while capture is active (requires restart warning)

## Decisions

### D1: Tauri commands for model management
Expose `ModelManager` via three Tauri commands:
- `get_model_status` → returns list of all models with `{ model, name, size_mb, downloaded }`
- `download_model` → starts download, emits progress via Tauri event
- `cancel_download` → cancels in-progress download (optional, defer if complex)

**Rationale:** Mirrors existing `get_engine_config`/`set_engine_config` pattern. Progress via events matches Tauri best practices for long-running operations.

### D2: Download progress via Tauri events
Use `app.emit("model-download-progress", { model, progress })` for progress updates. Frontend listens via `listen("model-download-progress", ...)`.

**Rationale:** Consistent with existing `transcription-segment` event pattern. Avoids polling.

### D3: Model section placement in config panel
Add Model section as the first section in the configuration panel, before Audio Input.

**Rationale:** Model selection is foundational - it affects transcription quality and is a common first-time setup task.

### D4: Model list UI structure
Display models in a table/list with columns: Model name, Size, Status (downloaded/not downloaded/downloading), Action (Select/Download/Cancel).

**Rationale:** Clear visual hierarchy. Users can see all options at once. Size info helps users choose based on disk space/quality tradeoff.

## Risks / Trade-offs

- **[Risk] Download fails mid-transfer** → Progress bar shows error state, retry button appears. Temp `.download` file cleaned up by `ModelManager`.
- **[Risk] User closes config panel during download** → Download continues in background. Re-opening panel shows current progress.
- **[Risk] Large model downloads take long time** → Show estimated time if possible, allow cancel. Default to smaller models in profiles.
- **[Risk] Network unavailable** → Show error message, disable download buttons for unavailable models.
