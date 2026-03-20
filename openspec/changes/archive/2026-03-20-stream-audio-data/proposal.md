## Why

Consumer apps like OmniRec use vtx-engine for real-time transcription while independently capturing audio for recording. When a recording stops, OmniRec must run a second FFmpeg pass to mux its separately-captured audio WAV into the video MP4 — a process that scales linearly with recording length and creates a noticeable delay for the user. If vtx-engine could stream its audio data to consumers in real time, apps could mux audio into the video container as it is being recorded, eliminating the post-recording mux step entirely.

Different consumers have different needs for the audio stream. An app muxing audio into a video recording wants the processed audio (with gain and AGC applied) so the recording matches what the user heard. An app performing its own audio analysis, custom processing, or archival recording wants the raw audio before any engine processing is applied. Some apps may want both — for example, muxing processed audio into a video while simultaneously saving an unprocessed copy for later remastering.

## What Changes

- Add two new `EngineEvent` variants: `AudioData` for processed audio samples and `RawAudioData` for unprocessed audio samples, delivered to subscribers via the existing broadcast channel
- Include per-chunk timing metadata (sample count, sample rate, and a cumulative sample offset) so consumers can maintain A/V sync without relying on wall-clock correlation between independent capture sessions
- Add `EngineBuilder` options to independently enable processed audio streaming, raw audio streaming, or both (both disabled by default to avoid broadcast channel pressure for consumers that don't need audio data)
- Provide a session start timestamp when capture begins so consumers can compute the offset between their own video timeline and the audio stream for correct lip sync

## Capabilities

### New Capabilities
- `audio-data-streaming`: Opt-in delivery of processed and/or raw audio sample chunks and timing metadata through the engine's broadcast event channel during live capture. Consumers can independently enable processed audio (post-gain, post-AGC), raw audio (pre-gain, pre-AGC), or both.

### Modified Capabilities
- `broadcast-events`: New `AudioData` and `RawAudioData` event variants carrying audio samples and timing metadata
- `engine-builder`: New builder methods to enable processed and/or raw audio data streaming

## Impact

- **Public API**: Two new `EngineEvent` variants — `AudioData` (processed) and `RawAudioData` (raw) — as non-breaking additions to the enum, though consumers with exhaustive matches will need updating. New `EngineBuilder::with_audio_streaming()` and `EngineBuilder::with_raw_audio_streaming()` methods.
- **Audio loop**: The capture loop gains up to two conditional paths: one that clones processed mono samples into an `AudioData` event, and one that clones raw mono samples (before gain/AGC) into a `RawAudioData` event. Each path is gated by its own independent flag.
- **Broadcast channel**: Audio data events are high-frequency (~100 per second at 10ms chunk intervals). Enabling both streams doubles the event rate. Consumers that subscribe but don't drain quickly will hit `Lagged` errors. The opt-in gates ensure only consumers that request audio data pay this cost.
- **Consumer apps**: OmniRec (and similar apps) can subscribe to `AudioData` events for processed audio to pipe directly into their FFmpeg encoding process, or `RawAudioData` events for unprocessed audio, or both. Provided timestamps enable lip sync — eliminating the post-recording mux pass.
- **No breaking changes**: Existing consumers that don't enable either audio streaming option see no behavioral difference.
