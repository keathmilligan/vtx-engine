## Context

vtx-engine captures audio from platform backends (WASAPI/CoreAudio/PipeWire), processes it through a pipeline (mono conversion, gain, AGC), then routes it to VAD, visualization, and transcription subsystems. All output to consumers flows through a `tokio::sync::broadcast` channel carrying `EngineEvent` variants. Currently, no event variant carries raw audio samples — consumers can only observe transcription text, visualization data (downsampled waveform/spectrogram), and scalar metrics.

The audio loop maintains two copies of the mono-converted audio at each iteration: `raw_mono_samples` (the unmodified mono mixdown) and `processed_samples` (a clone that is mutated in-place by software mic gain and AGC). Both exist in the loop today — raw samples are accumulated for the raw WAV recording buffer, and processed samples feed VAD, visualization, and transcription.

Consumer apps like OmniRec record video and audio independently. OmniRec writes audio to a temporary WAV file during recording and runs a second FFmpeg mux pass after recording stops. This post-recording mux is proportional to recording length and creates a user-visible delay. Streaming audio through the engine's event channel would let consumers pipe audio directly into their encoding pipeline in real time.

Different consumers have different needs. An app muxing audio into a video recording wants the processed audio so the result matches what the user heard. An app performing its own audio analysis, archival recording, or custom processing wants the raw audio before engine processing is applied. Some apps may want both — for example, muxing processed audio into a live video while saving an unprocessed copy for later remastering.

The audio loop runs on a dedicated `std::thread` and processes chunks as they arrive from the platform backend (typically ~10ms intervals). Subsystem gating follows two patterns: build-time immutable bools (`vad_enabled`, `visualization_enabled`) copied by value into the thread closure, and runtime-toggleable `Arc<AtomicBool>` flags (`transcription_enabled`, `recording_active`). Events are sent via `let _ = sender.send(...)` (fire-and-forget).

## Goals / Non-Goals

**Goals:**
- Deliver processed mono f32 audio samples to subscribers through the existing broadcast event channel
- Deliver raw (pre-gain, pre-AGC) mono f32 audio samples as an independent opt-in stream
- Allow consumers to enable processed, raw, or both streams independently
- Provide sufficient timing metadata for consumers to maintain A/V synchronization
- Follow established engine patterns for feature gating and event delivery
- Keep both features opt-in so non-streaming consumers are unaffected

**Non-Goals:**
- Changing the audio format (sample rate, bit depth) for streaming — consumers receive the same 48kHz mono f32 PCM that the internal pipeline produces
- Providing a dedicated high-throughput channel separate from the broadcast channel — the broadcast channel is sufficient for this use case
- Buffering or flow control for slow consumers — the existing `Lagged` behavior applies
- Streaming pre-mono-conversion (multi-channel interleaved) audio — both streams deliver mono

## Decisions

### Decision 1: Use the existing broadcast channel for audio delivery

**Choice**: Emit audio samples as new `EngineEvent` variants on the existing `broadcast::Sender<EngineEvent>`.

**Rationale**: The broadcast channel is the established delivery mechanism for all engine output. Adding a separate channel would bifurcate the consumer API, require additional lifecycle management, and complicate the `subscribe()` contract. The broadcast channel already handles multiple subscribers, lagged receiver semantics, and event filtering.

**Alternative considered — dedicated `mpsc` channel**: Would provide backpressure and avoid `Lagged` drops, but would require a new subscription API, per-consumer channel management, and break the single-channel pattern that all consumers rely on. Rejected because audio data loss from occasional `Lagged` events is acceptable (similar to visualization data) and the opt-in gate limits channel pressure.

**Alternative considered — ring buffer with shared memory**: Would provide zero-copy access to audio data. Rejected as over-engineering; the copy cost of ~480 f32 samples per 10ms chunk (~1.9KB) is negligible, and ring buffers would require unsafe code and a fundamentally different consumer API.

### Decision 2: Two separate event variants for processed and raw audio

**Choice**: Add `EngineEvent::AudioData` for processed audio and `EngineEvent::RawAudioData` for raw audio, as distinct variants with the same struct shape.

**Rationale**: Separate variants let consumers pattern-match on exactly the stream they care about without inspecting a discriminator field. This follows the existing engine pattern where each logical data stream has its own event variant (e.g., `TranscriptionComplete` vs `TranscriptionSegment`). Both variants carry the same fields (`samples`, `sample_rate`, `sample_offset`) but represent different points in the processing pipeline.

**Alternative considered — single variant with a `kind: AudioStreamKind` field**: Would reduce enum size but force consumers to match-and-filter, adding boilerplate. Rejected because the enum discriminant cost is negligible and two variants is cleaner for the common case where a consumer wants only one stream.

### Decision 3: Two independent build-time immutable flags

**Choice**: Add `audio_streaming_enabled: bool` and `raw_audio_streaming_enabled: bool` to `AudioEngine`, set via `EngineBuilder::with_audio_streaming()` and `EngineBuilder::with_raw_audio_streaming()` respectively. Store as plain `bool` values copied into the audio loop thread closure, matching the `visualization_enabled` pattern. Each flag independently gates its corresponding event emission.

**Rationale**: Audio streaming is a pipeline configuration concern — consumers either need a given audio stream or they don't. Independent flags allow any combination: processed only, raw only, both, or neither. Runtime toggling would require `Arc<AtomicBool>` and add complexity for no clear use case. The `visualization_enabled` precedent demonstrates this pattern works well for high-frequency data emission.

**Alternative considered — runtime-toggleable via `Arc<AtomicBool>`**: Would allow consumers to start/stop streaming mid-session. Rejected because the primary use case (muxing audio into a video recording) requires streaming from the start of capture. If a future use case demands runtime toggling, the flags can be promoted to `Arc<AtomicBool>` without API breakage.

### Decision 4: Per-chunk timing via cumulative sample offset

**Choice**: Both `AudioData` and `RawAudioData` events carry:
- `samples: Vec<f32>` — mono audio samples (processed or raw respectively)
- `sample_rate: u32` — always 48000 during live capture
- `sample_offset: u64` — cumulative count of samples emitted for that stream since `CaptureStateChanged { capturing: true }`, starting at 0

**Rationale**: A cumulative sample offset provides sample-accurate timing without relying on wall-clock synchronization between independent systems. The consumer computes the timestamp of any chunk as `sample_offset / sample_rate` seconds from capture start. This is the same approach used in professional audio (sample-based timecodes) and avoids clock drift between the audio engine and the consumer's video timeline.

Both streams use the same `sample_offset` sequence independently — since both are derived from the same audio chunks in the same loop iteration, their offsets will be identical when both are enabled. A consumer using both streams can correlate them by matching `sample_offset` values.

The consumer's sync workflow:
1. Observe `CaptureStateChanged { capturing: true }` — this is T=0 for the audio timeline
2. Receive `AudioData` and/or `RawAudioData` events — each chunk's position is `sample_offset / sample_rate` seconds from T=0
3. In their own video pipeline, record the wall-clock time of the first video frame relative to T=0
4. Apply the delta as an A/V offset (identical to what OmniRec already does with `audio_delay_ms`)

**Alternative considered — wall-clock `Instant` per chunk**: Would simplify the consumer slightly but `std::time::Instant` is not serializable and wall-clock correlation between threads has microsecond-level jitter. Sample offsets are deterministic and jitter-free.

**Alternative considered — `Duration` since capture start**: Similar to sample offset but loses sub-sample precision and requires the engine to track a start `Instant`. Sample offset is more natural for audio processing and can be trivially converted to a duration by the consumer.

### Decision 5: Emit raw audio before processing, processed audio after processing

**Choice**: In the audio loop, emit `RawAudioData` immediately after mono conversion (before gain and AGC), and emit `AudioData` after all processing (after AGC, after VAD/visualization). Both emissions occur before the transcription state update.

**Rationale**: Each event should carry samples from the correct pipeline stage. Raw audio must be captured before any mutations to `processed_samples`. Processed audio should be captured after all processing is complete. The audio loop processing order becomes: mono conversion → **raw audio streaming** → gain → AGC → VAD → visualization → **processed audio streaming** → render output → transcription state.

### Decision 6: Both streams default to disabled

**Choice**: Both audio streaming options are disabled by default. Consumers must explicitly call `with_audio_streaming()` and/or `with_raw_audio_streaming()` to enable them.

**Rationale**: Audio data events are high-frequency (~100/sec) and relatively large compared to other events (~1.9KB per chunk vs bytes for metrics). Enabling both streams doubles the event rate to ~200/sec. Emitting either by default would waste broadcast channel capacity and increase `Lagged` risk for consumers that only need transcription or visualization. The opt-in pattern ensures only consumers that request audio data pay this cost.

## Risks / Trade-offs

**[Risk] Broadcast channel saturation from audio events** → At ~100 events/sec per stream (up to ~200 with both enabled) and 256 channel capacity, a consumer that stalls for ~1.3-2.5 seconds will receive `Lagged`. Mitigation: this is the same behavior that already exists for visualization data; consumers are expected to drain the channel promptly. The opt-in gates ensure only consumers that request audio streaming pay this cost. Documentation will note the latency sensitivity.

**[Risk] Memory overhead from cloning sample vectors** → Each audio chunk clones ~480 f32 samples (~1.9KB) into the broadcast channel per enabled stream, multiplied by the number of active subscribers. With both streams enabled, this doubles to ~3.8KB per chunk. Mitigation: at 100 chunks/sec with one subscriber and both streams, this is ~380KB/sec — still negligible. The `Vec<f32>` clone is cheaper than the `VisualizationData` clone that already happens at the same rate.

**[Risk] `sample_offset` overflow** → A `u64` counter at 48kHz overflows after ~12 million years of continuous capture. Not a practical concern.

**[Trade-off] Two event variants increases enum size** → Adding two variants rather than one increases the `EngineEvent` enum by one additional variant. The cost is negligible — the enum is already 14 variants, and the discriminant overhead is a single byte.

**[Trade-off] Build-time only enablement** → Cannot toggle audio streaming at runtime. Acceptable for the known use cases. Promoting to `Arc<AtomicBool>` later is backward-compatible.
