## 1. AgcConfig Changes

- [x] 1.1 Add `gate_threshold_db` field to `AgcConfig` in `lib.rs` with default `-50.0`, `#[serde(default)]`, and documentation comment
- [x] 1.2 Add the `default_agc_gate_threshold_db()` helper function and wire it into `Default` impl

## 2. AgcProcessor Changes

- [x] 2.1 Add a `gate_threshold_power` field to `AgcProcessor` derived from `config.gate_threshold_db` on construction and config update
- [x] 2.2 Add the `AGC_GATE_DECAY_TIME_MS` constant (`500.0`)
- [x] 2.3 Modify `AgcProcessor::process()` to implement the three-region gain logic: above gate threshold (normal), between noise floor and gate threshold (decay toward unity), at or below noise floor (hold)

## 3. Tests

- [x] 3.1 Add unit test: noise below gate threshold causes gain decay toward unity
- [x] 3.2 Add unit test: speech above gate threshold still gets normal AGC processing
- [x] 3.3 Add unit test: smooth transition from gate decay back to active AGC on speech resumption
- [x] 3.4 Add unit test: existing AGC behavior is unchanged when gate_threshold_db is very low
- [x] 3.5 Verify existing AGC tests still pass

## 4. Demo UI

- [x] 4.1 Add `gate_threshold_db` to the TypeScript `AgcConfig` interface in `main.ts`
- [x] 4.2 Add a gate threshold slider to the AGC section in `index.html`
- [x] 4.3 Wire the slider to read/write `gate_threshold_db` in the config sync logic in `main.ts`
