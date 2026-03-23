# AGC Processor Specification

## Requirements

### Requirement: AGC Processor SHALL support configurable gate hold time
The AGC Processor SHALL provide a configurable `gate_hold_time_ms` parameter that delays gain increase when transitioning from the gate region to above-threshold region.

#### Scenario: Gate hold time prevents noise burst at segment start
- **WHEN** signal power transitions from below gate threshold to above gate threshold
- **AND** the configured `gate_hold_time_ms` is greater than 0
- **THEN** the AGC SHALL maintain unity gain (1.0) for the duration of `gate_hold_time_ms`
- **AND** after the hold time expires, the AGC SHALL apply normal gain computation

#### Scenario: Zero hold time restores legacy behavior
- **WHEN** `gate_hold_time_ms` is set to 0
- **THEN** the AGC SHALL immediately apply gain when power rises above gate threshold
- **AND** the behavior SHALL be identical to the implementation before this enhancement

#### Scenario: Hold time resets on sustained silence
- **WHEN** signal power falls below gate threshold for more than 100ms
- **AND** then rises above gate threshold
- **THEN** the hold timer SHALL reset and begin counting from zero

### Requirement: AGC Processor SHALL track power region transitions
The AGC Processor SHALL maintain state to detect transitions between power regions (below noise floor, gate region, above threshold) to properly manage the hold time logic.

#### Scenario: Transition detection
- **WHEN** the smoothed power estimate changes from the gate region to above-threshold region
- **THEN** the AGC SHALL record the transition timestamp
- **AND** use this timestamp to compute elapsed hold time

#### Scenario: No hold during active speech
- **WHEN** signal power remains above gate threshold continuously
- **THEN** the AGC SHALL apply normal gain computation without hold delay
- **AND** gain changes SHALL follow standard attack/release time constants

### Requirement: AGC configuration SHALL include gate hold time parameter
The `AgcConfig` struct SHALL include a `gate_hold_time_ms` field with default value of 50.0 milliseconds.

#### Scenario: Configuration serialization
- **WHEN** `AgcConfig` is serialized to TOML or JSON
- **THEN** the `gate_hold_time_ms` field SHALL be included
- **AND** deserialization SHALL use the default value if the field is missing

#### Scenario: Hot-swap configuration update
- **WHEN** `update_config()` is called with a new `AgcConfig`
- **THEN** the new `gate_hold_time_ms` value SHALL take effect on the next `process()` call
- **AND** any in-progress hold timer SHALL be reset
