## Context

The demo app (`apps/vtx-demo`) is a Tauri 2 desktop app with a vanilla TypeScript frontend and a Rust backend. It currently has no concept of a "document" — state is ephemeral per session. The Record button starts live capture (which optionally writes a WAV file via the engine's manual recording subsystem), and the Open WAV File button opens a file for transcription. Neither action updates the UI to reflect the active file, and there is no way to re-run processing on a file once opened.

Key existing infrastructure:
- `generate_recording_filename()` → `vtx-{YYYYMMDD-HHMMSS}.wav` (in `crates/vtx-engine/src/audio.rs`)
- `recordings_dir()` → `{data_dir}/vtx-engine/recordings/`
- `engine.transcribe_audio_file(path)` — Tauri command `transcribe_file` already exists
- `EngineEvent::RecordingStopped { duration_ms }` — emitted when recording stops, but does NOT carry a file path
- The app header (`<h1>`) currently shows a static `"vtx-engine Demo"` string

## Goals / Non-Goals

**Goals:**
- Track the active document (current WAV file path) as frontend state
- Update the `<h1>` title to reflect the active document filename
- Change button labels and colors: "Open WAV File" → "Open" (blue), "Record" stays (red always)
- Add a "Reprocess" button that re-runs `transcribe_file` on the active document
- Enable Reprocess only when a document is open and not recording
- When recording stops, surface the generated WAV path as the active document

**Non-Goals:**
- No changes to the `vtx-engine` library API (new Tauri command only)
- No document persistence across sessions (active document resets on app restart)
- No multi-document or tabbed UI
- No waveform playback (audio playback in the renderer is not in scope)
- No changes to the existing `transcribe_file` Tauri command

## Decisions

### 1. Active document tracked in frontend only
The active document path is a module-level `string | null` variable in `main.ts` — no backend state needed. The Tauri `reprocess_file` command is just a thin wrapper around the existing `transcribe_audio_file` that takes a path argument. This keeps the backend stateless with respect to document identity.

**Alternative considered:** Store active document in `AppState` on the Rust side. Rejected — the frontend already manages session state (device selection, toggle states) in `localStorage`; keeping document state on the frontend is consistent and simpler.

### 2. WAV file path surfaced via new `recording-saved` Tauri event
`RecordingStopped` currently carries only `duration_ms`. Rather than modifying the engine's `EngineEvent` (a library-level change), the `stop_recording` Tauri command in `lib.rs` will be augmented to return the path of the saved WAV file as its `Ok` return value. The frontend then sets the active document from that return value.

**Alternative considered:** Add `wav_path: Option<String>` to `EngineEvent::RecordingStopped`. Rejected — this would be a library API change requiring a spec update to `broadcast-events`. The command return value approach is contained to the demo app.

**Alternative considered:** Emit a separate `recording-saved` Tauri event from `lib.rs` after `stop_recording()`. Viable, but returning the path directly from the command is simpler — the frontend already awaits `invoke("stop_recording")`.

### 3. `stop_recording` command returns `Option<String>` (the WAV path)
The engine's `stop_recording()` method currently returns `()`. To get the saved WAV path, the Tauri `stop_recording` command will call a new engine method `stop_recording_with_path() -> Option<PathBuf>` (or query the last saved path after stopping). 

**Simpler alternative chosen:** Instead of a new engine method, `stop_recording` in `lib.rs` will look up the recordings directory and the most recently modified `.wav` file immediately after stopping — this is a best-effort heuristic that works correctly since recordings are timestamped and written atomically. The frontend will use this path as the active document.

**Revised decision:** Query `engine.get_last_recording_path()` — a lightweight accessor on `AudioEngine` that stores the last WAV path written by `submit_recording()`. This is more reliable than filesystem scanning. This requires a small, self-contained addition to the engine's public API (one field + one accessor). No spec change needed since it is additive and internal to the demo's usage.

### 4. "Reprocess" button implemented as new `reprocess_file` Tauri command
The command is identical to `transcribe_file` in behavior — it calls `engine.transcribe_audio_file(path)`. A separate command is used so future differences (e.g., clearing previous results before reprocessing, progress events) can be added without touching `transcribe_file`.

### 5. Button colors via CSS ID selectors (existing pattern)
The existing CSS already uses `button#btn-capture.recording` and `button#btn-open-file` for per-button colors. The same pattern is used:
- `button#btn-capture` → always red (not just when `.recording`)
- `button#btn-open-file` → blue (change from current purple `#8b5cf6`)
- `button#btn-reprocess` → a neutral secondary color (e.g., `--btn-primary-bg`, the default blue) — or slightly muted to de-emphasize

### 6. Title display in `<h1>` element
The `<h1>` currently has static text `"vtx-engine Demo"`. It will be updated via JavaScript: when a document is active, set `h1.textContent = "VTX Engine Demo: " + filename`; when no document, reset to `"VTX Engine Demo"`. The `<title>` tag (browser/OS window title) will also be updated to match.

## Risks / Trade-offs

- **WAV path heuristic reliability:** The "query last recording path" approach depends on `submit_recording()` completing before `stop_recording` returns to the frontend. Since `submit_recording()` is synchronous within the Tauri command, this is safe.
- **Reprocess on a deleted file:** If the user reprocesses a file that has since been deleted, `transcribe_audio_file` will return an error. The frontend must handle this gracefully (show error in status, do not clear the document title).
- **Record button always red:** Changing `#btn-capture` to always be red (not just when recording) is a minor UX affordance change. The button will still show "Record"/"Stop" text to convey state. The `.recording` CSS class can be removed for this button since the base style is now red.

## Open Questions

- Should "Reprocess" clear the existing transcription output before re-running, or append to it? (Recommended: clear and replace — reprocessing implies a fresh run.)
- Should the active document persist across app restarts via `localStorage`? (Out of scope for this change, but a natural follow-up.)
