## 1. History Buffer Infrastructure

- [x] 1.1 Add `maxHistoryFrames` option to `SpeechActivityRendererOptions` (default 108 000) and thread it through the constructor
- [x] 1.2 Replace the nine ring-buffer typed arrays with nine append-only history arrays (same channel names, but length grows up to `maxHistoryFrames`)
- [x] 1.3 Add `totalFrames: number` counter (monotonically increasing) and initialize it to 0 in `clear()`
- [x] 1.4 Update `pushMetrics` (and `transferToRingBuffer` if still used) to append normalized frame values to the history arrays and increment `totalFrames`
- [x] 1.5 Implement front-trim logic: when `totalFrames` exceeds `maxHistoryFrames`, copy the tail `(maxHistoryFrames - bufferSize)` frames forward and decrement `totalFrames` accordingly (trim in chunks of `bufferSize` to amortize cost)
- [x] 1.6 Remove or repurpose the old `writeIndex` / `filled` ring-buffer fields (keep `bufferSize` as the visible window size)

## 2. Scroll Offset State & Public API

- [x] 2.1 Add `scrollOffset: number` field (initialized to 0 in constructor and `clear()`)
- [x] 2.2 Implement `scrollBy(deltaFrames: number)` — adjusts `scrollOffset` clamped to `[0, max(0, totalFrames - bufferSize)]`
- [x] 2.3 Implement `resetToLive()` — sets `scrollOffset` to 0
- [x] 2.4 Add `get isLive(): boolean` getter returning `scrollOffset === 0`

## 3. Canvas Interaction Handlers

- [x] 3.1 In the constructor, attach a `wheel` event listener on the canvas; map `deltaX` and `deltaY` to `scrollBy` calls (scale wheel pixels to frames using `framesPerPixel`)
- [x] 3.2 Attach `pointerdown`, `pointermove`, `pointerup` listeners for drag-to-scroll; track drag start X and accumulated delta; call `scrollBy` on each `pointermove`
- [x] 3.3 Call `event.preventDefault()` in wheel and pointer handlers to suppress browser scroll; set `touch-action: none` on the canvas element via the renderer or `styles.css`
- [x] 3.4 On `pointerup`, if `scrollOffset` has been dragged back to 0, call `resetToLive()`

## 4. Draw Path — History Slice

- [x] 4.1 Add a helper `getVisibleSlice()` that computes `startIndex = max(0, totalFrames - scrollOffset - bufferSize)` and returns the frame range `[startIndex, startIndex + bufferSize)` with left zero-padding if needed
- [x] 4.2 Refactor `draw()` to call `getVisibleSlice()` once per frame and pass the slice data to all sub-draw methods
- [x] 4.3 Update the speech bar draw (top 8%) to read from the slice's `speaking` and `lookbackSpeech` channels
- [x] 4.4 Update the word-break bar draw to read from the slice's `wordBreak` and `speaking` channels
- [x] 4.5 Update amplitude, ZCR, and centroid line drawing to read from the slice's respective float channels
- [x] 4.6 Update voiced-pending, whisper-pending, and transient dot drawing to read from the slice's boolean channels

## 5. Segment Markers — Absolute Frame Indices

- [x] 5.1 Change the `SegmentMarker` interface (or equivalent) to store `frameIndex: number` (absolute, from `totalFrames` at submission time) instead of a ring-buffer `writeIndex`
- [x] 5.2 Update `markSegmentSubmitted()` to record `this.totalFrames` as the marker's `frameIndex`
- [x] 5.3 Update `drawSegmentMarkers()` to compute each marker's canvas X using: `slotOffset = (totalFrames - scrollOffset - 1) - marker.frameIndex; x = area.x + (bufferSize - 1 - slotOffset) / (bufferSize - 1) * area.width`; skip markers where `slotOffset < 0` or `slotOffset >= bufferSize`

## 6. Dynamic X-Axis Labels

- [x] 6.1 Add `frameIntervalMs: number` property to the renderer (default 16); expose a setter or constructor option
- [x] 6.2 Replace the hardcoded x-axis label strings in `drawGrid()` with dynamically computed values: for each grid column `i`, compute `t = -(bufferSize - 1 - i + scrollOffset) * frameIntervalMs / 1000` and format as `${t.toFixed(1)}s`

## 7. Live / Scroll Indicator Overlay

- [x] 7.1 Add a `drawScrollIndicator()` method that draws a small pill in the top-right of the speech activity canvas
- [x] 7.2 When `scrollOffset === 0`, draw "● LIVE" in green
- [x] 7.3 When `scrollOffset > 0`, draw `"◀ -Xs"` where X is `(scrollOffset * frameIntervalMs / 1000).toFixed(1)`
- [x] 7.4 Call `drawScrollIndicator()` at the end of `draw()`, after all other layers

## 8. Verification

- [x] 8.1 Build the `vtx-viz` package (`pnpm build` or equivalent) and confirm no TypeScript errors
- [x] 8.2 Build the `vtx-demo` app and confirm no TypeScript errors
- [ ] 8.3 Manually verify: live view scrolls in real time; scroll left reveals history; scroll right returns to live; x-axis labels update correctly; segment markers appear at correct positions when scrolled back
