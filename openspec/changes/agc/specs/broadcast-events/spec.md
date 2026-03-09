## ADDED Requirements

### Requirement: EngineEvent includes an AgcGainChanged variant
`EngineEvent` SHALL include an `AgcGainChanged(f32)` variant. The `f32` value carries the current AGC gain in decibels at the time of emission. This event SHALL be emitted by the capture loop at most once per 100 milliseconds when AGC is enabled and active.

#### Scenario: AgcGainChanged is emitted during active AGC
- **WHEN** AGC is enabled and audio capture is running
- **THEN** `AgcGainChanged(gain_db)` events are emitted on the broadcast channel at most every 100 ms

#### Scenario: AgcGainChanged is not emitted when AGC is disabled
- **WHEN** `AgcConfig::enabled` is `false`
- **THEN** no `AgcGainChanged` events are emitted on the broadcast channel

#### Scenario: AgcGainChanged variant is reachable in a match expression
- **WHEN** a consumer writes `match event { EngineEvent::AgcGainChanged(db) => { ... }, _ => {} }`
- **THEN** the match arm compiles and receives events during active AGC capture
