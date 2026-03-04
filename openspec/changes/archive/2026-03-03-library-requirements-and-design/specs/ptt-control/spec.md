## ADDED Requirements

### Requirement: PushToTalkController is obtained from the engine
`AudioEngine` SHALL expose a `ptt_controller() -> PushToTalkController` method. `PushToTalkController` SHALL be `Clone + Send + 'static` so it can be moved to a hotkey listener thread or any async task. All clones of a controller SHALL share the same internal state via `Arc`.

#### Scenario: Controller obtained and moved to another thread
- **WHEN** `let ptt = engine.ptt_controller()` is called and `ptt` is moved into a `std::thread::spawn` closure
- **THEN** the controller operates correctly in the spawned thread without requiring a reference to `AudioEngine`

### Requirement: PushToTalkController accepts application-supplied press and release signals
`PushToTalkController` SHALL expose `press()` and `release()` methods. These methods represent a generic activation signal; the application is responsible for generating them (e.g., from a hotkey, a button, an IPC message). The controller SHALL NOT perform any hotkey detection itself.

#### Scenario: press() opens a PTT session
- **WHEN** `ptt.press()` is called while capture is active and PTT mode is enabled
- **THEN** the engine begins accumulating audio into a new speech segment
- **THEN** a `SpeechStarted` event is emitted on the broadcast channel

#### Scenario: release() closes and submits the PTT session
- **WHEN** `ptt.release()` is called after a prior `press()`
- **THEN** the accumulated audio segment is submitted to the transcription queue
- **THEN** a `SpeechEnded { duration_ms }` event is emitted on the broadcast channel

#### Scenario: press() while already pressed is a no-op
- **WHEN** `ptt.press()` is called twice without an intervening `release()`
- **THEN** the second call is silently ignored; only one segment is open

#### Scenario: release() without prior press is a no-op
- **WHEN** `ptt.release()` is called without a prior `press()`
- **THEN** the call is silently ignored and no events are emitted

### Requirement: PTT mode is enabled via EngineBuilder or EngineConfig
PTT mode SHALL be configured via `EngineBuilder::transcription_mode(TranscriptionMode)` or `EngineConfig::transcription_mode`. When `TranscriptionMode::PushToTalk` is set, VAD-based automatic segmentation SHALL be suppressed; segment boundaries are determined solely by `press()` / `release()` signals. When `TranscriptionMode::Automatic` is set (the default), the `PushToTalkController` exists but its `press()` / `release()` calls have no effect.

#### Scenario: PTT mode suppresses VAD segmentation
- **WHEN** `EngineBuilder::new().transcription_mode(TranscriptionMode::PushToTalk).build().await` is called and capture is active
- **THEN** speech detected by the VAD does NOT trigger automatic `SpeechStarted` / `SpeechEnded` events
- **THEN** only explicit `ptt.press()` / `ptt.release()` produce segment boundaries

#### Scenario: Automatic mode ignores PTT signals
- **WHEN** `EngineBuilder::new().build().await` (Automatic mode) is used and `ptt.press()` is called
- **THEN** no effect on segmentation; VAD continues to control segment boundaries

### Requirement: set_active() provides a single-method PTT signal interface
`PushToTalkController` SHALL expose `set_active(active: bool)` as an alternative to `press()` / `release()`. `set_active(true)` SHALL be equivalent to `press()` and `set_active(false)` SHALL be equivalent to `release()`.

#### Scenario: set_active toggles PTT state
- **WHEN** `ptt.set_active(true)` is called followed by `ptt.set_active(false)`
- **THEN** behavior is identical to calling `ptt.press()` then `ptt.release()`

### Requirement: TranscriptionMode and HotkeyCombination/KeyCode are in vtx-common
`vtx-common` SHALL export `TranscriptionMode` (`Automatic`, `PushToTalk`), `HotkeyCombination`, and `KeyCode` types. These types SHALL derive `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`, `Eq`, and `Hash`. `HotkeyCombination` SHALL implement order-independent equality and hashing. `KeyCode` SHALL cover the full set of keys from the FlowSTT `KeyCode` enum.

#### Scenario: HotkeyCombination equality is order-independent
- **WHEN** two `HotkeyCombination` values are constructed with the same keys in different order
- **THEN** they compare as equal and produce the same hash

#### Scenario: KeyCode serializes to snake_case string
- **WHEN** `KeyCode::RightShift` is serialized to JSON
- **THEN** the result is `"right_shift"`
