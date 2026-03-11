## Why

When playing back a previously recorded file, the engine pipeline reprocesses the audio through gain/AGC, but the source file is the *already-processed* WAV (`-processed.wav`). This double-processes the audio, causing visualizations to show inflated amplitude compared to what was seen during recording. Meanwhile, the browser `HTMLAudioElement` plays the already-processed file without current settings, so audible output doesn't reflect the user's current processing configuration either. The correct behavior is to always reprocess from the original raw recording and have both visualization and audible playback reflect the result.

## What Changes

- During playback, the engine pipeline will always source the **raw** (unprocessed) WAV file, not the processed variant. The raw file is reprocessed through the full pipeline (mic gain, AGC, VAD, visualization, transcription) with current settings.
- The browser-side `HTMLAudioElement` will be removed from the playback path. Instead, the engine backend will render processed audio to the system output device via a WASAPI render endpoint, ensuring audible playback matches exactly what the visualization and transcription see.
- The active document path (`activeDocumentPath`) after a recording will point to the **raw** WAV file so that subsequent Play operations source from the unprocessed original.
- The `open_file` / `play_file` pipeline will resolve the raw WAV path from any input path (raw or processed) before feeding audio into the engine loop.

## Capabilities

### New Capabilities
- `audio-output-playback`: Engine-level audio output rendering via WASAPI, routing processed pipeline audio to the system speaker/output device during file playback.

### Modified Capabilities
- `demo-reprocess`: Playback sources the raw WAV and processing is applied with current settings; audible output comes from the engine pipeline rather than a separate browser audio element.
- `demo-document-model`: The active document path points to the raw WAV file after recording, and raw-path resolution is applied when initiating playback from any WAV variant.

## Impact

- **Backend (vtx-engine crate)**: New WASAPI render output path in the audio loop; `play_file` gains raw-path resolution logic; audio loop conditionally writes processed audio to the output device during playback.
- **Frontend (vtx-demo)**: `HTMLAudioElement` playback path removed; `startFilePlayback` simplified to only invoke the engine pipeline; `stop_recording` returns the raw WAV path.
- **TranscribeState**: `on_recording_saved` callback fires with the raw WAV path instead of the processed path.
- **Existing recordings**: Both raw and processed WAV files remain on disk; old processed files become stale but are harmlessly overwritten on the next reprocess cycle.
