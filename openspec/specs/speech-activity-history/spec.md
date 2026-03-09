## Purpose

TBD — Specification for the speech activity history buffer and scroll interaction in the vtx-viz visualization package, enabling users to scroll back through historical speech activity data.

## Requirements

### Requirement: History buffer accumulates all incoming frames
The `SpeechActivityRenderer` SHALL maintain an append-only history buffer for all nine metric channels (amplitude, ZCR, centroid, speaking, lookback-speech, voiced-pending, whisper-pending, transient, word-break). Every frame pushed via `pushMetrics` SHALL be appended to the history buffer. The history buffer SHALL NOT overwrite existing frames; it grows monotonically until the configured cap is reached.

#### Scenario: Frames are preserved beyond the live window
- **WHEN** more than `bufferSize` (256) frames have been pushed
- **THEN** all frames remain accessible in the history buffer, not just the most recent `bufferSize`

#### Scenario: History buffer cap is enforced
- **WHEN** the total number of accumulated frames reaches `maxHistoryFrames` (default 108 000)
- **THEN** the oldest frames are discarded in a batch trim so that memory usage stays bounded

### Requirement: Scroll offset controls which historical slice is displayed
The renderer SHALL maintain a `scrollOffset` integer (in frames, minimum 0) that represents how many frames back from the live head the view is anchored. An offset of 0 means the live edge is visible (rightmost frame = most recently received frame).

#### Scenario: Default view shows live data
- **WHEN** `scrollOffset` is 0
- **THEN** the rightmost frame rendered is the most recently received frame

#### Scenario: Scroll offset shifts the visible window backward
- **WHEN** `scrollOffset` is N > 0
- **THEN** the rightmost frame rendered is the frame received N frames ago

#### Scenario: Scroll offset is clamped to available history
- **WHEN** the user attempts to scroll past the oldest available frame
- **THEN** `scrollOffset` is clamped to `max(0, totalFrames - bufferSize)`

### Requirement: Mouse wheel adjusts scroll offset
The renderer SHALL listen for `wheel` events on the canvas element. Scrolling left (positive deltaX or negative deltaY) SHALL increase `scrollOffset` (scrolling into history). Scrolling right (negative deltaX or positive deltaY) SHALL decrease `scrollOffset` toward 0.

#### Scenario: Wheel scroll left enters history
- **WHEN** the user scrolls left (or up) on the canvas
- **THEN** `scrollOffset` increases and the view moves into older history

#### Scenario: Wheel scroll right returns toward live
- **WHEN** the user scrolls right (or down) on the canvas while `scrollOffset > 0`
- **THEN** `scrollOffset` decreases; if it reaches 0 the live view is resumed

### Requirement: Pointer drag adjusts scroll offset
The renderer SHALL listen for `pointerdown`, `pointermove`, and `pointerup` events. Dragging left (decreasing clientX) SHALL increase `scrollOffset`; dragging right SHALL decrease it toward 0.

#### Scenario: Drag left enters history
- **WHEN** the user presses and drags left on the canvas
- **THEN** `scrollOffset` increases proportionally to the drag distance

#### Scenario: Drag right returns toward live
- **WHEN** the user drags right while `scrollOffset > 0`
- **THEN** `scrollOffset` decreases; if it reaches 0 the view resumes live follow

### Requirement: Segment markers are stored with absolute frame indices
Each segment marker SHALL record the value of the monotonically increasing `totalFrames` counter at the moment `markSegmentSubmitted()` is called. Markers SHALL remain valid and render at the correct horizontal position regardless of how many subsequent frames are pushed.

#### Scenario: Segment marker visible in scrolled-back view
- **WHEN** the user scrolls back to a time range that includes a previously submitted segment marker
- **THEN** the marker is rendered as a vertical dashed line at the correct x position within the visible window

#### Scenario: Segment marker not rendered outside visible window
- **WHEN** a segment marker's absolute frame index falls outside the currently visible window
- **THEN** no marker line is drawn for it

### Requirement: Live indicator shows current scroll state
The renderer SHALL draw an overlay on the canvas indicating whether the view is live or scrolled into history. When `scrollOffset === 0` it SHALL display a "LIVE" indicator. When `scrollOffset > 0` it SHALL display the approximate scroll depth in seconds.

#### Scenario: Live badge shown at live edge
- **WHEN** `scrollOffset` is 0
- **THEN** a "LIVE" text indicator is visible on the canvas

#### Scenario: Scroll depth shown when panned back
- **WHEN** `scrollOffset` is greater than 0
- **THEN** the indicator displays approximately how many seconds back the view is anchored, not a "LIVE" badge
