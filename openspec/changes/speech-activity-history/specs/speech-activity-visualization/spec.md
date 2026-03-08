## MODIFIED Requirements

### Requirement: Draw path reads from history slice at current scroll offset
The `SpeechActivityRenderer` draw path SHALL read a contiguous window of `bufferSize` frames from the history buffer, starting at `max(0, totalFrames - scrollOffset - bufferSize)`. If fewer than `bufferSize` frames exist before the window start, the left portion SHALL be padded with zero values. All rendered layers (speech bar, word-break bar, amplitude line, ZCR line, centroid line, voiced-pending dots, whisper-pending dots, transient dots, segment markers, grid) SHALL use this same slice.

#### Scenario: Live view renders the most recent frames
- **WHEN** `scrollOffset` is 0 and at least `bufferSize` frames have been received
- **THEN** the draw path uses the most recent `bufferSize` frames, identical to the previous ring-buffer behavior

#### Scenario: Scrolled view renders the correct historical slice
- **WHEN** `scrollOffset` is N > 0
- **THEN** the draw path uses frames `[totalFrames - N - bufferSize, totalFrames - N - 1]`

#### Scenario: Partial history is zero-padded on the left
- **WHEN** fewer than `bufferSize` frames have been received
- **THEN** the left portion of the canvas renders as empty (zero amplitude, no speech indicators)

### Requirement: X-axis time labels reflect actual frame interval and scroll offset
The x-axis grid labels SHALL be computed dynamically. Each label's time value SHALL be calculated as `t = -(bufferSize - 1 - colIndex + scrollOffset) * frameIntervalMs / 1000` seconds, where `colIndex` is the grid column index and `frameIntervalMs` is the configured frame interval (default 16 ms). Labels SHALL be formatted to one decimal place with an "s" suffix (e.g., "-4.1s").

#### Scenario: Live view labels match elapsed time
- **WHEN** `scrollOffset` is 0 and `frameIntervalMs` is 16
- **THEN** the rightmost x-axis label reads "0.0s" and the leftmost reflects approximately the buffer duration in seconds

#### Scenario: Scrolled view labels shift to reflect scroll depth
- **WHEN** `scrollOffset` is N frames
- **THEN** all x-axis labels shift left by `N * frameIntervalMs / 1000` seconds compared to the live view
