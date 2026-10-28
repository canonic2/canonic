# Canvas rendering

This topic owns how the canvas turns artboards into pixels: which
representation each artboard shows, how the canvas is drawn, how the server's
render manager produces and caches stills and pictures, and when an artboard
runs its app. The [rendering flow](../system/rendering-flow.md) owns the
shell/content boundary; [multiple artboards](multiple-artboards.md) owns
artboard instances, review output and context; [interaction](interaction.md)
owns the camera and input routing; [performance](../system/performance.md)
owns budgets and measurement; the [server](../system/server.md) owns the
process that hosts the render manager.

**Status:** decided by the owner on 2026-10-27. The new canvas, which replaces
the current canvas entirely, implements this contract; it is not built yet.
See [implementation gaps](#implementation-gaps).

## Decisions

| Decision (2026-10-27) | Why |
| --- | --- |
| Each artboard shows one of four representations, chosen by the canvas: a placeholder, a picture, a still or its running app | Most artboards only need to look right; only the artboard someone uses needs its app ([evidence](../../research/artboard-swapper.md)) |
| Artboards that need their real document show a **still**: a static copy of the preview's document, taken once per preview and build in the background | A running app in every visible artboard, and in every background render, overloads the machine; a static document costs what plain inline HTML costs |
| Stills come from booting each preview in a background browser, never from server-side rendering by framework | Workbench does not control the project's source; only the browser renders every framework and everything a page does after it mounts |
| The Workbench server's **render manager** owns stills, pictures, their queue, priorities, cancellation, caches and refresh, for every canvas it serves | The renderers, build events and the persistent cache are server-side; one owner can hold the whole machine to one budget |
| The render manager is part of the Rust server, not WebAssembly in the canvas | Its own work is bookkeeping; the cost is in the browser that boots previews, and the canvas never waits on the manager inside a frame |
| A **render host** does the rendering: the process that controls a Chromium, running render pages in it. The render manager runs no browser of its own; it assigns jobs to render hosts | A fleet of headless browsers took a whole development machine; a host's own Chromium shares its browser, GPU and network processes, and an embedding host can hand over pixels without encoding a screenshot |
| The canvas sends **demand**, the full set of what it wants now; each demand replaces the last | Cancelling and subscribing follow from one source of truth; a cancel can never race the request it cancels |
| The **machine budget** follows the actual machine: cores, memory, measured cost per preview and current load | Fixed limits are wrong on both small and large machines |
| WebGPU draws the canvas: the grid and every picture, on one surface | One surface draws any number of artboards in a few calls and redraws only when something visible changes |
| Pictures are PNGs rendered by Chromium; vector replay of Chromium's paint records is not pursued | Pixel-exact output needs Chromium's own rasterization, and reading paint records needs a browser without its sandbox ([paint-record record](../../research/iframe-paint-record.md)) |
| Small pictures share a texture atlas | A zoomed-out canvas then takes a few draw calls instead of one per artboard |
| Canvas work is event-driven | An idle canvas does no drawing or scheduling work |
| The layer holding stills and running artboards is a composited layer while the camera moves, and stops being one once a zoom settles (decision 2026-10-28; see [the camera layer](#the-camera-layer)) | Without the layer, every zoom step repaints every iframe and zooming stutters; a permanent layer keeps the resolution it was made at, so stills stay blurry after zooming in |

## Representations

At any moment each artboard on screen shows exactly one representation:

1. **Placeholder** while nothing else of it exists.
2. **Picture**: pixels of the artboard at one resolution level, drawn by
   WebGPU. The best available level is shown: the smallest at or above the
   wanted level, otherwise the largest below it. A lower level is shown at
   once while the wanted one is produced. Once any picture of an artboard
   exists it is never blank.
3. **Still**: the artboard's document as static HTML and CSS in its own
   iframe, with no scripts. It is shown for the few artboards large enough on
   screen to read, nearest the centre (see [stills on the canvas](#stills-on-the-canvas)).
4. **Running**: the artboard's preview running in its own content renderer,
   for the artboard someone uses (see [running artboards](#running-artboards)).

Resolution levels are 0.125, 0.25, 0.5, 1, 2 and 4 device pixels per CSS
pixel. The wanted level is the smallest level at or above the camera scale
times the device pixel ratio, capped at 4.

## Drawing

- One WebGPU surface draws the grid, placeholders and pictures. Only artboards
  intersecting the viewport are drawn.
- 0.125× pictures (80 × 60 device pixels for a 640 × 480 artboard) live in
  4096 × 4096 atlas pages with a slot allocator and a free list. Each atlas
  page draws in one instanced call with per-artboard texture coordinates; all
  placeholders draw in one call. Larger levels are individual textures.
- Sampling never reads a neighbouring atlas slot (coordinates are clamped
  half a texel inside the slot), and minified pictures are filtered so they do
  not shimmer when the canvas is zoomed out below 0.125×.
- A frame is drawn only when something visible changed: the camera, a new or
  evicted picture, a representation swap or a setting.

## Render manager

The render manager runs in the Workbench server. One manager serves every
canvas connected to that server (editor windows, the sidebar, capture) and
owns the queue, the machine budget, the stills and pictures cache, and which
render host does each job. The canvas decides what it needs; the manager
decides how and when to produce it; a render host produces it.

### Render hosts

- A render host is a process that controls a Chromium: an embedding
  application that owns the Chromium its canvas runs in, or the editor
  extension's capture helper, an Electron process Workbench ships. It
  registers with the render manager over the previews stream, with how many
  jobs it takes at once, and returns results the same way.
- It runs each job in a **render page** of its Chromium: boot a preview and
  take its still, or render pictures from stills. Render pages are never shown
  to the person and are never throttled as hidden pages, so a boot runs at full
  speed; where the host can, they run at background priority, so they yield to
  the person's apps.
- An embedding host uses the Chromium it already runs (offscreen browsers that
  hand over their pixels), adding no browser process. The capture helper adds
  one Chromium beside the editor, the one it already is.
- The manager gives each job to one registered host, within the machine
  budget, and takes it back if the host leaves. With no host registered, the
  canvas shows the cached stills and pictures it receives and placeholders for
  the rest.

### Demand

- A canvas sends **demand**: every item it wants now. An item names an
  artboard (preview, scenario, size, inputs, lens, color scheme), a
  representation (`still` or `picture` at a level), a priority, and the
  **held key**: the key of the still or picture the canvas already has for
  it, so the manager sends only a different one.
- Each demand replaces the canvas's previous one. A large demand travels as a
  difference from the previous one, with a sequence number; results carry the
  number of the demand they answer, so the canvas drops stale ones.
- The manager compares a demand with the previous one from the same canvas:
  items no longer wanted lose that canvas's claim, items still wanted take the
  new priority, new items are looked up and queued.
- A job is cancelled only when no canvas's latest demand wants it. A queued
  job is dropped. A boot in progress is stopped, unless it is nearly finished;
  then it completes and its still is cached.
- Demand is also a subscription: an item in a canvas's latest demand receives
  its newer still or picture when a build changes it. Leaving the demand ends
  that.
- The canvas sends demand only when its inputs change: the work area, the
  wanted level, the still artboards, or the layout. Drawing never waits for
  the manager.
- Results arrive at the rate the canvas consumes them: the canvas acknowledges
  results, and the manager does not send faster than it acknowledges.

### Priority and the machine budget

- The canvas gives each item its priority, nearest the view's centre first
  within each tier: stills of the still artboards; then pictures for the
  **work area** (the view and a quarter of its size on each side), a 0.125×
  picture of an artboard with none before its wanted level; then, once the
  wanted level has held for 400 ms, pictures at that level for the
  **neighbour ring** (the artboards up to one view beyond the work area), so a
  pan finds them ready. Across canvases a job takes the highest priority any
  canvas gives it. The queue runs by priority, not arrival.
- The **machine budget** limits how many previews boot and how many pictures
  render at once, across every render host of the machine. It starts from the number of cores and the memory available,
  leaving room for the editor, the build and other work. It then uses each
  preview's measured cost (memory peak and CPU time of its boots), so a heavy
  page counts for more than a small one, and backs off while free memory or
  CPU is low. Running artboards in the canvas count against the same budget.
- When the queue is empty and the machine is quiet, the manager refreshes
  stale stills and pictures that some canvas still demands.

### Stills

- A render host takes a still by loading the preview in a render page,
  waiting until it is ready, fonts and images included, and serializing its
  document: the DOM with form values, every style rule the document uses
  (including rules added by script and adopted style sheets), shadow roots, and
  canvas elements as images. Scripts are removed. Images, fonts and other
  assets stay as URLs to the server, so the browser caches them.
- One boot produces one still; the render page then leaves the preview, so its
  memory is released.
- A still is keyed by everything that changes the document (its **still
  key**): the preview entry's build revision, scenario, artboard size, inputs,
  lens and color scheme.
- A failed preview produces an error still that shows the failure.

### Pictures

- Pictures are rendered from stills, never by booting the preview again.
  Small levels render several stills together as one sheet and are cut into
  pictures afterwards.
- A picture is keyed by its still's content hash and the level (its **picture
  key**). A rebuild that produces an identical still keeps every picture of it.

### Builds

- On a build, the manager compares each preview entry's revision. Stills of
  changed entries become stale, and so do their pictures.
- A stale still or picture stays in use until its replacement arrives; an
  artboard is never blanked by a rebuild. Replacements are produced in demand
  priority order.

## Caches

| Owner | Holds | Lifetime |
| --- | --- | --- |
| Render manager | Stills and pictures, by key, in memory and on disk | Survives restarts; shared by every canvas of the server; bounded on disk, oldest first out |
| Canvas | Picture textures (atlas pages and individual textures) and the stills in its iframes | The canvas's lifetime; GPU memory bounded, larger levels and off-screen pictures evicted first, 0.125× pictures kept |

A reopened canvas receives cached stills and pictures from the manager at once,
before any build has finished.

## Stills on the canvas

- When artboards are at least the **still threshold** (320 CSS pixels) wide
  on screen, the **still artboards**, up to the **still limit** (five) nearest
  the viewport centre, show stills. A still stays shown down to 85% of the
  threshold, so zooming near it does not flicker. The limit follows the
  machine budget.
- A still is loaded into its iframe as a sandboxed document without scripts,
  hidden over the artboard's picture until it has painted, then revealed.
  Swapping is never visible as a blank or a flash.
- Stills and running artboards are opaque, white where the document sets no
  background, as in a browser tab and a render page. The picture beneath never
  shows through: text in a picture is laid out at its level's device scale and
  can sit a few pixels from the same text in the still.
- CSS-only behavior such as hover styles works in a still; nothing else does.

### The camera layer

Stills and running artboards sit in one DOM layer, the **camera layer**, that
the camera moves with a CSS transform. Chromium needs two opposite things from that layer, so the
canvas switches it:

- **While the camera moves**, the layer is its own composited layer
  (`will-change: transform`). A pan or zoom then moves pixels already painted
  instead of repainting every iframe; without it, zooming over stills
  repaints them on every step and stutters.
- **Once a zoom has been still for 200 ms**, the canvas removes
  `will-change`. Chromium keeps a `will-change: transform` layer at the
  raster scale it was created at and never repaints it for a new scale, so
  without this step text in a still stays blurry after zooming in, however
  long the camera rests. Removing it makes Chromium repaint the layer sharp at
  the current scale.
- The next pan or zoom makes it a composited layer again, created at the
  current scale.

Both halves are required; removing either brings back stuttering zooms or
blurry stills. Screenshots taken through the DevTools protocol render a fresh
frame and do not show the blur, so it is checked on screen.

## Running artboards

- An artboard runs its app when someone uses it: the first click or key in
  its still turns it into a running artboard. That input is not replayed into
  the app.
- The running app is served by the Workbench server as for any preview: its
  document, module, chunks and hot updates. It is revealed over the still once
  it has painted.
- When a running artboard is left, the canvas takes a still of the running
  document (a **taken still**) and gives it to the manager, so what the person
  typed or opened stays visible on the canvas. The running app stays until the
  taken still is revealed over it.
- A few recently running artboards stay running but hidden, within the
  machine budget, so returning to them is immediate.
- Canvas navigation (Space + drag, ⌘/Ctrl + wheel, pinch and the zoom keys)
  works over stills and running artboards; other input belongs to the content.
  Input routing is owned by [interaction](interaction.md#focus-and-command-ownership).

## Acceptance criteria

- A zoomed-out canvas of 1,000 artboards takes a number of draw calls that
  depends on atlas pages, not on artboards, and an idle canvas runs no
  scheduling or drawing work.
- A canvas showing stills is as responsive as the same documents given inline
  to its iframes.
- Zooming over stills is as smooth as panning over them, and text in a still
  is sharp shortly after a zoom stops, at any zoom up to 800%.
- No more previews boot at once than the machine budget allows, whatever the
  number of canvases and artboards.
- Every visible artboard shows a placeholder, a picture, a still or its
  running app; an artboard with a picture is never blank; stills and running
  apps appear without a flash.
- Panning away cancels queued jobs no canvas still demands; a nearly finished
  job completes and is cached.
- Two canvases demanding the same still cause one boot.
- A build re-stills only previews whose entry changed, and a still identical to
  the previous one keeps its pictures.
- Reopening the canvas shows cached stills and pictures without booting a
  preview.
- A still matches its running preview's pixels, and pictures at 5%, 67% and
  800% zoom show no seams or bleeding between atlas slots.
- Navigation input over stills and running artboards moves the camera;
  clicks, typing and scrolling reach a running app.

The [artboard swapper record](../../research/artboard-swapper.md) holds the
measurements behind the drawing and atlas decisions.

## Implementation gaps

- **New canvas.** The new canvas is not built. It implements this contract
  together with the instance, review and context contracts in
  [multiple artboards](multiple-artboards.md) and the one-shell rule in the
  [rendering flow](../system/rendering-flow.md).
- **Render manager.** The Workbench server has no render manager.
- **Render hosts.** No product process is a render host yet. The capture
  helper renders screenshots but takes no render jobs, and no embedding
  application implements the contract.
- **Still fidelity.** Which page features a still cannot carry (script-driven
  fonts, video, WebGL, nested documents) is not established.
- **Automated tests.** No product test covers this contract.
- **Scale and layout.** Drawing is measured with up to 2,000 equally sized
  artboards in a grid. A separate prototype held about 60 frames per second
  with 100,000 freely placed artboards and one instanced draw
  ([100k record](../../research/free-artboards-wasm.md)); combining free
  placement with this contract is not established.

## Open questions

- How do selection and application state (Release and Restart) relate to
  running artboards and stills taken when they are left?
- Which representations do streams without a document use, such as the iOS
  Simulator and window streams?
- Should running artboards be served from one site separate from the canvas,
  so Chromium runs them in another process? The
  [live content record](../../research/live-content.md) found this removes
  canvas freezes while pages load.
- Should pictures travel as PNG, or as a format cheaper to decode and upload?
- How does review capture use stills, pictures and running artboards to
  produce full-resolution screenshots and handoffs?
