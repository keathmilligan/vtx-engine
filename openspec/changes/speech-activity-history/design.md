## Context

`SpeechActivityRenderer` uses nine parallel typed-array ring buffers of 256 frames each (≈4.1s at 16ms/frame). All frames that age out of the ring buffer are permanently lost. The draw path always reads the most-recent `bufferSize` frames, offset from `writeIndex`. There is no scroll state and no way to view older data. Segment markers also rely on the same ring-buffer index, so they vanish when their slot is overwritten.

The renderer lives entirely in `packages/vtx-viz/src/renderers.ts`. Interaction wiring (wheel, pointer) would be added in `apps/vtx-demo/src/main.ts` where the canvas element is accessible, or alternatively inside the renderer itself as it already receives the canvas reference at construction time.

## Goals / Non-Goals

**Goals:**
- Accumulate every incoming frame indefinitely (or up to a configurable max, e.g. 30 minutes × 60fps = 108 000 frames ≈ ~4 MB for 9 typed arrays).
- Support a scroll offset that lets the user inspect any historical slice.
- Draw the correct time slice at any offset — all nine channels, speech bar, word-break bar, metrics lines, dots, and segment markers.
- Compute x-axis time labels dynamically from the actual frame interval and current scroll offset.
- Show a "LIVE" indicator when the view is pinned to the live edge; suppress it (or show scroll depth) when panned back.
- Allow mouse wheel and click-drag to adjust the scroll offset on the speech activity canvas.
- Auto-advance the view while live (offset = 0 means "follow the head"); panning backward freezes the view relative to the history anchor; releasing back to the right edge resumes live follow.

**Non-Goals:**
- Persisting history across page reloads or between recording sessions.
- Zooming the time axis.
- Changing the waveform, spectrogram, or mini-waveform renderers.
- Backend or Rust-side changes.

## Decisions

### Decision 1 — Single flat history buffer (append-only `Float32Array` / `Uint8Array` segments)

**Chosen:** Maintain nine parallel typed arrays that grow by appending. When a configurable cap (`maxHistoryFrames`, default 108 000) is reached, the oldest `bufferSize` frames are dropped by copying the tail down — equivalent to a slow ring buffer at macro scale.

**Alternatives considered:**
- *Keep the ring buffer as primary, add a separate resizable history.* Rejected: two buffers diverge in state, adds complexity, doubles memory writes.
- *Use JavaScript `Array` of objects.* Rejected: much higher memory and GC pressure compared to typed arrays.
- *Chunked deque of fixed typed arrays.* More complex; the append+trim approach is simpler and fast enough at the cap rates involved.

### Decision 2 — Scroll state lives inside the renderer

**Chosen:** `SpeechActivityRenderer` owns a `scrollOffset: number` (frames from the live head, 0 = live) and exposes `scrollBy(delta: number)` and `resetToLive()` public methods. `main.ts` attaches `wheel` and `pointerdown/pointermove/pointerup` event listeners on the canvas and calls these methods.

**Alternatives considered:**
- *Wiring interaction entirely in `main.ts` with scroll offset passed in each `draw` call.* Would require `main.ts` to know renderer internals; renderer already owns the canvas.
- *Renderer internally attaches its own canvas event listeners.* Simpler and more self-contained; chosen as primary approach so the renderer is a self-contained widget.

### Decision 3 — Draw path reads a contiguous slice from history buffer

**Chosen:** On each draw, compute `headIndex = totalFrames - 1` and `startIndex = headIndex - scrollOffset - (bufferSize - 1)`. Read `bufferSize` consecutive frames from the history arrays. If `startIndex < 0`, pad left with zeros. This is a straight indexed read — no modular arithmetic needed because the history buffer is not circular (it's append-only with occasional front-trim).

**Alternatives considered:**
- *Re-use the ring buffer draw logic by copying the slice into the ring buffer.* Wastes a copy on every frame; unnecessary.

### Decision 4 — Segment markers stored as absolute frame indices

**Chosen:** `segmentMarkers` becomes `Array<{ frameIndex: number }>` where `frameIndex` is the value of `totalFrames` at the moment `markSegmentSubmitted()` is called. On draw, a marker's canvas X is computed as `slotOffset = headIndex - scrollOffset - marker.frameIndex; x = area.x + (bufferSize - 1 - slotOffset) / (bufferSize - 1) * area.width`. Markers outside the visible window are skipped.

**Previous:** Markers stored the `writeIndex` of the ring buffer, which became invalid once overwritten. Absolute `frameIndex` survives indefinitely.

### Decision 5 — X-axis labels computed from frame interval and scroll offset

**Chosen:** Store `frameIntervalMs` (default 16) as a renderer property. On draw, compute each label's time as `t = -(bufferSize - 1 - col + scrollOffset) * frameIntervalMs / 1000`. Format with one decimal place. This replaces the six hardcoded strings.

### Decision 6 — "LIVE" indicator is a canvas overlay, not a DOM element

**Chosen:** Draw a small pill ("● LIVE") in the top-right corner of the speech activity canvas when `scrollOffset === 0`. When scrolled back, show a small "◀ scrolled Xs back" text instead. Keeps the renderer self-contained.

**Alternatives considered:**
- *DOM badge in `index.html` toggled by a CSS class.* Would work but requires `main.ts` to observe scroll state and update DOM — more coupling.

## Risks / Trade-offs

- **Memory growth at high frame rates.** At 60fps, 30 min = 108 000 frames × 9 arrays × ~1–4 bytes each ≈ 3–4 MB. Acceptable. The `maxHistoryFrames` cap limits the worst case. → *Mitigation: enforce cap with a periodic trim in `pushMetrics`.*
- **Front-trim cost.** Copying a 108 000-frame typed array every time the cap is hit is O(n). With a cap of 108 000 and a trim chunk of 1 × bufferSize (256), this happens every 256 pushes at maximum history. Each copy is ~108 000 × 9 = ~1 M element moves. At 60fps that's every ~4 seconds — acceptable as a one-time burst. → *Mitigation: trim in larger chunks (e.g., 10 × bufferSize) to amortize; schedule trim outside the draw path if needed.*
- **Scroll offset desync if history is trimmed.** When the front is trimmed, all absolute `frameIndex` values remain valid because they reference `totalFrames`, which is a monotonically increasing counter separate from the storage indices. → *No mitigation needed; the design is self-consistent.*
- **Pointer drag on a canvas may conflict with text selection or other default browser behaviors.** → *Mitigation: call `event.preventDefault()` in pointer handlers; set `touch-action: none` on the canvas via CSS.*

## Migration Plan

1. Add `maxHistoryFrames` option to `SpeechActivityRenderer` constructor (backward-compatible, defaults to 108 000).
2. Replace ring buffer fields with history buffer fields; update `pushMetrics`, `transferToRingBuffer` (remove), `draw`, `drawGrid`, `drawSegmentMarkers`, `markSegmentSubmitted`, and `clear`.
3. Add `scrollBy`, `resetToLive`, and `isLive` to the public API.
4. Attach canvas event listeners inside the renderer constructor (wheel + pointer).
5. Update `main.ts` to pass `frameIntervalMs` if/when it is known from config (for now, use the 16ms default).
6. No rollback required; `clear()` already resets state between sessions.

## Open Questions

- Should scroll be capped so the user cannot scroll past the oldest available frame, or silently clamp? → Clamp at oldest available frame.
- Should releasing the scroll (scrolling back to offset 0) snap automatically, or require an explicit click? → Scrolling to the right edge (offset 0) resumes live automatically; no explicit button needed for now.
