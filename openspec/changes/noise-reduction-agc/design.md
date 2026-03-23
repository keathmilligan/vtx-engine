# Design: Noise Reduction for AGC

## Context

The current `AgcProcessor` implements a feed-back RMS envelope follower with asymmetric attack/release time constants. It includes a basic noise gate that decays gain toward unity (1.0) when signal power falls between the noise floor (-100 dB power equivalent) and the gate threshold (-50 dB default).

**Problem**: When using AGC with dual input (microphone + system audio) and echo cancellation, the AGC can amplify white noise present at the beginning of voice segments before the VAD (Voice Activity Detection) has had time to distinguish speech from noise. This creates burst/popping sounds that interfere with transcription accuracy.

**Root cause**: The current gate logic only considers instantaneous power. When a new speech segment starts, if there's background noise at the beginning, the AGC sees low power (above gate threshold but below target) and applies high gain, amplifying the noise before VAD can suppress it.

## Goals / Non-Goals

**Goals:**
1. Prevent gain rise during the initial moments of a segment when noise is present
2. Add look-ahead or hold-time mechanism to allow VAD to properly classify the signal
3. Maintain fast attack for legitimate speech that follows the noise period
4. Provide configuration for tuning based on environment noise levels

**Non-Goals:**
1. Not implementing full spectral noise reduction (e.g., RNNoise) - scope is gate enhancement only
2. Not modifying the VAD itself - working within existing VAD timing
3. Not changing default AGC behavior for established speech (no impact on active speech segments)
4. Not implementing automatic noise floor estimation (manual configuration for now)

## Decisions

### Decision 1: Segment-Aware Gating with Hold Time

**Decision**: Add a configurable hold time (in milliseconds) during which gain cannot rise when transitioning from the gate region to above-threshold.

**Rationale**: 
- Simple to implement and understand
- Gives VAD time to confirm speech before AGC applies aggressive gain
- Configurable for different environments (quiet office vs. noisy environment)
- Can be disabled by setting hold time to 0

**Alternatives considered**:
- Look-ahead buffer: Requires storing future samples, adds latency and complexity
- Adaptive threshold based on noise history: More complex, risk of false positives
- Tighter coupling with VAD: Creates dependency, harder to test independently

### Decision 2: Per-Segment Reset of Hold Time

**Decision**: The hold time should be reset when transitioning from sustained silence (below gate threshold) to above-threshold region.

**Rationale**:
- Only affects the transition from noise/silence to potential speech
- Does not interfere with legitimate gain changes during active speech
- Works naturally with the existing power region detection

### Decision 3: Configuration in AgcConfig

**Decision**: Add `gate_hold_time_ms` parameter to `AgcConfig` with a reasonable default (50-100ms).

**Rationale**:
- Users can tune based on their environment
- Hot-swappable like other AGC parameters
- Follows existing configuration pattern
- Can be adjusted without code changes

## Risks / Trade-offs

**Risk**: Hold time may delay gain application for legitimate quiet speech starts
- **Mitigation**: Default should be conservative (50ms). Users in very quiet environments can reduce to 0. The trade-off is acceptable given the noise amplification problem is more severe than a slight delay in gain for quiet speech.

**Risk**: Different noise floors require different hold times
- **Mitigation**: Start with fixed default and gather feedback. Future iteration could add adaptive hold time based on measured noise floor.

**Risk**: Breaking change for users who rely on fast AGC attack on all signals
- **Mitigation**: Hold time default should be low enough (50ms) that impact is minimal. Document in changelog. Users can set to 0 to restore previous behavior.

**Trade-off**: Simpler implementation vs. adaptive intelligence
- **Chosen**: Fixed configurable hold time
- **Rationale**: Solves 80% of the problem with 20% of the complexity. Adaptive solutions require significant testing across environments.

## Migration Plan

**Deployment**:
1. Add `gate_hold_time_ms` parameter to `AgcConfig` with default 50.0
2. Update `AgcProcessor` to track hold time state
3. Implement hold time logic in the gain computation
4. Add unit tests for the new behavior
5. Update documentation

**Rollback**:
- Users can set `gate_hold_time_ms: 0.0` to restore previous behavior
- Code rollback involves reverting the single commit

**Testing strategy**:
1. Unit tests with synthetic noise followed by speech
2. Integration tests with dual input and echo cancellation
3. Manual testing with various background noise levels
4. A/B transcription accuracy comparison
