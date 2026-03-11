## Context

The vtx-engine audio architecture has a clean split between raw and processed audio established in the audio loop (`lib.rs:765-833`). During recording, visualization correctly shows processed audio (post-gain, post-AGC). Two problems exist during playback:

1. **Double processing**: `activeDocumentPath` points to the processed WAV (`-processed.wav`). When the user clicks Play, `play_file()` reads that already-processed file and feeds it through the full pipeline (gain → AGC → viz), applying processing a second time. Visualization shows inflated amplitude.

2. **Audio element bypass**: The browser `HTMLAudioElement` plays the file directly from disk, completely bypassing the engine pipeline. Audible output doesn't reflect current processing settings.

The `windows` crate (v0.58) is already a dependency with `Win32_Media_Audio`, which includes `IAudioRenderClient` for WASAPI audio output. The existing WASAPI capture code in `wasapi.rs` provides structural patterns (shared-mode init, event-driven loops, resampling) that can be adapted for the render path.

## Goals / Non-Goals

**Goals:**
- Playback always sources from the raw (unprocessed) WAV file
- Visualization during playback reflects current processing settings applied to the raw audio
- Audible playback output reflects the same processed audio that drives visualization
- The active document path points to the raw WAV so reprocessing always starts from the original

**Non-Goals:**
- Real-time audio output during live recording (microphone monitoring / sidetone)
- Output device selection UI (can be added later; uses the system default render endpoint)
- Cross-platform audio output (Linux PipeWire / macOS CoreAudio render paths are out of scope; this change is Windows-only, matching the current platform focus)
- Replacing the `HTMLAudioElement` for non-engine audio use cases

## Decisions

### Decision 1: Raw-path resolution in `play_file()`

`play_file()` will resolve any input path (raw or processed) to the raw WAV path before reading the file. This uses the existing `extract_recording_stem()` function to find the stem, then checks for the raw file (`<stem>.wav`) on disk.

**Alternatives considered:**
- Change `activeDocumentPath` only and trust callers to always pass raw paths → fragile; the Open File dialog can select either variant.
- Store raw + processed paths as a pair → over-engineering for a single-file-at-a-time model.

**Rationale:** Resolution at the point of use is defensive and handles all entry paths (Play button, Open dialog, drag-and-drop future).

### Decision 2: `on_recording_saved` fires with raw WAV path

After recording stops, the `on_recording_saved` callback fires with the raw WAV path instead of the processed path. This means `activeDocumentPath` always points to the unprocessed original.

**Rationale:** Ensures that clicking Play after a recording always reprocesses from raw, regardless of processing settings at record time.

### Decision 3: WASAPI render endpoint for processed audio output

A new audio output path is added to the engine. During file playback, processed audio chunks from the audio loop are written to a WASAPI shared-mode render endpoint (the system default output device).

**Architecture:**

```
Audio Loop Thread
    │
    ├── processed_samples
    │       │
    │       ├──► VisualizationProcessor (unchanged)
    │       ├──► SpeechDetector (unchanged)
    │       ├──► write_processed_buffer() (unchanged)
    │       └──► output_tx.send(processed_samples) ──► Render Thread
    │                                                       │
    │                                                       ▼
    │                                              WASAPI IAudioRenderClient
    │                                              (shared mode, event-driven)
    │                                                       │
    │                                                       ▼
    │                                                   Speakers
```

The render path is a dedicated thread that:
1. Opens the default render endpoint via `IMMDeviceEnumerator`
2. Initializes `IAudioClient` in shared mode with event callback
3. Gets `IAudioRenderClient`
4. Receives mono processed samples from the audio loop via an `mpsc` channel
5. Converts mono to stereo (duplicate channels) and resamples to match the device mix format
6. Writes frames to the render buffer on each event signal
7. Stops when the playback-active flag is cleared

**Alternatives considered:**
- Stream processed audio back to the frontend via Tauri events and play via Web Audio API → high latency (IPC + JS scheduling), poor sync with visualization, complex buffering.
- Use the `rodio` crate for cross-platform output → adds a dependency and another abstraction layer when we already have WASAPI infrastructure.
- Write processed audio to a temp file and play that via `HTMLAudioElement` → introduces a delay (must write before playing) and sync issues with visualization.

**Rationale:** WASAPI render is the natural counterpart to the existing WASAPI capture code. It runs in the same process at the same latency tier, uses the same COM infrastructure already initialized, and provides sample-accurate sync with visualization since both consume from the same `processed_samples` in the same audio loop iteration.

### Decision 4: Remove `HTMLAudioElement` playback path

The browser-side `HTMLAudioElement` creation in `startFilePlayback()` is removed entirely. The engine backend handles both processing and audible output. The frontend's role during playback becomes: invoke the engine command, update UI state, listen for events.

**Rationale:** Two independent audio paths playing in parallel is the root cause of the mismatch. A single path through the engine guarantees visualization and audio output are identical.

### Decision 5: AudioBackend trait extension

The `AudioBackend` trait gains optional render methods with default no-op implementations:

```rust
fn start_render(&self) -> Result<mpsc::Sender<Vec<f32>>, String> { ... }
fn stop_render(&self) -> Result<(), String> { ... }
```

`start_render()` returns a channel sender that the audio loop uses to push processed samples. The backend owns the render thread. Default impls return an error so non-Windows platforms gracefully degrade (playback works for visualization/transcription but without audible output).

**Rationale:** Keeps the cross-platform trait contract clean while allowing platform-specific render implementations to be added incrementally.

### Decision 6: Mono-to-device format conversion in the render thread

The audio loop produces mono f32 at 48kHz (processed samples). The render thread converts this to the device's native format:
- Mono → stereo: duplicate each sample to both channels
- Resample: reuse the existing `Resampler` struct if device rate ≠ 48kHz
- Float → int: convert if the device mix format is integer PCM

This keeps the audio loop simple (always mono f32) and isolates device-specific concerns in the render thread.

## Risks / Trade-offs

**[Risk] Device-specific format issues** → The render endpoint's mix format may vary. Mitigation: Use `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY` flags during `IAudioClient::Initialize` to let WASAPI handle format conversion, reducing our format negotiation burden. Fall back to manual conversion only if auto-convert is unavailable.

**[Risk] Render thread latency** → The channel between the audio loop and render thread adds buffering. Mitigation: Use a bounded `sync_channel` with a small capacity (2-4 chunks) to keep latency under 40-80ms, comparable to the `HTMLAudioElement` path.

**[Risk] No audible output on non-Windows platforms** → Linux and macOS don't get render output from this change. Mitigation: The architecture (trait method + channel) is designed for incremental platform support. Playback still works for visualization and transcription; only audible output is missing.

**[Trade-off] Output device selection** → This change uses the system default render device. Users who had configured a specific output device via `setSinkId()` on the `HTMLAudioElement` lose that capability. This can be restored later by adding device selection to the `start_render()` API.
