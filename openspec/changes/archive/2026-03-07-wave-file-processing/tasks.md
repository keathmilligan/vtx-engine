## 1. Engine: Expose Last Recording Path

- [x] 1.1 Add `last_recording_path: Arc<Mutex<Option<PathBuf>>>` field to `AudioEngine` struct in `crates/vtx-engine/src/lib.rs`
- [x] 1.2 After `save_to_wav` succeeds in `transcribe_state.rs` (`submit_recording`), store the resulting path in `last_recording_path`
- [x] 1.3 Implement `pub fn get_last_recording_path(&self) -> Option<PathBuf>` on `AudioEngine`

## 2. Tauri Backend: Update stop_recording and Add reprocess_file

- [x] 2.1 Change `stop_recording` command signature in `apps/vtx-demo/src-tauri/src/lib.rs` to return `Result<Option<String>, String>`
- [x] 2.2 After calling `engine.stop_recording()`, call `engine.get_last_recording_path()` and return the path as an `Option<String>` (absolute path string)
- [x] 2.3 Add `reprocess_file` Tauri command that accepts `path: String` and calls `engine.transcribe_audio_file(path).await`, returning `Result<Vec<TranscriptionSegment>, String>`
- [x] 2.4 Register `reprocess_file` in the `invoke_handler!` macro

## 3. Frontend: Active Document State

- [x] 3.1 Add `let activeDocumentPath: string | null = null` module-level state variable in `apps/vtx-demo/src/main.ts`
- [x] 3.2 Add `setActiveDocument(path: string | null)` helper that updates `activeDocumentPath`, updates the `<h1>` text (and `document.title`), and updates the enabled/disabled state of `btn-reprocess`
- [x] 3.3 In `openWavFile()`, call `setActiveDocument(filePath)` after the dialog returns a valid path
- [x] 3.4 In `stopRecording()`, await the updated `invoke<string | null>("stop_recording")` return value and call `setActiveDocument(path)` if non-null

## 4. Frontend: Add Reprocess Button

- [x] 4.1 Add `<button id="btn-reprocess" disabled>Reprocess</button>` to the `#action-buttons` div in `apps/vtx-demo/index.html`, after `btn-open-file`
- [x] 4.2 Add `const btnReprocess = document.getElementById("btn-reprocess") as HTMLButtonElement` in `main.ts`
- [x] 4.3 Wire `btnReprocess.addEventListener("click", reprocessFile)` in `setupEventListeners()`
- [x] 4.4 Implement `async function reprocessFile()` that: disables `btn-reprocess`, clears transcription output, sets status to `"Reprocessing..."`, calls `invoke("reprocess_file", { path: activeDocumentPath })`, renders results via `addTranscriptionResult`, restores status, and re-enables `btn-reprocess`
- [x] 4.5 Ensure `btn-reprocess` is also disabled in `startRecording()` and re-enabled (if document is set) in `stopRecording()`

## 5. Frontend: Button Labels and Title Element

- [x] 5.1 Change the `btn-open-file` button label from `"Open WAV File"` to `"Open"` in `index.html`
- [x] 5.2 Change the `<h1>` element content from `"vtx-engine Demo"` to `"VTX Engine Demo"` in `index.html` (canonical capitalisation for display)
- [x] 5.3 Add `const appTitle = document.querySelector("header h1")!` DOM reference in `main.ts`

## 6. CSS: Button Color Updates

- [x] 6.1 Change `button#btn-open-file` background from `#8b5cf6` to `var(--btn-primary-bg)` (`#396cd8`) in `styles.css`
- [x] 6.2 Change `button#btn-open-file:hover` background from `#7c3aed` to `var(--btn-primary-bg-hover)` (`#2d5ab8`)
- [x] 6.3 Change `button#btn-capture` base style to always use `var(--btn-recording-bg)` (`#dc3545`) regardless of `.recording` class — update or consolidate the existing `button#btn-capture.recording` rule
- [x] 6.4 Add `button#btn-capture:hover:not(:disabled)` rule using `var(--btn-recording-bg-hover)` (`#c82333`)
- [x] 6.5 Add `button#btn-reprocess` style (use default blue, same as base `button` rule — no override needed unless a distinct color is desired)
