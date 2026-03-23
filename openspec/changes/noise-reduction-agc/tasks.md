# Implementation Tasks: Noise Reduction for AGC

## 1. Configuration Updates

- [x] 1.1 Add `gate_hold_time_ms` field to `AgcConfig` struct in `lib.rs`
- [x] 1.2 Add default value function `default_agc_gate_hold_time_ms()` returning 50.0
- [x] 1.3 Update `AgcConfig` Default implementation to include `gate_hold_time_ms`
- [x] 1.4 Update serialization/deserialization to handle new field

## 2. AgcProcessor State Management

- [x] 2.1 Add `hold_timer_ms` field to `AgcProcessor` struct to track elapsed hold time
- [x] 2.2 Add `last_power_region` field to track power region transitions
- [x] 2.3 Add `PowerRegion` enum for power region classification
- [x] 2.4 Add `gate_hold_time_ms` cached value from config
- [x] 2.5 Update `AgcProcessor::new()` to initialize new state fields
- [x] 2.6 Update `AgcProcessor::update_config()` to refresh cached hold time value

## 3. Core Hold Time Logic Implementation

- [x] 3.1 Implement power region detection (below noise floor / gate region / above threshold)
- [x] 3.2 Implement transition detection from gate region to above-threshold
- [x] 3.3 Add hold timer accumulation based on chunk duration
- [x] 3.4 Modify gain computation to check hold timer before applying gain
- [x] 3.5 Reset hold timer when power falls below gate threshold for sustained period

## 4. Integration and Testing

- [x] 4.1 Add unit test for gate hold time preventing noise burst (`agc_gate_hold_time_prevents_noise_burst`)
- [x] 4.2 Add unit test for zero hold time legacy behavior (`agc_zero_hold_time_legacy_behavior`)
- [x] 4.3 Update existing AGC tests to work with new functionality
- [x] 4.4 Fix test configuration in config_persistence.rs
- [x] 4.5 Run existing AGC tests to ensure no regression (14/14 pass)
- [x] 4.6 Run full test suite (54/54 pass)

## 5. Documentation and Validation

- [x] 5.1 Update `AgcConfig` documentation comments for new parameter
- [x] 5.2 Add inline comments explaining hold time logic in `process()` method
- [x] 5.3 Update example configurations in test files
- [x] 5.4 Verify hot-swap configuration update works correctly (tested via `update_config`)

## Summary

Successfully implemented noise reduction enhancement for AGC with configurable gate hold time. The feature prevents white noise at the beginning of speech segments from being amplified, which was causing burst/popping sounds that interfered with transcription.

**Key changes:**
- Added `gate_hold_time_ms` parameter to `AgcConfig` (default: 50ms)
- Added hold timer state tracking in `AgcProcessor`
- Implemented power region detection and transition logic
- Gate hold time delays gain increase when transitioning from gate region to above-threshold
- Fully backward compatible - set to 0ms to restore legacy behavior

**Test coverage:**
- New tests verify hold time prevents noise burst
- New tests verify zero hold time restores legacy behavior
- All 14 AGC tests pass
- Full test suite: 54 unit tests + 3 doc tests pass
