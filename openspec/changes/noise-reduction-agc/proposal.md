# Noise Reduction for AGC

## Why

When using AGC (Automatic Gain Control) with dual input (microphone + system audio) and echo cancellation, white noise present at the beginning of voice segments gets amplified, resulting in burst or popping sounds. These audio artifacts interfere with transcription accuracy by introducing false transcriptions of noise as speech, particularly affecting the whisper.cpp transcription pipeline.

## What Changes

- **Enhanced noise gate for AGC**: Improve the existing noise gate in `AgcProcessor` to prevent gain from rising during initial noise at segment starts
- **Segment-aware gating**: Add look-ahead or hold-time mechanism to distinguish between sustained noise and actual speech onset
- **Adaptive noise floor estimation**: Dynamic noise floor tracking to better handle varying background noise levels
- **BREAKING**: May change default AGC behavior for very quiet speech at segment boundaries - will need tuning guidance

## Capabilities

### New Capabilities
- `noise-gate-enhancement`: Improved noise gating with segment boundary awareness to prevent noise burst amplification

### Modified Capabilities
- `agc-processor`: Enhanced gate logic with adaptive thresholds and segment-start protection

## Impact

**Affected code:**
- `crates/vtx-engine/src/processor.rs` - `AgcProcessor` implementation
- `crates/vtx-engine/src/lib.rs` - AGC configuration defaults
- `crates/vtx-engine/src/config.rs` - `AgcConfig` struct additions

**API changes:**
- New configuration parameters for enhanced noise gate (hold time, adaptive threshold)
- Default behavior may change for edge cases (near-threshold speech)

**Transcription integration:**
- Reduced false transcriptions from amplified noise
- Improved accuracy for segment starts

**Testing considerations:**
- Dual input scenarios (mic + system audio)
- Echo cancellation enabled/disabled
- Various noise floors (quiet office, noisy environment)
- Segment boundary cases
