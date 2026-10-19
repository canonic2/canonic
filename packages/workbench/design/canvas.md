# Artboard canvas

The canvas keeps each artboard's runtime attached while selection, dimensions,
and the shared camera change. Click an artboard's label to select it.

```example empty
```

```example single
```

Pending artboards keep their supplied name while their renderer starts.

```example loading
```

```example multiple
```

Standalone Docs uses a reading surface at 100%, filling the canvas without
artboard labels, selection borders, shadows or resize rails.

```example reading
```

## Public API

`wb-canvas` takes a `state: CanvasState` property and named slots matching the
artboard IDs. The application supplies and owns the iframe in each slot.
Setting state preserves existing nodes and emits no selection command.
An artboard's optional `target.label` supplies its human-readable name before
`view` is available. Rendered views supply their current `view.label`.
Ready views with `contentSize` use a transparent shell; the application owns
measurement and dimension updates.
`reading` is a boolean property reflected by the `reading` attribute. The host
sets it for a standalone docs page and clears it for preview artboards. Reading
mode keeps the layout width tied to the canvas, centers horizontal zoom, and
changes the viewport height to show more or less of the page. Its background
stays white. `scale` reads the current camera scale.

`fitAll()`, `fitSelected()`, `recenter()` (preserving zoom), `zoomTo(scale)`, `wheelZoom(delta, point)`, and
`panReady(boolean)` control the camera.

`wb-artboard-select` emits `{ id }`; `wb-camera-change` emits `{ scale }`.
Both bubble and cross the shadow boundary.
`wb-artboard-resize` emits `{ id, width, height }` from the selected artboard's
edge rails when it supports arbitrary dimensions; the application validates and
applies the dimensions. It also bubbles and crosses the shadow boundary.
`wb-artboard-deselect` asks the host to clear the selected annotation when a
gesture begins on the empty canvas; it bubbles and crosses the shadow boundary.
`wb-reading-resize` emits `{ id, width, height }` when the reading viewport
changes. The host applies those dimensions to the content renderer and state.
`wb-reading-scroll` emits `{ dy }` for page scrolling, or `{ from, to, pointY }`
to keep the page position beneath a zoom anchor. The host forwards it to the
docs renderer. Both events bubble and cross the shadow boundary.
The `artboard` part exposes each shell. Semantic `--wb-*` theme tokens set
surfaces, text, borders, selection and focus.

Labels are native buttons with pressed state. Space and drag pans, wheel pans,
Command/Ctrl-wheel zooms, Shift 1 fits all, Shift 2 fits the selection, and
Command/Ctrl 0 shows 100%. The application bridges iframe input. Disconnecting
releases input listeners and the resize observer; reconnecting reacquires them.
