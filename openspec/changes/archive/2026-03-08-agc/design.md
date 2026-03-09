## Context

The engine applies software mic gain as a simple linear scalar in the capture loop (`lib.rs:656–663`). The gain is a fixed value set by the user via `mic_gain_db` in `EngineConfig` and hot-updated via `set_mic_gain`. There is no feedback mechanism — if input levels shift, the user must manually re-adjust.

The capture loop runs on a dedicated OS thread. Per-chunk processing must be cheap (no allocations, no blocking). Sample rate is normalised to 16 kHz before this stage; chunk sizes vary by platform backend but are typically 10–40 ms of audio. The existing VAD already computes a per-chunk RMS value which can inform gain decisions, but the AGC must be self-contained to avoid coupling concerns.

The `EngineEvent` broadcast channel (`tokio::sync::broadcast`) carries all engine-to-consumer notifications. The `AgcGainChanged` event will be emitted on the same channel.

## Goals / Non-Goals

**Goals:**
- Maintain a perceptually consistent output level (configurable target RMS, default −18 dBFS) regardless of microphone sensitivity or distance.
- Use a proven, deterministic algorithm with no external dependencies.
- Keep the AGC state machine on the capture thread with zero heap allocation per chunk.
- Support hot-update of all AGC parameters without restarting capture.
- Emit a periodic `AgcGainChanged(f32)` broadcast event so the UI can display current gain.
- Default to disabled; existing behaviour is completely unchanged when `AgcConfig::enabled = false`.

**Non-Goals:**
- Noise gating or silence suppression (handled by VAD).
- Loudness normalisation for transcription output (post-processing concern).
- Per-speaker or adaptive profiling.
- OS/driver-level gain control (the existing `set_gain` hook on `AudioBackend` remains a no-op).
- Lookahead (feed-forward from future samples) — not compatible with real-time streaming.

## Decisions

### D1 — Algorithm: Digital RMS Envelope Follower with Exponential Smoothing

**Chosen:** Feed-back RMS AGC with separate attack and release time constants applied to a running power (mean-squared) estimate.

The gain update rule per chunk:

```
power_estimate = α * power_estimate + (1 - α) * chunk_rms²
current_gain   = target_rms / sqrt(power_estimate).clamp(min_gain, max_gain)
```

Where `α` is derived from the time constant `τ` and chunk duration `Δt`:

```
α_attack  = exp(-Δt / τ_attack)   (fast — pulls gain down quickly on loud onset)
α_release = exp(-Δt / τ_release)  (slow — raises gain gradually after a loud burst)
```

Attack smoothing is applied when `chunk_rms² > power_estimate` (signal is getting louder); release smoothing is applied when it is getting quieter.

**Why RMS envelope follower over alternatives:**
- **Peak detector** — responds to transients rather than perceived loudness; causes AGC to hunt on plosives. Rejected.
- **Lookahead leveler** — requires a delay buffer; incompatible with the real-time, low-latency design goal. Rejected.
- **FFT-based loudness (ITU-R BS.1770)** — accurate but allocates and is computationally heavier than necessary for mic AGC; overkill for speech-only input. Rejected.
- **RMS envelope follower** — industry-standard approach used in broadcast AGC, telephony (WebRTC AGC1), and hearing aids. Proven, parameter-free at the algorithm level, O(n) per chunk, no allocation. **Selected.**

### D2 — Gain Limits

`AgcConfig` exposes `min_gain_db` (default −6 dB) and `max_gain_db` (default +30 dB). The floor prevents the AGC from amplifying silence/noise to target level when no one is speaking. The ceiling prevents extreme gain on a nearly-silent but persistent signal.

Gain is clamped after the envelope step, before application to samples.

### D3 — AGC Stage Position in the Pipeline

AGC is inserted **after** the existing `mic_gain_db` manual gain stage and **before** the VAD and visualization stages. Manual gain serves as a coarse, static trim; AGC provides fine, dynamic levelling. This matches the signal chain convention in professional audio hardware (trim → dynamics → processing).

```
raw PCM → mono mix → [manual gain] → [AGC] → [VAD] → [Visualization] → segment buffer
```

### D4 — Hot-Update Mechanism

AGC parameters are stored in an `Arc<Mutex<AgcConfig>>` shared between the capture thread and the public API. The capture thread reads config once per chunk under a `try_lock` (non-blocking); if the lock is contended it uses the previous config. This avoids stalling the audio thread on a hot-update from the UI thread.

An alternative was `AtomicU32` per field (like the existing `mic_gain_db`). Rejected because `AgcConfig` has five fields and packing them into atomics would be fragile and complex. The `try_lock` approach is simpler and the contention window (a `clone()`) is nanoseconds.

### D5 — `AgcGainChanged` Event Emission Rate

Emitting one event per chunk (~100 events/sec at 10 ms chunks) would flood the broadcast channel. The AGC processor will emit `AgcGainChanged` at most once per 100 ms (configurable internally, not user-exposed) by tracking elapsed chunks. The gain value carried is the instantaneous clamped gain in dB.

### D6 — `AgcProcessor` as a Standalone Struct in `processor.rs`

The AGC state machine (`AgcProcessor`) will live in `processor.rs` alongside `SpeechDetector` and `VisualizationProcessor`. This keeps all per-chunk DSP in one module and makes unit testing straightforward without involving the full engine.

### D7 — Default Parameter Values

| Parameter | Default | Rationale |
|---|---|---|
| `enabled` | `false` | Non-breaking — existing behaviour unchanged |
| `target_level_db` | −18.0 | Headroom for peaks while keeping speech intelligible |
| `attack_time_ms` | 10.0 | Fast enough to catch loud onsets before clipping |
| `release_time_ms` | 200.0 | Slow enough to avoid pumping between words |
| `min_gain_db` | −6.0 | Modest attenuation floor |
| `max_gain_db` | +30.0 | Practical limit; beyond this, noise dominates |

## Risks / Trade-offs

- **Pumping artefacts** → Mitigated by the asymmetric attack/release design and the slow release default (200 ms). Users can tune via `release_time_ms`.
- **Gain hunting during silence** → Mitigated by `max_gain_db` ceiling (gain cannot grow indefinitely) and by the fact that the VAD discards silent segments before transcription.
- **try_lock miss drops a hot-update for one chunk** → Acceptable; AGC parameters change on human timescales (100s of ms), not per-chunk timescales (10 ms).
- **`AgcGainChanged` event adds load to the broadcast channel** → Throttled to 10 Hz max; well within the 256-event channel capacity.
- **Interaction with manual gain** → Documented: `mic_gain_db` is a pre-AGC trim. If AGC is enabled, extreme manual gain values (+20 dB) will be compensated by the AGC. Users should set manual gain to 0 dB when AGC is active.

## Migration Plan

1. Add `AgcConfig` struct and `#[serde(default)]` embed in `EngineConfig` — existing TOML files without the key deserialise to disabled defaults.
2. Implement `AgcProcessor` in `processor.rs` with unit tests.
3. Insert AGC stage in the capture loop in `lib.rs`.
4. Add `AgcGainChanged(f32)` variant to `EngineEvent`.
5. Add `set_agc_config` / `agc_config` methods to `AudioEngine`.
6. Wire `AgcConfig` initialisation in `builder.rs`.
7. Update demo UI (`main.ts`) and Tauri command pass-through.
8. No rollback complexity — disabling AGC (`enabled = false`) restores original behaviour exactly.

## Open Questions

- None blocking implementation. Parameter defaults are based on standard telephony/broadcast practice and can be tuned post-implementation via the UI.
