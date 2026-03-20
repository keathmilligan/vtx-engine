## 1. Types and Event Variants

- [x] 1.1 Add `StreamingAudioData` struct to `common.rs` with fields `samples: Vec<f32>`, `sample_rate: u32`, `sample_offset: u64`. Derive `Debug`, `Clone`, `Serialize`, `Deserialize`.
- [x] 1.2 Add `AudioData(StreamingAudioData)` variant to the `EngineEvent` enum in `common.rs`
- [x] 1.3 Add `RawAudioData(StreamingAudioData)` variant to the `EngineEvent` enum in `common.rs`
- [x] 1.4 Export `StreamingAudioData` from `common.rs` (already covered by `pub use common::*` in `lib.rs`)

## 2. Builder and Engine Struct

- [x] 2.1 Add `audio_streaming_enabled: bool` and `raw_audio_streaming_enabled: bool` fields to `EngineBuilder` struct in `builder.rs`, defaulting to `false`
- [x] 2.2 Add `with_audio_streaming()` builder method in `builder.rs` that sets `audio_streaming_enabled = true` and returns `Self`
- [x] 2.3 Add `with_raw_audio_streaming()` builder method in `builder.rs` that sets `raw_audio_streaming_enabled = true` and returns `Self`
- [x] 2.4 Add `audio_streaming_enabled: bool` and `raw_audio_streaming_enabled: bool` fields to the `AudioEngine` struct in `lib.rs`
- [x] 2.5 Wire the two new flags from the builder into the `AudioEngine` struct in the `build()` method in `builder.rs` (matching the `visualization_enabled` pattern)

## 3. Audio Loop Integration

- [x] 3.1 Copy `audio_streaming_enabled` and `raw_audio_streaming_enabled` into the audio loop thread closure in `start_audio_loop()` in `lib.rs` (matching how `visualization_enabled` is copied)
- [x] 3.2 Add a `sample_offset: u64` counter variable initialized to 0 at the top of the audio loop thread, before the main loop
- [x] 3.3 Emit `EngineEvent::RawAudioData` immediately after mono conversion (after `raw_mono_samples` is created, before gain/AGC), gated by `raw_audio_streaming_enabled`. Use `raw_mono_samples.clone()` for the samples, the current `sample_rate`, and the current `sample_offset`.
- [x] 3.4 Emit `EngineEvent::AudioData` after AGC processing and after the visualization block, gated by `audio_streaming_enabled`. Use `processed_samples.clone()` for the samples, the current `sample_rate`, and the current `sample_offset`.
- [x] 3.5 Increment `sample_offset` by `raw_mono_samples.len() as u64` after both emission points (once per chunk, since both streams share the same offset sequence)
- [x] 3.6 Reset `sample_offset` to 0 at the start of the audio loop (already handled by initialization; verify it resets across capture sessions by confirming the audio loop thread is re-spawned on each `start_capture`)

## 4. Demo App Update

- [x] 4.1 Add match arms for `EngineEvent::AudioData` and `EngineEvent::RawAudioData` in the exhaustive match in `apps/vtx-demo/src-tauri/src/lib.rs`. Emit as Tauri events `"audio-data"` and `"raw-audio-data"` respectively (or use no-op arms with comments since the demo doesn't enable streaming).

## 5. Documentation

- [x] 5.1 Add a new section to `USAGE.md` covering audio data streaming: enabling via builder, receiving events, computing timestamps for A/V sync, and choosing between processed and raw streams
- [x] 5.2 Update the `README.md` features list to mention real-time audio data streaming

## 6. Verification

- [x] 6.1 Run `cargo build` for the workspace to verify the changes compile without errors
- [x] 6.2 Run `cargo clippy` to verify no new warnings
- [x] 6.3 Run `cargo test` to verify existing tests pass
- [x] 6.4 Run `cargo doc --no-deps` to verify documentation builds cleanly
