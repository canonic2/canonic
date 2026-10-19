# Artboard canvas

The canvas keeps each artboard's runtime attached while selection, dimensions,
and the shared camera change. Click an artboard's label to select it.

```example empty
```

```example single
```

```example multiple
```

## Public API

`wb-canvas` takes a `state: CanvasState` property and named slots matching the
artboard IDs. The application supplies and owns the iframe in each slot.
Setting state preserves existing nodes and emits no selection command.
Ready views with `contentSize` use a transparent shell; the application owns
measurement and dimension updates.
There are no reflected attributes. `scale` reads the current camera scale.

`fitAll()`, `fitSelected()`, `recenter()` (preserving zoom), `zoomTo(scale)`, `wheelZoom(delta, point)`, and
`panReady(boolean)` control the camera.

`wb-artboard-select` emits `{ id }`; `wb-camera-change` emits `{ scale }`.
Both bubble and cross the shadow boundary.
`wb-artboard-resize` emits `{ id, width, height }` from the selected artboard's
edge rails when it supports arbitrary dimensions; the application validates and
applies the dimensions. It also bubbles and crosses the shadow boundary.
`wb-artboard-deselect` asks the host to clear the selected annotation when a
gesture begins on the empty canvas; it bubbles and crosses the shadow boundary.
The `artboard` part exposes each shell. Semantic `--wb-*` theme tokens set
surfaces, text, borders, selection and focus.

Labels are native buttons with pressed state. Space and drag pans, wheel pans,
Command/Ctrl-wheel zooms, Shift 1 fits all, Shift 2 fits the selection, and
Command/Ctrl 0 shows 100%. The application bridges iframe input. Disconnecting
releases input listeners and the resize observer; reconnecting reacquires them.
