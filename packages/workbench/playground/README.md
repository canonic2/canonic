# Canvas playground

A development tool for trying canvas ideas in Chrome. It is never packaged.

The permanent part is the shell:
- Workbench's own dot grid (`ui/src/modules/canvas/grid.js`) and camera
  geometry (`ui/src/modules/canvas/geometry.js`), with Workbench's canvas
  colour tokens;
- a HUD with frame statistics, a scenario list and an inspector.

Each experiment is a **scenario**: one module the shell loads, swaps and
hot-reloads without reloading the page.

## Run

From `packages/workbench/`:

```sh
node playground/run.ts
```

This serves the playground on a free loopback port and opens it in an owned
Chrome window with a temporary profile. Closing that window, or pressing
Ctrl-C, stops the browser and the server and removes their temporary files.

| Option | Effect |
| --- | --- |
| `--scenarios <dir>` | Also load `*.scenario.js` files from `<dir>` (repeatable). Use it for local, uncommitted experiments. |
| `--port <n>` | Listen on a fixed port. |
| `--no-open` | Serve only; open the printed address in any Chromium browser. Host actions are then unavailable. |
| `--verbose` | Log each background render's phases (set content, ready, screenshot) and size. |
| `--renderers <n>` | Background headless Chromes rendering in parallel (default: CPU count / 4, between 1 and 4). |
| `--paint-record` | Launch Chrome with `--enable-gpu-benchmarking --no-sandbox`, so scenarios can read Chrome's paint records. Run only trusted local scenarios with it. |

## Controls

| Input | Action |
| --- | --- |
| Drag on empty canvas, middle drag, or Space + drag | Pan |
| Wheel / trackpad scroll | Pan |
| ⌘/Ctrl + wheel, pinch | Zoom toward the pointer |
| ⌘/Ctrl + `=` / `-` / `0` | Zoom in / out / 100% |
| Shift + `1` | Fit the scenario |
| `1`–`9`, `[` / `]` | Open a scenario by position / previous / next |
| `R` / Shift + `R` | Reload the scenario / reload the page |
| Tab | Show or hide the side panels |

Space + drag, ⌘/Ctrl + wheel, pinch and the ⌘/Ctrl zoom keys also work over
scenario iframes whose document has this server's origin (`srcdoc` and pages
it serves): the shell listens in their windows and forwards those inputs to the
canvas, while plain clicks, typing and scrolling stay with the iframe's content.
Space typed into a field inside an iframe stays a space. Cross-origin iframes
cannot be reached this way.

The shell remembers the last scenario and each scenario's camera. Saving a
file in a scenario's folder reloads that scenario and keeps the camera.

## Scenarios

A scenario is a file named `<name>.scenario.js` whose default export has a
`title`, an optional `description` and a `setup(ctx)` function. `setup` may be
async. It returns optional hooks:

```js
export default {
  title: 'Spinning square',
  description: 'Draws one square on the overlay.',
  setup(ctx) {
    let angle = 0;
    ctx.controls.toggle('Spin', false, on => ctx.animate(on));
    return {
      bounds: () => [{ x: 0, y: 0, width: 200, height: 200 }], // used by Fit
      frame({ overlay, dpr, time }) {                          // screen-space 2D, cleared every frame
        angle = time / 500;
        const p = ctx.worldToScreen(100, 100), size = 100 * ctx.camera.scale;
        overlay.setTransform(dpr, 0, 0, dpr, p.x, p.y); overlay.rotate(angle);
        overlay.fillStyle = '#00a1ff'; overlay.fillRect(-size / 2, -size / 2, size, size);
        ctx.stats.set('angle', angle % (2 * Math.PI));
      },
      dispose() {},                                            // release anything setup created
    };
  },
};
```

Hooks:

| Hook | When it runs |
| --- | --- |
| `frame(info)` | After every redraw. `info` has `camera`, `width`, `height` (CSS pixels), `dpr`, `time` and `overlay`, a 2D context covering the stage in device pixels. |
| `render(pass, info)` | In a WebGPU render pass on the canvas surface, submitted after the grid's own pass, so a scenario's GPU error never blanks the grid. Draw with `ctx.gpu.device` and the pass. |
| `bounds()` | Returns world rectangles `{x, y, width, height}` for Fit. |
| `dispose()` | Before the scenario is replaced or reloaded. May be async. |

`ctx` provides:

| Member | Purpose |
| --- | --- |
| `world` | A DOM layer in world coordinates; the camera transforms it. Put iframes and DOM artboards here. It is emptied on dispose. |
| `stage` | The stage element, for scenario-specific input. Drags that start on scenario content are not treated as pans. |
| `camera`, `dpr` | The current camera `{x, y, scale}` (screen = world × scale + offset) and device pixel ratio. |
| `setCamera(camera)`, `fit()` | Move the camera. |
| `screenToWorld(x, y)`, `worldToScreen(x, y)` | Convert between stage CSS pixels and world coordinates. |
| `gpu` | `{device, format}` of the shell's WebGPU device, or `null` without WebGPU. |
| `requestFrame()`, `animate(on)` | Redraw once, or continuously while `on`. |
| `controls` | `button(label, fn)`, `toggle(label, value, fn)`, `select(label, options, value, fn)`, `segmented(label, options, value, fn)` and `slider(label, {min, max, step, value}, fn)` add inspector controls. |
| `stats.set(key, value)` | Shows a readout in the inspector. |
| `signal` | An `AbortSignal` aborted on dispose. |
| `host` | Actions served by `run.ts`. `render({html, width, height, scale, clip, signal})` renders an HTML document in a pool of headless Chromes started with the server (`renderers` of them, each with its own GPU process, draining one queue in parallel) and resolves to `{png, ms, phases}`, where `phases` gives the milliseconds spent queued, setting the content, waiting for its frames, fonts and images, and taking the screenshot; it never touches the visible window. Each renderer keeps one page and replaces its content per job, so relative URLs resolve against `/api/`. Without `clip` the whole page is captured at `scale` device pixels per CSS pixel; with `clip` only that region, magnified by `scale` (up to 16,384 pixels a side). Aborting `signal` cancels the job: a queued job is dropped and a running one's result is discarded. `renderStats()` returns the server's queue counters. The rest need the Chrome window opened by `run.ts` (`owned` is `false` otherwise): `frame()` forces a new frame, `capture(rect, scale)` returns a native PNG `Blob` of a stage rectangle with the HUD hidden, and `paintRecord()` returns `{files, layers}`, Chrome's serialized paint record per layer (`files[i].url`) with the layer tree, when started with `--paint-record`. |

Errors thrown by a scenario appear in the error panel. They do not stop the
shell; fix the file and save to reload.

## Artboard swapper

The built-in scenario, `scenarios/artboard-swapper.scenario.js`, is the
direction the canvas follows: many artboards, each drawn as the cheapest
representation that looks right.

- Every visible artboard draws its best cached PNG with WebGPU, or a
  placeholder until one exists. Only artboards on screen are drawn.
- Missing resolutions come from a cache in three tiers: GPU textures (largest
  and off-screen evicted first, the 0.125× thumbnails kept), encoded PNG sheets
  in memory, and IndexedDB, keyed by a hash of each artboard's HTML.
- Only real misses are rendered, in the background renderer pool: nearest the
  centre first, a 0.125× pass before the exact resolution, small resolutions
  batched into sheets of up to 16 artboards. Work for artboards that leave the
  view (plus a 25% margin) is cancelled.
- Once artboards are large on screen, the few nearest the centre become live
  iframes, hidden over their PNG until they have loaded and painted.

Its content is either ten heavy themes from `scenarios/assets/wild.js`
(gradients, blend modes, filters, backdrop blur, many fonts and scripts, emoji,
generated SVG images) or simple pages. The inspector shows what each artboard
displays, queue and cache state, where fills came from, and swap latency.

Larger or temporary experiments belong in an ignored local folder, loaded with
`--scenarios`.

A save anywhere in the scenario's folder reloads the scenario file. Modules it
imports stay cached by the browser until Shift + `R` reloads the page.
