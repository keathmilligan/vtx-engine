# Noise Gate Enhancement Specification

## ADDED Requirements

### Requirement: Noise gate SHALL provide segment-aware protection
The noise gate enhancement SHALL prevent AGC gain amplification during the initial moments of potential speech segments, protecting against amplified noise bursts that interfere with transcription.

#### Scenario: Noise burst suppression at segment start
- **WHEN** a new potential speech segment begins with background noise above the gate threshold
- **AND** the noise gate hold time is active
- **THEN** the AGC SHALL maintain unity gain
- **AND** the background noise SHALL not be amplified
- **AND** transcription accuracy SHALL improve by avoiding false speech detection

#### Scenario: Fast recovery for legitimate speech
- **WHEN** actual speech begins after the hold time has expired
- **THEN** the AGC SHALL apply appropriate gain using standard attack time constant
- **AND** the speech SHALL be amplified to target level

### Requirement: Noise gate SHALL be compatible with dual input scenarios
The noise gate enhancement SHALL function correctly when using both microphone input and system audio input (dual input) with echo cancellation.

#### Scenario: Dual input with echo cancellation
- **WHEN** AGC is enabled with echo cancellation (AEC)
- **AND** both microphone and system audio inputs are active
- **THEN** the noise gate hold time SHALL apply to both input streams
- **AND** residual echo SHALL not trigger premature gain increase

#### Scenario: Echo cancellation state awareness
- **GIVEN** echo cancellation is processing and removing echo
- **WHEN** residual noise remains after echo removal
- **THEN** the noise gate SHALL still apply hold time protection
- **AND** prevent amplification of residual noise at segment boundaries

### Requirement: Noise gate SHALL provide environment-specific tuning
The noise gate enhancement SHALL be tunable for different acoustic environments through the `gate_hold_time_ms` configuration parameter.

#### Scenario: Quiet office environment
- **GIVEN** a quiet office with minimal background noise (-60 dB or lower)
- **WHEN** `gate_hold_time_ms` is set to 20ms
- **THEN** the noise gate SHALL provide minimal protection
- **AND** speech response time SHALL remain fast

#### Scenario: Noisy environment
- **GIVEN** a noisy environment with significant background noise (-40 to -50 dB)
- **WHEN** `gate_hold_time_ms` is set to 100ms
- **THEN** the noise gate SHALL provide strong protection against noise bursts
- **AND** the transcription SHALL have reduced false positives from amplified noise

#### Scenario: Default configuration
- **WHEN** no explicit `gate_hold_time_ms` is configured
- **THEN** the default value of 50ms SHALL be used
- **AND** this SHALL provide balanced protection for typical office environments
