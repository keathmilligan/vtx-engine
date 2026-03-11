## 1. Raw-Path Resolution

- [x] 1.1 Add `resolve_raw_wav_path()` function to `audio.rs` that takes any WAV path and returns the raw variant (strips `-processed` suffix, checks file exists, falls back to original with warning)
- [x] 1.2 Call `resolve_raw_wav_path()` at the top of `play_file()` in `lib.rs` so the engine always reads from the raw WAV

## 2. Recording Callback Returns Raw Path

- [x] 2.1 Change `submit_recording()` in `transcribe_state.rs` so `on_recording_saved` fires with the raw WAV path instead of the processed path
- [x] 2.2 Verify `get_last_recording_path()` now returns the raw path, confirming `activeDocumentPath` points to raw after recording

## 3. WASAPI Render Output

- [x] 3.1 Add `start_render()` and `stop_render()` default methods to the `AudioBackend` trait in `platform/backend.rs`
- [x] 3.2 Implement WASAPI render thread in `wasapi.rs`: open default render endpoint, init shared-mode `IAudioClient`, get `IAudioRenderClient`, event-driven write loop receiving mono f32 via `mpsc` channel
- [x] 3.3 Handle mono-to-stereo expansion and sample rate conversion in the render thread (reuse `Resampler` pattern)
- [x] 3.4 Implement `start_render()` and `stop_render()` on `WasapiBackend` to manage the render thread lifecycle

## 4. Audio Loop Integration

- [x] 4.1 Add an `Option<mpsc::Sender<Vec<f32>>>` field (or similar) to the audio loop context for the render output channel
- [x] 4.2 In `play_file()`, call `start_render()` before spawning the feeder thread; store the sender so the audio loop can access it
- [x] 4.3 In the audio loop, after processing stages, send a clone of `processed_samples` to the render sender when playback is active
- [x] 4.4 Call `stop_render()` when playback ends (both natural completion and cancellation via `stop_playback()`)

## 5. Remove HTMLAudioElement Playback

- [x] 5.1 Remove `HTMLAudioElement` creation, `setSinkId`, and `playbackAudio.play()` from `startFilePlayback()` in `main.ts`
- [x] 5.2 Remove `stopAudioElement()` helper and `playbackAudio` variable from `main.ts`
- [x] 5.3 Remove the `reprocess_file` Tauri command alias from `src-tauri/src/lib.rs` (playback via `open_file` is the single path)

## 6. Verification

- [x] 6.1 Build the project (`cargo build`) and fix any compilation errors
- [x] 6.2 Run existing tests (`cargo test`) and fix any failures
