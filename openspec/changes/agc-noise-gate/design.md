## Context

The AGC subsystem (`processor.rs:1252–1400`) uses a feed-back RMS envelope follower to maintain consistent output levels. It has a hard-coded noise floor at `1e-10` power (effectively digital silence) below which gain is held. Real-world background noise (fans, room tone, mic self-noise) has power well above this floor — typically in the `-50 to -65 dBFS` range — so the AGC treats it as signal and amplifies it toward the `-18 dBFS` target during speech pauses.

The existing `max_gain_db = 30` cap limits the worst case, but 30 dB of gain applied to room noise at `-55 dBFS` still brings it to `-25 dBFS`, which is clearly audible. The `release_time_ms = 200` slows the ramp-up but does not prevent it.

The processing chain is: `mono mix → mic gain → AGC → VAD → visualization → segment buffer`. The AGC operates in-place on mono f32 samples at 16 kHz on a dedicated OS thread with no allocations per chunk.

## Goals / Non-Goals

**Goals:**
- Prevent the AGC from amplifying background noise during speech pauses.
- Add a configurable gate threshold that distinguishes noise-level signal from speech-level signal.
- Decay gain toward unity when in the noise region, so the AGC does not sit at high gain waiting to slam down when speech resumes.
- Preserve existing behavior when the gate is set very low (backwards-compatible).
- Keep the change minimal — modify the existing AGC processor rather than adding a new pipeline stage.

**Non-Goals:**
- Adaptive noise floor estimation (future enhancement — the static threshold can later become the fallback).
- A separate noise gate processor stage in the pipeline.
- Spectral noise suppression or frequency-domain processing.
- Changes to the VAD or any other processing stage.

## Decisions

### D1 — Gate Threshold as a Power-Domain Comparison

The gate threshold is specified by the user in dBFS (`gate_threshold_db`) and converted to a power value on config update: `gate_threshold_power = 10^(gate_threshold_db / 10)`. The comparison happens against `power_estimate`, which is already in mean-squared (power) domain. This avoids a `sqrt()` per chunk for the comparison.

**Alternative considered:** Comparing in the RMS (amplitude) domain. Rejected because `power_estimate` is natively in power domain, and the existing noise floor check also uses power. Staying in the same domain is simpler and avoids unnecessary computation.

### D2 — Gain Decay Toward Unity in the Gate Region

When `power_estimate` is between the digital noise floor (`1e-10`) and the gate threshold, the AGC decays `current_gain_linear` toward `1.0` using an exponential decay with a dedicated time constant (`gate_decay_time_ms`).

```
decay_alpha = exp(-chunk_duration_s / (gate_decay_time_ms / 1000.0))
current_gain_linear = decay_alpha * current_gain_linear + (1 - decay_alpha) * 1.0
```

This ensures:
- Gain does not remain stuck at a high value during extended pauses (which would cause a loud burst when speech resumes).
- The decay is smooth, avoiding audible discontinuities.
- Unity gain in the noise region means noise passes through at its natural level — not amplified, not gated.

**Alternative considered:** Hard hold (freeze gain at its current value). Rejected because if the AGC had already ramped gain up before the signal dropped into the noise region, the held gain would continue amplifying noise. Decaying to unity is safer.

**Alternative considered:** Decay toward zero (silence the output). Rejected because that would be a noise gate, which is a different concern. The goal is to prevent *amplification* of noise, not to suppress noise entirely.

### D3 — Default Gate Threshold: -50 dBFS

A default of `-50 dBFS` (power = `1e-5`) sits comfortably above typical room noise floors (`-55 to -65 dBFS`) but well below conversational speech levels (`-30 to -15 dBFS`). This provides a good out-of-the-box experience for most environments.

Users in noisier environments can raise the threshold; users with very quiet backgrounds can lower it.

### D4 — Default Gate Decay Time: 500 ms

A 500 ms decay time constant is slow enough to avoid audible gain changes during brief pauses between words (where the power may dip toward the gate threshold momentarily) but fast enough that gain settles to unity within 1-2 seconds of sustained silence.

### D5 — Single New Config Field

Only `gate_threshold_db` is exposed as a user-facing config field. The gate decay time (`500 ms`) is an internal constant, not user-configurable. This keeps the API surface minimal. If tuning proves necessary, it can be promoted to a config field later without breaking changes.

## Risks / Trade-offs

- **Threshold too high clips quiet speech** → Mitigated by the conservative default of `-50 dBFS`, which is well below whisper levels. The field is user-configurable if adjustment is needed.
- **Brief pauses trigger gate decay** → Mitigated by the slow 500 ms decay constant. A brief dip below the threshold during a word break causes negligible gain change before speech resumes and the AGC re-engages normally.
- **Interaction with release_time_ms** → When power drops below the gate threshold, the gate decay (500 ms) takes over from the release smoother. Since both are exponential decays but toward different targets (release raises gain toward target, gate decays toward unity), the gate effectively overrides the release in the noise region. This is the desired behavior.
- **Static threshold does not adapt** → Accepted trade-off for simplicity. An adaptive noise floor estimator can be layered on top in a future change, using `gate_threshold_db` as the fallback.
