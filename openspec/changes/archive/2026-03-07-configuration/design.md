## Context

The demo app (`apps/vtx-demo`) is a Tauri v2 desktop application with a vanilla TypeScript frontend. The engine (`crates/vtx-engine`) exposes `EngineConfig` — a serde-serializable struct with all tunable parameters — but the demo has no runtime UI for changing them. The frontend persists a small `AppSettings` blob in `localStorage`; the Rust backend builds a single `AudioEngine` instance at startup via `EngineBuilder` and currently offers no command to read or write its config after initialization.

Audio gain is not present in `EngineConfig` today. The `AudioBackend` trait (`platform/backend.rs`) has `set_aec_enabled` and `set_recording_mode` as hot-settable methods; gain follows the same pattern. Audio *output* in the demo is handled entirely by a browser `<audio>` element — the engine has no playback path — so output device selection requires only a browser-side `setSinkId` call, not any Rust changes.

## Goals / Non-Goals

**Goals:**
- A gear icon button in the status bar (right of `#model-name`) that opens a modal config panel
- The panel exposes all tunable `EngineConfig` fields grouped by concern
- Mic input gain (`mic_gain_db`) is added to `EngineConfig` and applied in the capture pipeline via a new `AudioBackend::set_gain()` method
- Audio output device selection via `HTMLMediaElement.setSinkId()` on the demo's `<audio>` element
- New Tauri commands `get_engine_config` / `set_engine_config` give the frontend read/write access to the live config
- Config changes persist to `localStorage` (extended `AppSettings`) and are re-applied on the next capture start
- Reset-to-defaults restores factory values without requiring a restart

**Non-Goals:**
- Live hot-reload of config mid-capture (changes take effect on next `start_capture`)
- Exposing `SpeechDetector` internal parameters (ZCR/centroid ranges, hold times) — these are not part of `EngineConfig`
- Output routing through the engine itself — playback stays in the browser `<audio>` element
- Any changes to `WhisperModel` or `TranscriptionProfile` selection (separate concern from runtime tuning)

## Decisions

### 1. Config round-trip via `get_engine_config` / `set_engine_config` commands

**Decision**: Add two Tauri commands that serialize/deserialize `EngineConfig` as JSON.

**Rationale**: The frontend needs a canonical source of truth for current config values so the panel can be pre-populated accurately on open. Writing back through a dedicated command keeps the Rust side in control of validation and default-filling. The alternative — storing config state exclusively in `localStorage` — would drift from the engine's actual defaults over time and requires duplicating default values in TypeScript.

**Alternative considered**: A single `update_engine_config` command accepting a partial patch object. Rejected because serde's partial deserialization from JSON into an existing struct is awkward in Rust; a full round-trip (get → mutate → set) is simpler and the payload is small.

### 2. Mic gain as software gain in the capture pipeline

**Decision**: Add `mic_gain_db: f32` (default 0.0) to `EngineConfig` and apply it as a software linear multiplier on PCM samples in `AudioEngine`'s capture loop, not via OS/driver API.

**Rationale**: Hardware gain APIs differ significantly across WASAPI, CoreAudio, and PipeWire — some require elevated permissions, some are per-endpoint vs. per-session, and reliable cross-platform behavior is not guaranteed. A software gain stage applied to the raw PCM buffer before the VAD/transcription pipeline is simpler, portable, and sufficient for the demo's purpose. `AudioBackend::set_gain()` is still added to the trait so future implementors can opt into hardware gain if desired, but the demo backend implementations delegate to the software path.

**Alternative considered**: OS-level gain via `AudioBackend::set_gain()` implemented natively per platform. Rejected for this change due to complexity and cross-platform inconsistency; it can be revisited as a follow-on.

### 3. Config changes take effect on next `start_capture`

**Decision**: `set_engine_config` updates the engine's stored config but does not interrupt a running capture session. The UI shows a note when capture is active that changes will apply on next start.

**Rationale**: Rebuilding the capture pipeline mid-stream risks audio glitches and is architecturally complex. For a developer demo tool, "apply on next start" is an acceptable trade-off. This matches the current behavior of AEC toggle (already only applied at capture start).

**Alternative considered**: Restart capture automatically when config is saved. Rejected — too disruptive for users actively capturing.

### 4. Output device via browser `setSinkId`

**Decision**: Use `HTMLMediaElement.setSinkId(deviceId)` on the existing `<audio>` element in the frontend. Enumerate output devices with `navigator.mediaDevices.enumerateDevices()` filtered to `audiooutput`. Persist the selected `deviceId` in `AppSettings`.

**Rationale**: The engine has no playback path; the audio element is a browser API and `setSinkId` is the standard way to route it. No Rust or Tauri changes are needed. The only constraint is that `setSinkId` requires the `speaker-selection` permissions policy, which Tauri's WebView grants by default on all platforms.

**Alternative considered**: Exposing output device via a Tauri command that calls a native playback API. Rejected — over-engineered for what is purely a browser-side concern.

### 5. Config panel as a modal dialog (not a dedicated route)

**Decision**: Implement the config panel as an in-page modal overlay (`<div role="dialog">`) with a backdrop, styled consistent with the existing UI aesthetic. No routing library is introduced.

**Rationale**: The demo has no router and uses plain DOM manipulation. A modal is consistent with this approach and keeps the implementation self-contained in `main.ts` / `index.html` / `styles.css`.

### 6. Settings grouped into logical sections

The panel is organized into four sections:
- **Audio Input** — mic gain slider
- **Voice Detection** — VAD thresholds (voiced dB, whisper dB, voiced onset ms, whisper onset ms)
- **Segmentation** — segment max duration, word-break grace period, lookback ms, word-break segmentation toggle, transcription queue capacity
- **Visualization** — viz frame interval ms
- **Audio Output** — output device selector (browser-side only)

## Risks / Trade-offs

- **Software gain clipping**: A high `mic_gain_db` value applied to already-loud input can cause PCM clipping. Mitigation: clamp output samples to `[-1.0, 1.0]` in the gain stage; display range in the UI as -20 dB to +20 dB.
- **`setSinkId` browser support**: Available in Chromium-based WebViews (Tauri uses WebView2 on Windows, WebKit on macOS/Linux). macOS WebKit does not support `setSinkId`. Mitigation: detect capability and hide the output device selector on unsupported platforms with an explanatory note.
- **Config divergence during capture**: If the user edits config while capturing, the displayed values don't reflect what the running engine is using. Mitigation: show a visible warning banner in the panel when capture is active.
- **`AppSettings` schema migration**: The `localStorage` blob is extended with new fields. Mitigation: `loadSettings` already merges saved values with `defaultSettings()` via object spread, so missing fields default correctly — no migration needed.

## Open Questions

- Should `mic_gain_db` have a wider range (e.g., -40 to +40 dB) for power users, or stay conservative (-20 to +20 dB) to prevent obvious misuse? Defaulting to conservative; can be widened based on feedback.
- Should the gear icon use an SVG inline or a Unicode character (⚙)? Inline SVG is preferred for crispness at all DPI levels but adds HTML verbosity. Decide at implementation time.
