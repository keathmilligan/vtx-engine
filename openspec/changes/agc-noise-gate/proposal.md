## Why

When AGC is active, it amplifies background noise during speech pauses. The current noise floor threshold (`1e-10`) only prevents gain explosion on digital silence — real-world room noise sits orders of magnitude above this, so the AGC treats it as signal and boosts it toward the target level. This produces audibly noisy output between utterances.

## What Changes

- Add a configurable gate threshold to the AGC that distinguishes background noise from speech-level signal.
- When the smoothed power estimate falls below this gate threshold, the AGC decays its gain toward unity (1.0) instead of continuing to boost, preventing noise amplification during pauses.
- Add a `gate_threshold_db` field to `AgcConfig` with a sensible default so the improvement works out-of-the-box.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `agc`: Add a noise-aware gate threshold that prevents the AGC from amplifying background noise. New `gate_threshold_db` config field; modified gain computation behavior when power is below the gate threshold.

## Impact

- `AgcConfig` struct gains a new field (`gate_threshold_db`) — serialization-compatible via `#[serde(default)]`.
- `AgcProcessor::process()` gains a new code path for the gate region between the noise floor and the gate threshold.
- Existing behavior is preserved when the gate threshold is set to a very low value (effectively disabled).
- Demo UI may optionally expose the new parameter.
- No breaking changes to the public API.
