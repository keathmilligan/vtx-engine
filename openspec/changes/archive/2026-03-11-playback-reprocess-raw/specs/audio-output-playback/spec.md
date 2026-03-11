## ADDED Requirements

### Requirement: AudioBackend trait exposes render methods
The `AudioBackend` trait SHALL expose `start_render(&self) -> Result<mpsc::Sender<Vec<f32>>, String>` and `stop_render(&self) -> Result<(), String>` methods with default implementations that return `Err` (unsupported). Platform backends that support audio output SHALL override these methods.

#### Scenario: Default render methods return error
- **WHEN** `start_render()` is called on a backend that does not override the default
- **THEN** it returns `Err` with a message indicating render is not supported on this platform

#### Scenario: Windows backend supports render
- **WHEN** `start_render()` is called on the WASAPI backend
- **THEN** it returns `Ok(sender)` where `sender` is an `mpsc::Sender<Vec<f32>>` for pushing mono f32 samples at 48kHz

### Requirement: WASAPI render endpoint for processed audio output
The WASAPI backend SHALL open the system default render endpoint in shared mode when `start_render()` is called. A dedicated render thread SHALL receive mono f32 samples at 48kHz via the returned channel sender, convert them to the device's native format (stereo expansion, resampling, sample type conversion), and write them to the `IAudioRenderClient` buffer.

#### Scenario: Render thread writes to default output device
- **WHEN** `start_render()` is called and mono f32 samples are sent through the returned sender
- **THEN** the audio is audible through the system default output device

#### Scenario: Render thread handles mono to stereo conversion
- **WHEN** mono samples are sent to the render channel
- **THEN** the render thread duplicates each sample to both left and right channels before writing to the device buffer

#### Scenario: Render thread resamples when device rate differs
- **WHEN** the default render device mix format sample rate is not 48kHz
- **THEN** the render thread resamples from 48kHz to the device rate before writing

#### Scenario: Render stops cleanly
- **WHEN** `stop_render()` is called
- **THEN** the render thread drains any remaining buffered audio, releases the WASAPI resources, and exits

### Requirement: Audio loop sends processed samples to render during playback
During file playback (when `playback_active` is true), the audio loop SHALL send a copy of `processed_samples` to the render channel after all processing stages (mic gain, AGC) have been applied. The same samples that drive visualization and transcription SHALL be the samples rendered to the output device.

#### Scenario: Visualization and audible output match during playback
- **WHEN** a file is being played back through the engine pipeline with AGC enabled
- **THEN** the waveform visualization amplitude and the audible output both reflect the AGC-processed signal

#### Scenario: No render output when not playing back
- **WHEN** live capture is active (no file playback)
- **THEN** no samples are sent to the render channel

### Requirement: Render lifecycle tied to playback
The engine SHALL call `start_render()` at the beginning of `play_file()` and `stop_render()` when playback ends (either naturally or via `stop_playback()`). The render endpoint SHALL not remain open between playback sessions.

#### Scenario: Render starts with playback
- **WHEN** `play_file()` is called
- **THEN** `start_render()` is called before the feeder thread begins injecting audio

#### Scenario: Render stops when playback ends naturally
- **WHEN** the feeder thread finishes sending all chunks
- **THEN** `stop_render()` is called after the playback-complete event

#### Scenario: Render stops when playback is cancelled
- **WHEN** `stop_playback()` is called during active playback
- **THEN** `stop_render()` is called and the render thread exits promptly
