## Why

The speech activity visualization only retains ~4 seconds of live data in its ring buffer, discarding older frames as new ones arrive. Once speech activity scrolls off the left edge of the canvas there is no way to review it, making it impossible to inspect what happened earlier in a recording session.

## What Changes

- The `SpeechActivityRenderer` will maintain a large unbounded (or configurable cap) history buffer that accumulates every incoming frame, in addition to the existing ring buffer used for live rendering.
- A scroll offset will be added so the user can drag or scroll left to review older frames; the rightmost position (offset 0) is the live view.
- The canvas draw loop will read from the history buffer using the current scroll offset to determine which slice of frames to render.
- The x-axis time labels will be computed dynamically from the actual frame interval and scroll offset so they always reflect real elapsed time.
- Segment markers will be stored in the history buffer with their absolute frame index so they render correctly at any scroll position.
- Mouse wheel and pointer drag interactions will be wired up on the canvas to control the scroll offset.
- A visual indicator (e.g., a "LIVE" badge or a scroll position indicator) will show whether the view is at the live edge or scrolled into history.

## Capabilities

### New Capabilities

- `speech-activity-history`: Unbounded (or capped) history buffer for speech activity metrics, a scroll offset model, and interaction handlers (wheel + drag) to navigate through the recorded history. Renders the correct time slice at any scroll position with accurate dynamic x-axis labels.

### Modified Capabilities

- `speech-activity-visualization`: The existing renderer gains scroll/history state and its draw path is updated to read from a history slice rather than directly from the ring buffer at all times.

## Impact

- `packages/vtx-viz/src/renderers.ts` — primary change site: `SpeechActivityRenderer` class
- `apps/vtx-demo/src/main.ts` — canvas event listener wiring (wheel, pointer drag on the speech activity canvas)
- `apps/vtx-demo/index.html` — possible addition of a scroll indicator or LIVE badge element
- `apps/vtx-demo/src/styles.css` — minor styling for any new indicator elements
- No changes to the Rust engine, Tauri commands, or event schema
