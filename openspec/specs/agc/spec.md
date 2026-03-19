## Purpose

TBD — Specification for the Automatic Gain Control (AGC) subsystem in vtx-engine, which normalizes microphone input levels using an RMS envelope-follower algorithm.

## Requirements

### Requirement: AgcConfig is a serializable configuration struct
`AgcConfig` SHALL be a public Rust struct deriving `Debug`, `Clone`, `Serialize`, `Deserialize`, and `PartialEq`. It SHALL have the following fields, all with `#[serde(default)]`:

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | `bool` | `false` | Whether AGC is active |
| `target_level_db` | `f32` | `-18.0` | Target RMS output level in dBFS |
| `attack_time_ms` | `f32` | `10.0` | Gain reduction time constant in milliseconds |
| `release_time_ms` | `f32` | `200.0` | Gain increase time constant in milliseconds |
| `min_gain_db` | `f32` | `-6.0` | Minimum allowable AGC gain in dB |
| `max_gain_db` | `f32` | `30.0` | Maximum allowable AGC gain in dB |
| `gate_threshold_db` | `f32` | `-50.0` | Power level in dBFS below which the AGC decays gain toward unity instead of boosting |

#### Scenario: AgcConfig defaults to disabled
- **WHEN** `AgcConfig::default()` is called
- **THEN** `enabled` is `false` and all other fields match the documented defaults, including `gate_threshold_db` at `-50.0`

#### Scenario: AgcConfig round-trips through serde
- **WHEN** an `AgcConfig` with non-default values (including `gate_threshold_db`) is serialized to TOML and deserialized
- **THEN** the deserialized value equals the original

#### Scenario: Existing config without gate_threshold_db deserializes with default
- **WHEN** a TOML file contains an `[agc]` section without a `gate_threshold_db` key
- **THEN** `gate_threshold_db` defaults to `-50.0`
- **THEN** no error is returned

### Requirement: AgcConfig is embedded in EngineConfig
`EngineConfig` SHALL contain a field `pub agc: AgcConfig` annotated with `#[serde(default)]`. When absent from a TOML config file, it SHALL deserialize to `AgcConfig::default()` (AGC disabled).

#### Scenario: Existing config file without agc key loads with AGC disabled
- **WHEN** a TOML file without an `[agc]` section is loaded as `EngineConfig`
- **THEN** `config.agc.enabled` is `false`
- **THEN** no error is returned

#### Scenario: EngineConfig with agc section serializes and deserializes correctly
- **WHEN** an `EngineConfig` with `agc.enabled = true` and `agc.target_level_db = -20.0` is saved and reloaded
- **THEN** the reloaded config has `agc.enabled == true` and `agc.target_level_db == -20.0`

### Requirement: AgcProcessor implements the RMS envelope-follower algorithm
`AgcProcessor` SHALL be a public struct in `processor.rs` implementing a feed-back RMS AGC using exponential smoothing. On each call to `process(samples: &mut [f32], sample_rate: u32)` it SHALL:

1. Compute the chunk RMS power: `chunk_power = mean(s² for s in samples)`.
2. Select the smoothing coefficient based on direction:
   - If `chunk_power > power_estimate`: use `α_attack = exp(-chunk_duration_s / (attack_time_s))`
   - Otherwise: use `α_release = exp(-chunk_duration_s / (release_time_s))`
3. Update: `power_estimate = α * power_estimate + (1 - α) * chunk_power`.
4. Determine the gain behavior based on the power estimate:
   - If `power_estimate > gate_threshold_power`: compute gain normally as `gain = target_rms / sqrt(power_estimate)`, clamped to `[min_gain_linear, max_gain_linear]`.
   - If `power_estimate` is between the noise floor (`1e-10`) and `gate_threshold_power`: decay `current_gain_linear` toward `1.0` (unity) using an exponential decay with a fixed time constant of `500 ms`.
   - If `power_estimate <= 1e-10` (digital silence): hold the current gain unchanged.
5. Apply gain in-place: `s = (s * gain).clamp(-1.0, 1.0)` for each sample.
6. Store the current gain for observation via `current_gain_db() -> f32`.

The `gate_threshold_power` SHALL be derived from `AgcConfig::gate_threshold_db` as `10^(gate_threshold_db / 10)`.

#### Scenario: Unity gain on a signal already at target level
- **WHEN** `AgcProcessor` is fed a sine wave with RMS equal to the target RMS level for several chunks
- **THEN** the applied gain converges to approximately 1.0 (0 dB) within 500 ms

#### Scenario: Gain increases for quiet input
- **WHEN** `AgcProcessor` is fed a signal at -40 dBFS with a target of -18 dBFS
- **THEN** the output RMS level converges toward -18 dBFS within the release time window

#### Scenario: Gain decreases for loud input
- **WHEN** `AgcProcessor` is fed a signal at 0 dBFS with a target of -18 dBFS
- **THEN** the output RMS level falls toward -18 dBFS within the attack time window

#### Scenario: Gain is clamped to configured limits
- **WHEN** `AgcProcessor` is configured with `max_gain_db = 10.0` and receives a near-silent input
- **THEN** the applied gain never exceeds 10 dB

#### Scenario: Silence does not cause gain explosion
- **WHEN** `AgcProcessor` receives chunks of all-zero samples
- **THEN** `current_gain_db()` does not exceed `max_gain_db` and no NaN or infinity is produced

#### Scenario: Noise below gate threshold does not get amplified
- **WHEN** `AgcProcessor` is processing speech and the input transitions to low-level noise below `gate_threshold_db`
- **THEN** the AGC decays its gain toward unity (0 dB) instead of boosting the noise toward the target level
- **THEN** the noise passes through at approximately its natural level

#### Scenario: Gain decays smoothly during gate region
- **WHEN** `AgcProcessor` had been amplifying speech at +20 dB gain and the input drops to noise below the gate threshold
- **THEN** the gain decays from +20 dB toward 0 dB over approximately 1-2 seconds (500 ms time constant)
- **THEN** no abrupt gain changes or audible discontinuities occur

#### Scenario: Speech resumption after gate decay re-engages AGC normally
- **WHEN** the AGC gain has decayed toward unity during a noise period and speech resumes above the gate threshold
- **THEN** the AGC computes gain normally using the envelope follower
- **THEN** the transition from gate-decay to active AGC is smooth

### Requirement: AGC stage is applied in the capture loop after manual gain
The capture loop in `AudioEngine` SHALL apply the AGC stage on the mono samples **after** the `mic_gain_db` manual gain stage and **before** the VAD and visualization stages. The AGC stage SHALL be skipped entirely (no processing cost) when `AgcConfig::enabled` is `false`.

#### Scenario: AGC disabled — no effect on samples
- **WHEN** `AgcConfig::enabled` is `false`
- **THEN** the mono samples passed to the VAD are identical to those after the manual gain stage

#### Scenario: AGC enabled — samples are level-adjusted
- **WHEN** `AgcConfig::enabled` is `true` and input is consistently below the target level
- **THEN** the AGC processor amplifies the samples before they reach the VAD

### Requirement: AGC parameters can be hot-updated without restarting capture
`AudioEngine` SHALL expose:
- `pub fn set_agc_config(&self, config: AgcConfig)` — replaces the active AGC configuration immediately.
- `pub fn agc_config(&self) -> AgcConfig` — returns the current AGC configuration.

The capture loop SHALL pick up the new config within at most one audio chunk duration (~40 ms) without restarting.

#### Scenario: set_agc_config takes effect during active capture
- **WHEN** `set_agc_config(AgcConfig { enabled: true, .. })` is called while capture is running
- **THEN** subsequent audio chunks are processed with AGC enabled without stopping capture

#### Scenario: agc_config returns the most recently set config
- **WHEN** `set_agc_config(cfg)` is called and then `agc_config()` is called
- **THEN** the returned config equals `cfg`

### Requirement: set_engine_config applies AgcConfig immediately
When `AudioEngine::set_config` is called (e.g., from the Tauri `set_engine_config` command), the new `AgcConfig` embedded in the provided `EngineConfig` SHALL be applied immediately via `set_agc_config`, consistent with how `mic_gain_db` is applied immediately.

#### Scenario: set_config applies AGC changes without restart
- **WHEN** `engine.set_config(EngineConfig { agc: AgcConfig { enabled: true, .. }, .. })` is called during capture
- **THEN** AGC becomes active immediately without restarting the capture loop
