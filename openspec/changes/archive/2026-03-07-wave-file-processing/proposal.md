## Why

The demo app currently has no concept of a document — it is purely session-based with no way to revisit a prior recording or re-run processing on existing audio. Making each recording a first-class document (a WAV file) enables a persistent, file-centric workflow where users can open, review, and reprocess recordings just like any other document-oriented application.

## What Changes

- Rename the "Open WAV File" button to **"Open"** and style it blue — this button opens an existing WAV file as the active document.
- Style the **"Record"** button red — recording creates a new WAV file and sets it as the active document.
- Add a **"Reprocess"** button — replays the active WAV file through the processing pipeline (re-runs transcription and visualization).
- Display the active document filename in the app header next to the title: `VTX Engine Demo: <filename>` (or just `VTX Engine Demo` when no document is open).
- Introduce a concept of **active document state**: the currently open WAV file path, its generated filename (for new recordings), and whether the document is dirty/playing.
- When recording stops, the generated filename (e.g. `vtx-20260307-143022.wav`) becomes the active document and is reflected in the title.
- When a WAV file is opened, its filename is reflected in the title.
- Reprocess is only enabled when a document is open and not currently recording.

## Capabilities

### New Capabilities

- `demo-document-model`: Active document state management in the demo app — tracks the current WAV file path, updates the header title, and enables/disables the Reprocess button based on document state.
- `demo-reprocess`: Reprocess command in the demo app — replays the active WAV file through the transcription and visualization pipeline.

### Modified Capabilities

- `demo-ui-controls`: The Open and Record buttons change label/color; a Reprocess button is added. (UI-level changes to the demo app; no spec-level behavior change to the engine itself.)

## Impact

- `apps/vtx-demo/index.html` — button labels, new Reprocess button, title element update
- `apps/vtx-demo/src/main.ts` — active document state, title update logic, Reprocess handler, updated Open and Record handlers
- `apps/vtx-demo/src/styles.css` — blue Open button, red Record button, Reprocess button styling
- `apps/vtx-demo/src-tauri/src/lib.rs` — new `reprocess_file` Tauri command (replays a WAV file path through the existing `transcribe_audio_file` pipeline)
- No changes to the `vtx-engine` library crate or any existing specs
