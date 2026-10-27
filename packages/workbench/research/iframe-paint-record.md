# Chrome paint records as replayable vectors

## Question

Can Chrome expose an already-rendered iframe's completed paint output (its
display list, with shaped glyph identities, positions, fonts, clips and
transforms) in a form that our own WebGPU renderer can replay, so inactive
artboards keep their appearance and stay sharp under zoom without our
implementing HTML/CSS layout, shaping or font-specific approximations?

## Status

Decided, 2026-10-27: **feasible, but pursue only with a native integration.**
Chrome's own paint record carries a complete, replayable vector payload, and a
library-free WebGPU replay of it is faithful in geometry and sharp to 800%.
No stock-Chrome route is acceptable for the product: the only route that
returns the payload needs `--enable-gpu-benchmarking` plus `--no-sandbox`, and
the stock tracing route returned nothing for page renderers. PNG snapshots stay;
no production code changed.

| Requirement | Outcome |
| --- | --- |
| Completed paint output, not a DOM walk | Satisfied: Chrome's serialized `SkPicture` of the page layer; the producer only applies Skia's matrix/clip state machine |
| Glyph identities, shaped positions, fonts | Satisfied: per-run glyph IDs and per-glyph positions after shaping (ligatures, Arabic contextual forms, CJK fallback), full embedded font programs with collection index and variation coordinates |
| Clips, transforms, order, effects | Satisfied as data: rect/rounded-rect clips, 4×4 matrices, paint order, gradient shader, blur mask filter, color filter, layer saves |
| Raster image | Satisfied: the source PNG travels in the payload and is replayed as a texture |
| Stock Chrome access | Failed: needs `--enable-gpu-benchmarking --no-sandbox`; the stock trace route emitted no page-renderer picture |
| Target the iframe viewport | Partially: capture is per composited layer; the iframe is cropped from the page layer by its rectangle |
| Retained independently of the iframe | Satisfied: about 154 KiB of path data plus a 347-byte PNG, no DOM or payload references |
| Vector sharpness under zoom | Satisfied: 400% and 800% views keep outline edges; PNG enlargement does not |
| Pixel parity with the live iframe | Not met with our own shader for glyphs. With Chrome rasterizing the recorded glyphs (amendment), text matches within 1–2 levels; the remaining differences are our shadow, gradient and shape-edge rasterization |
| Payload size and capture semantics | Established costs: 47–72 MB per capture (whole font files), stale recordings without a forced commit |

## Hypothesis

Chrome records every layer as a paint-op list before rasterization. If that
record can be read with its payloads (not only operation names), a replayer
needs no layout or shaping, only Skia's drawing semantics. Two unknowns decide
the question: whether stock Chrome or Electron exposes the record with text and
font payloads, and whether glyph outlines can be obtained from it without
platform font guessing.

## Method

Owner-approved plan: a 45-minute payload phase with an explicit stop rule
(no glyph IDs/positions/resolvable fonts → write up the negative result), then
at most 2 hours for a minimal replay, visible demonstration and fidelity
comparison. The owner approved `--no-sandbox` for one owned Chrome process
that loads only the local fixture.

Source inspection preceded the experiment (primary sources below). Mechanisms
examined:

1. CDP `LayerTree.makeSnapshot` / `snapshotCommandLog` / `replaySnapshot`.
2. CDP tracing category `disabled-by-default-devtools.timeline.picture`
   (`cc::DisplayItemList:snapshot` events with `skp64`).
3. `chrome.gpuBenchmarking.printToSkPicture(dir)` with
   `--enable-gpu-benchmarking` (and `--no-sandbox` for its file writes).
4. `Page.printToPDF`, rejected by source/documentation review: it lays the
   document out again for print and cannot target a same-origin iframe.
5. A Chromium or Electron patch, noted as the fallback integration boundary,
   not built.

Fixture: one deterministic same-origin 640 × 480 iframe. Six local system
TrueType files are served as web fonts (Georgia regular/bold/italic, Courier
New, STIX Two Text with `liga`, Arial Unicode for Greek, Cyrillic, Japanese,
Hebrew and Arabic). One line uses system Verdana, with a CJK fallback that
Chrome resolves to PingFang SC (CFF outlines). The fixture also has mixed
sizes, a rounded card with border and box shadow, an `overflow:hidden`
rounded clip over a linear gradient, a rotated and scaled badge, inline SVG
(rounded rectangle stroke, cubic stroke with round caps, translucent circle)
and a 48 × 48 PNG with rounded corners. Capture waits for every
`FontFace` to report `loaded`, for `document.fonts.ready` and for decoded
images. No font file is committed.

Replay, all in the ignored lab, with no third-party library:

- A reader for SKP format version 110, written from Skia's serializers. Every
  op must consume exactly its recorded byte size.
- Glyph outlines from the embedded font data: TrueType `glyf`, and CFF Type 2
  charstrings including CID-keyed fonts.
- A scene builder that applies Chrome's matrices and clips, then crops to the
  iframe rectangle.
- A WebGPU renderer: polygon-area coverage per physical pixel for shapes and
  every clip, curve flattening per zoom, one coverage item per text run, a
  linear-gradient paint, an image atlas texture, a stroker, and an analytic
  Gaussian rounded-rectangle approximation for the blur mask.

The demo offers Live / Extracted vector / PNG control, zoom 25–800%, pan,
reset, re-extract, native DPR, and readouts for extraction phases, payload
size, retained size and representation types.

Fidelity: live iframe versus vector and versus PNG, screenshots at native DPR
with identical camera and stage clipping. Cameras: 25%, 100%, and 400% and
800% views anchored on the heading, the card shadow, the Arabic/CJK lines, the
clip/image corner and the SVG stroke. All stage pixels are counted; no region
is excluded. To separate faithful extraction from our replay, Chrome's own
`LayerTree.replaySnapshot` of the same payload is compared with the live
iframe at 100%.

## Conditions

Chrome 156.0.8078.48, headed, an owned disposable profile with mock keychain
and basic password store, and an explicit sRGB color profile. Node 24.12.0.
Intel macOS (Darwin 24.6.0), 16 logical CPUs, 64 GiB RAM, AMD GPU through
Metal. Display DPR 2. The one-minute load average was about 14–35 during the
demo runs, so all timings are single contended observations.

## Results

### What each mechanism actually returns

| Mechanism | Runtime | Payload observed | Usable? |
| --- | --- | --- | --- |
| `LayerTree.snapshotCommandLog` | Stock | Op names and parameters. `drawTextBlob` logs only x, y and paint; images log only their dimensions; shaders are empty objects | No: no glyphs, fonts or pixels |
| `LayerTree.replaySnapshot` | Stock | A PNG data URL | Raster only; it also has no image decoder, so embedded images draw empty |
| Tracing `devtools.timeline.picture` | Stock | Browser-process and WebUI pictures only. The page renderer recorded its display list twice per run (`RecordingSource::FinishDisplayItemListUpdate`) but emitted no snapshot event, in three runs, including one with the `cc.debug.display_items`/`cc.debug.picture` categories added. Cause not isolated. By source, these pictures also omit font data (`kDontIncludeData`) | No, as observed |
| `gpuBenchmarking.printToSkPicture` | Stock binary with `--enable-gpu-benchmarking --no-sandbox` | One `.skp` per picture layer: the complete recording, with full font programs embedded (`kDoIncludeData` default) and images PNG-encoded | Yes |

Payload of the standalone capture (fixture page only), 47,311,623 bytes:

- 65 ops: 22 `drawTextBlob`, 6 `drawRRect`, 3 `clipRRect`, 2 `clipRect`,
  6 `concat44`, 3 `drawPath`, 1 `drawOval`, 1 `drawImageRect`, 2 `drawRect`
  and 1 `drawPaint`, plus save/restore. 36 paints, 2 paths, 22 text blobs,
  1 image; no unknown ops.
- 23 text runs with horizontal positioning in device pixels. The STIX run's
  glyphs 655/656/658 are its shaped `ffi`/`ffl` ligatures; the Arabic run
  carries contextual-form glyph IDs (6516, 6612, …).
- 9 typefaces, all with embedded data: 6 web fonts, system Verdana, PingFang SC
  (13 MB, CFF) and `.SF NS` (8 MB, variable). Variation coordinates are present.
- Effects: the shadow is `SkBlurMaskFilterImpl` with `SkModeColorFilter`; the
  gradient is `SkLocalMatrixShader` over `SkLinearGradient`.

The demo captures the whole demo page, so its payload is 72,307,832 bytes. The
same `.SF NS` file is embedded four times, once per weight/variation instance.
Font programs are more than 99.9% of the payload. Parsing took 1.4–18 ms;
`printToSkPicture` took 122–379 ms and transfer to the page 124–261 ms (single
contended samples). Building the iframe scene took about 40–190 ms, mostly
first-use font table access.

### Fidelity (stage screenshots, 2,722,320 physical pixels each)

| View | Vector changed px | Mean / max channel error | PNG control changed px | PNG mean / max |
| --- | ---: | ---: | ---: | ---: |
| 25% | 12,574 | 0.056 / 177 | 14,513 | 0.082 / 189 |
| 100% | 126,817 | 0.462 / 192 | 13,055 | 0.0013 / 2 |
| 400% heading | 53,848 | 0.591 / 196 | 333,352 | 3.476 / 215 |
| 400% card + shadow | 240,594 | 0.433 / 202 | 336,153 | 2.065 / 215 |
| 800% Arabic/Hebrew/Verdana | 26,852 | 0.214 / 150 | 327,386 | 3.118 / 215 |
| 800% clip + image | 834 | 0.0003 / 10 | 658,897 | 0.316 / 96 |
| 800% SVG strokes | 7,382 | 0.023 / 102 | 108,428 | 0.789 / 155 |

Mean error is absolute channel error averaged over all RGBA values of the
stage. It is not a fidelity percentage. Zero differing pixels remains the
parity criterion, and it is not met.

At 100%, 80,831 of the differing vector pixels differ by at most 2 levels.
Most of those are gradient pixels: Chrome dithers gradients and the replay
does not. The 400% card view's 212,160 one- or two-level differences are the
approximate shadow. Errors of 33 levels or more are glyph and shape edges.

The PNG control is not bit-exact either: at 100% it differs from the later
live repaint by up to 2 levels in 13,055 pixels. This bounds Chrome's own
re-raster noise for this scene.

Extraction versus replay: Chrome's own software replay of the identical
payload differs from the live GPU-rastered iframe in 152,646 of 1,228,800
frame pixels (mean 1.93, max 204). Part of that is the missing image, which
CDP replay cannot decode. Even Skia, replaying the same record through
another backend, does not reproduce the screen exactly. Our replay's
remaining differences have the same character, and none of them stems from
missing geometry.

The magnified screenshots were inspected:

- All text, ligatures, Arabic joining, Hebrew, CJK fallback, the rotation,
  clips, gradient, SVG strokes and the image are present and positioned.
- At 800% the outline edges match the live re-raster closely, while the PNG
  is visibly enlarged.

Differences introduced by our renderer:

- Small-text glyph coverage: CoreText thickens rasterized glyphs, and the
  replay draws exact outlines without hinting.
- The shadow, approximated with an analytic erf rather than Skia's blur.
- No gradient dithering.
- Image resampling filter details.

Retained scene: 37 items (22 text runs covering 409 glyphs, 173 distinct
outlines; 12 paths, 3 of them strokes; 1 gradient, 1 image, 1 blur) and
9 clips. That is about 157,000 bytes of float path data plus the 347-byte PNG.
The scene keeps no reference to the payload or the iframe. Representation
types:
- vector paths and glyph outlines;
- one raster source image as a texture;
- one approximated effect (blur).

There are no text bitmaps, page screenshots or raster fallbacks in the vector
mode. The PNG mode is a separately labelled control.

Replay cost diagnostics (single contended samples):
- Geometry rebuilds at a new zoom took 4–40 ms (16,752–37,515 edges).
- GPU completion of ten repeated cached draws at 100% took 8.8–44.3 ms in the
  final run; the preceding run recorded 8.6–240 ms.
- First draws per view took 14.6 ms – 4.7 s in the final run, against
  8.4–99.7 ms in the preceding run for identical geometry.

These samples establish no frame-time claim. Run-level coverage loops over
every edge of a row band, so per-pixel cost grows with run length. A
production renderer would need tiled binning.

### Failures and corrections (preserved)

- Trace route: no page-renderer snapshot in three runs (above).
- The first trace run stalled on an animation-frame wait in the headed
  window; it was replaced by a timed wait.
- A WGSL field named `meta` (a reserved word) invalidated the pipeline. The
  runner now surfaces shader compilation errors.
- Per-glyph coverage items left visible seams inside joined Arabic letters at
  800%. The replay now builds one coverage union per text run, as Skia
  rasterizes one run mask. Pixel counts changed by less than 0.1%; the seams
  disappeared in the inspected screenshot.
- Re-extract from PNG mode produced an empty artboard (709 glyphs decoded,
  all outside the frame). `printToSkPicture` serializes the last committed
  recording, and the iframe's switch back to visible had not committed. The
  host now forces a frame before printing, the page rejects captures without
  iframe text and image, and the runner asserts that re-extraction reproduces
  the identical scene. The corrected run passed.

## Correctness

The reader accepts only version 110, and every op must decode to exactly its
recorded size. The scene builder reports every op, blend mode, effect, font
format or variation axis it cannot represent. For the iframe region, the final
report lists none as unsupported. It lists these approximations:
- the transparent `kSrc` clear, a no-op over an opaque stage;
- non-antialiased clips drawn with coverage;
- the analytic blur;
- the ignored gradient color space.

`.SF NS` variation deltas are not applied, but only host-UI text uses that
font.

Trusted pointer input passed for the mode buttons, the zoom buttons, the pan
drag, reset and re-extract, and the original iframe `Window` identity
survived. The demo browser and server remain open for the owner, and the
runner closes both on SIGINT or SIGTERM. Earlier lab browsers closed after
their runs.

## Threats to validity

- One fixture, one platform (macOS, CoreText, Skia's `ctxt` typefaces), one
  Chrome version, and mostly TrueType fonts. Arbitrary-font support is not
  established: there is no CFF2, no variable-font delta application, no color
  glyphs (`sbix`/`COLR`), and no text on paths or RSXform runs.
- Composited layers are not handled. Their placement lives in cc property
  trees, which the picture does not include. Pages with `will-change`, video,
  canvas or 3D transforms need that data.
- Filters, backdrop filters, blend modes, masks and layer opacity are only
  partly exercised by this fixture. They arrive as flattenable parameters
  that a replayer must implement one by one.
- The SKP format is versioned and internal; its serializers changed often
  (versions 82–110 are documented in the source).
- Timings came under heavy host contention and are diagnostics only.

## Decision and follow-ups

Pursue only with a native integration; do not ship the benchmarking route.
The payload question is answered: Chrome's paint record contains what a
faithful vector replay needs, and the replay keeps outlines sharp under zoom.
Its blocking costs are the access path, not the data:

- `--no-sandbox` for the whole browser is unacceptable in Workbench.
- The capture covers whole layers and needs a forced commit.
- Payloads are tens of megabytes because full font programs are embedded per
  typeface instance.
- The format is unstable across Chromium versions.

The smallest credible next boundary is an Electron/Chromium-side capture hook
that, for one frame:

1. forces and awaits a commit;
2. serializes that frame's paint record with an `SkSerialProcs` typeface hook
   that emits font identity plus only the used glyph outlines (`SkFont::getPath`),
   and an image hook for source images;
3. returns the bytes over IPC, with the renderer sandbox intact.

That requires a patched Electron or Chromium build. The cost (build
infrastructure, per-release format tracking) should be reviewed with the
owner before any work. Untested alternatives: per-window `sandbox: false` with
the benchmarking switch in Electron, and Electron's `contentTracing` for the
trace route.

The glyph-rasterization gap remains even with a perfect payload. Exact small
text needs either native glyph masks (raster) or a platform glyph-coverage
model. This lab does not choose between them.

Total effort was about 40 minutes of active work from plan approval to this
record, well inside the 2 h 45 min ceiling. No benchmark campaign, extension
or production integration is scheduled.

### Amendment: Chrome-rasterized glyphs (2026-10-27)

The owner asked whether text pixels can match when Chrome rasterizes the glyphs
instead of our shader. Budget: 60 minutes, one implementation, at most one
targeted correction, and the same cameras and comparisons as above.

Candidate: everything except text stays as above. For each text run, the
recorded glyph IDs are drawn by Chrome's own canvas text path at their exact
recorded positions and transform for the current zoom and DPR. The resulting
alpha mask becomes the run's coverage texture in paint order, multiplied by
clip coverage. Canvas text needs characters, not glyph IDs, so each embedded
font is rewritten for the lab:
- a new `cmap` maps private-use code point `U+F0000 + glyph ID` to that glyph;
- shaping tables (`GSUB`, `GPOS`, `GDEF`, `morx`, `kerx`, `kern`) are dropped,
  so each single character draws exactly its glyph;
- outline, metrics and hinting tables are unchanged.

The mask is regenerated whenever the camera changes, so it is never enlarged.
It is a raster produced by Chrome at the displayed resolution, not a stored
text bitmap; the retained scene still keeps glyph IDs, positions and outlines.

Decision rule: zero differing text pixels at matched cameras would establish
text parity. Any remaining difference is reported with its location and
magnitude, and outline mode stays available as the control.

Results: all eight rewritten fonts (TrueType and CFF) loaded through Chrome's
font sanitizer in 272–280 ms in total, and every in-frame run drew from them.
Same cameras and stage (2,280 × 1,130 physical pixels); a fixed header height
replaced a first attempt whose mode readout changed the stage size and
invalidated its outline comparison (preserved locally).

| View | Chrome glyphs: changed px | of which 1–2 levels | of which > 32 levels | Own outlines: changed px | PNG control |
| --- | ---: | ---: | ---: | ---: | ---: |
| 25% | 4,836 | 3,974 | 28 | 12,595 | 14,533 |
| 100% | 82,532 | 80,296 | 126 | 126,832 | 0 |
| 400% heading | 3,373 | 3,366 | 7 | 50,267 | 316,443 |
| 400% card + shadow | 198,733 | 198,062 | 60 | 227,339 | 323,942 |
| 800% Arabic/Hebrew/Verdana | 7,224 | 7,201 | 1 | 26,852 | 327,387 |
| 800% clip + image | 805 | 405 | 0 | 805 | 600,819 |
| 800% SVG strokes | 6,911 | 982 | 0 | 6,911 | 101,367 |

The inspected difference images show text practically resolved. Text-only
views keep a dotted 1–2-level fringe at glyph edges, consistent with rounding
when the 8-bit mask is blended over the background in a separate pass rather
than inside Skia's own draw. That cause is not isolated. At 100%, the text
regions are dark (unchanged) in the difference image. The remaining large
differences are in work still done by our shader: the analytic shadow, missing
gradient dithering, and antialiased edges of rounded borders, the SVG curve
and the image's rounded clip.

Text drawing took 3.2–15 ms per camera change for 78–402 glyphs. Cached draws
completed in 2.9–16.4 ms (single samples under load). The masks are produced
by Chrome at the displayed resolution and never enlarged; the retained scene
still stores only glyph IDs, positions and outlines.

Conclusion of the amendment: text pixel parity is reachable within 1–2 levels
by letting Chrome rasterize the recorded glyphs. Exact zero is not
established. The same principle applies to the rest of the record: drawing
paths, clips, gradients, shadows and images through Chrome's 2D canvas at the
current camera would use Skia for every pixel. Whether that canvas path
matches the page's GPU tile rasterization edge for edge is the next unknown.

### Amendment: cost against PNG snapshots (2026-10-27)

Owner question: how the Chrome-glyph replay compares in cost with PNG
snapshots at 1×, 2×, 3× and "10000×", and with the outline-only replay.
Budget 75 minutes, one bounded correction.

A 10000× PNG of the 640 × 480 artboard would be 6,400,000 × 4,800,000 pixels,
about 112 TiB as RGBA and 390× wider than the largest texture the adapter
allows. It is reported by arithmetic, not run. Variants:
- PNG at 1×, 2× (native) and 3× of CSS pixels;
- PNG at 16×, the resolution that stays sharp at 800% zoom on this 2× display
  (10,240 × 7,680);
- vector replay with own outlines;
- vector replay with Chrome glyphs.

PNG acquisition is a CDP clipped screenshot rendered at that scale, then
transfer, decode and GPU upload. Vector acquisition is print, transfer, parse
and scene build, with font registration reported separately; it is shared by
both vector variants.

Samples:
- Acquisition: 10 per variant (3 for 16×), variant order alternated.
- Redraw: 20 per cell at 100%, 400% and 800%, measured to GPU completion,
  for three actions: an identical frame, a one-physical-pixel pan, and a 2%
  zoom step. Variants are interleaved by round.

Memory is reported as encoded bytes and the GPU allocations the lab owns, not
process RSS. Quality is the pixel difference against live at the same three
zooms. The host was heavily loaded (one-minute load 14–35); comparisons are
matched under contention, and absolute values are not latency guarantees.

Results (one run set, adapter texture limit 16,384). Acquisition, ms, from the
start of capture until GPU-ready:

| Variant | Size | p50 | p95 / max | Breakdown p50 |
| --- | --- | ---: | ---: | --- |
| PNG 1× | 640 × 480 | 62 | 101 | capture 47, decode 3.5, upload 3.5 |
| PNG 2× | 1,280 × 960 | 108 | 195 | capture 86, decode 12, upload 7 |
| PNG 3× | 1,920 × 1,440 | 202 | 297 | capture 165, decode 24, upload 13 |
| PNG 16× (n = 3) | 10,240 × 7,680 | 6,561 | 7,369 | capture 4,158, decode 1,501, upload 844 |
| Vector, own outlines | — | 450 | 993 | commit 76, print 176, transfer 142, parse 2, scene 33 |
| Vector, Chrome glyphs | — | 760 | 1,820 | the above plus font registration 341 |

Memory (lab-owned allocations):

| Variant | Encoded | GPU allocations |
| --- | ---: | ---: |
| PNG 1× / 2× / 3× | 82 / 194 / 324 KiB | 1.2 / 4.7 / 10.5 MiB texture |
| PNG 16× | 2.5 MiB | 300 MiB texture |
| Vector, own outlines | 155 KiB retained (77 MiB transient payload, almost all fonts) | 0.5–0.6 MiB edge buffers per view |
| Vector, Chrome glyphs | 155 KiB retained, plus the rewritten fonts registered in the page | 0.12–0.3 MiB buffers + 2.0–8.5 MiB text atlas per view |

Redraw to GPU completion, ms, 20 samples per cell (21 for 16×), p50 / p95:

| Variant | Identical frame | 1-pixel pan | 2% zoom step |
| --- | --- | --- | --- |
| PNG 1×, 2×, 3× (all zooms) | 1.0–1.5 / 1.6–3.2 | 1.0–1.4 / 1.6–3.6 | 1.1–1.5 / 2.3–3.1 |
| PNG 16× (all zooms) | 2.1–3.0 / 4.4–8.3 | 2.9–3.1 / 6.0–10.0 | 2.6–3.4 / 5.1–5.8 |
| Vector, own outlines, 100% / 400% / 800% | 8.5 / 13.3 / 6.1 (p95 ≤ 15) | 8.4 / 10.7 / 6.2 (no rebuild) | 18.2 / 18.1 / 11.6 (p95 36 / 28 / 21) |
| Vector, Chrome glyphs, 100% / 400% / 800% | 3.2 / 2.3 / 2.3 (p95 ≤ 5.5) | 30.4 / 13.9 / 10.3 (p95 105 / 37 / 23) | 32.7 / 17.8 / 10.3 (p95 115 / 42 / 21) |

The Chrome-glyph variant redraws text with Chrome on every camera change,
pans included, because the cache key includes the camera. Pans by whole
physical pixels could reuse the masks; that optimization was not measured.

Quality, changed pixels against live (pixels > 32 levels in parentheses):

| Variant | 100% | 400% | 800% |
| --- | ---: | ---: | ---: |
| PNG 1× | 252,819 (76,035) | 536,864 (293,893) | 558,885 (300,327) |
| PNG 2× | 9,232 (0) | 316,443 (148,990) | 312,687 (157,833) |
| PNG 3× | 158,849 (14,053) | 212,654 (98,536) | 210,261 (103,576) |
| PNG 16× | 113,575 (23,748) | 44,238 (22,810) | 10 (10) |
| Vector, own outlines | 126,832 (22,665) | 50,267 (23,814) | 20,321 (3,055) |
| Vector, Chrome glyphs | 82,532 (126) | 3,372 (6) | 4,290 (7) |

PNG 16× is near-exact at 800%. At lower zooms it is drawn with plain bilinear
sampling and no mipmaps, so it aliases (8:1 at 100%). The 2× PNG is exact
only at its own resolution; this run differed by at most 2 levels in 9,232
pixels, against 0 in the previous run. Chrome glyphs give the fewest large
errors at every zoom; at 100% the remaining small differences are mainly the
gradient and the shadow.

### Amendment: complex page and Chrome-raster replay (2026-10-27)

Owner request: a demo with a complex website. A deterministic 1200 × 860
landing page was added. It has:
- linear, radial, conic and repeating gradients;
- multiple and inset shadows, text shadows, backdrop blur, CSS blur and color
  filters, opacity groups and blend modes;
- clip-paths, masks and `object-fit` photos;
- a tiled background image, SVG, a table and native form controls;
- a 3D-transformed card;
- about 20 fonts: script, display, monospace, CJK/Arabic/Hebrew, color emoji.

Procedural PNG bitmaps stand in for photos. Budget about 2 hours.

Findings that change the earlier picture:
- Chrome split the page into six picture layers: the backdrop-filter panel,
  the opacity group, the 3D card and two regions promoted for overlap. The
  partition differed between two captures of the same page. Placement comes
  from CDP `LayerTree` (each layer's `ScreenSpaceTransform`, in paint order),
  re-enabled at capture because navigation drops it and emitted only with a
  new frame. The perspective layer is approximated by an affine fit of its
  corners.
- The payload grew to 253 MB, 188 MB of it Apple Color Emoji for four emoji.
  That font could not be registered for Chrome-drawn glyphs ("Invalid font
  data"), so emoji are not replayed.
- Compositor-level effects on composited layers are not in any picture. The
  glass panel's backdrop blur and saturation are missing in every replay.
  Composited opacity and blend would be missing too.

Candidate added: **Chrome-raster replay** (`Chrome-raster vector`). Every op
of every layer is drawn through Canvas 2D at the current camera:
- paths, clips, transforms and layers via offscreen canvases;
- linear, radial, conical and sweep gradients, including premultiplied
  interpolation and repeat/mirror tiles;
- image and picture shader patterns, dashes and points;
- blur masks, image-filter blur, merge, transform and color-matrix filters
  (via SVG `feColorMatrix`), and blend modes;
- text through the glyph-ID fonts.

Each layer paints its own surface before compositing, because every layer
picture begins by clearing its backing with `kSrc`; the first attempt drew
them into one canvas and erased the frame. The WebGPU path still replays
only the main layer and lacks these shaders, so it is not representative for
such pages.

Results, stage 2,920 × 1,910 physical pixels. Each cell shows changed pixels
and, in parentheses, pixels off by more than 32 levels:

| View | Chrome-raster vector | PNG (native 2×) | WebGPU vector |
| --- | ---: | ---: | ---: |
| 100% | 1,361,194 (100,156) | 0 (0) | 3,271,045 (2,973,166) |
| 50% | 364,046 (25,481) | 602,295 (12,881) | 822,794 (744,741) |
| 200% hero | 2,205,836 (38,627) | 3,222,477 (82,337) | 4,638,901 (4,364,421) |
| 400% glass + photo | 3,286,688 (1,270,908) | 3,443,928 (83,471) | 4,934,369 (3,237,429) |
| 400% cards | 1,574,977 (947) | 2,443,608 (233,463) | 5,340,843 (5,281,899) |
| 800% nav | 1,164,051 (3) | 3,151,868 (102,932) | 4,280,563 (2,850,927) |
| 800% quote | 53,857 (118) | 752,317 (292,097) | all |
| 800% strip + form | 441,329 (259) | 3,770,513 (35,247) | all |

Gradient interpolation and tile corrections reduced the 100% large errors from
135,660 and the 200% ones from 65,483. The inspected images and difference
maps show the remaining large errors in four places: the glass backdrop
(compositor effect), the emoji, the perspective card, and a gradient
interpolated in a non-sRGB color space (reported, not replayed). Elsewhere the
differences are 1–8-level noise from blur and image resampling.

Costs (single contended samples):
- Ready for Chrome-glyph text after 3.8 s: print 1.2 s, transfer 1.0 s, font
  registration 1.3 s.
- Raster replay per camera change took 25–160 ms, with one 6.0 s outlier at
  800% that was not isolated; upload and draw took 45–905 ms.

Auto mode shows the PNG up to 100% and the Chrome-raster vector above. It was
exact at 100% and matched the raster results above 100%. At 50%, the
mipmapped PNG had fewer large errors (12,881) than the raster replay (25,481).
The demo remains open with the complex page.

### Primary sources

- Chromium [`cc/paint/display_item_list.cc`](https://github.com/chromium/chromium/blob/main/cc/paint/display_item_list.cc):
  `EmitTraceSnapshot`, `AddToValue` (`skp64`, `layer_rect`).
- Chromium [`cc/layers/recording_source.cc`](https://github.com/chromium/chromium/blob/main/cc/layers/recording_source.cc):
  `FinishDisplayItemListUpdate` emits the trace snapshot.
- Chromium [`cc/debug/picture_debug_util.cc`](https://github.com/chromium/chromium/blob/main/cc/debug/picture_debug_util.cc):
  `SerializeAsBase64`, typeface proc with `kDontIncludeData`.
- Chromium [`content/renderer/gpu_benchmarking_extension.cc`](https://github.com/chromium/chromium/blob/main/content/renderer/gpu_benchmarking_extension.cc):
  `PrintToSkPicture`, `SkPictureSerializer` (PNG image proc, `--no-sandbox` note).
- Chromium [`inspector_layer_tree_agent.cc`](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/inspector/inspector_layer_tree_agent.cc)
  and [`logging_canvas.cc`](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/platform/graphics/logging_canvas.cc):
  snapshot ID only, a command log without glyph payloads, PNG replay.
- [CDP LayerTree domain](https://chromedevtools.github.io/devtools-protocol/tot/LayerTree/).
- Skia [`SkPictureData.cpp`](https://github.com/google/skia/blob/main/src/core/SkPictureData.cpp)
  (`WriteTypefaces` defaults to `kDoIncludeData`; tag layout),
  [`SkPicturePlayback.cpp`](https://github.com/google/skia/blob/main/src/core/SkPicturePlayback.cpp),
  [`SkPictureFlat.h`](https://github.com/google/skia/blob/main/src/core/SkPictureFlat.h),
  [`SkPicturePriv.h`](https://github.com/google/skia/blob/main/src/core/SkPicturePriv.h) (version 110),
  [`SkTextBlob.cpp`](https://github.com/google/skia/blob/main/src/core/SkTextBlob.cpp),
  [`SkFont_serial.cpp`](https://github.com/google/skia/blob/main/src/core/SkFont_serial.cpp),
  [`SkFontDescriptor.cpp`](https://github.com/google/skia/blob/main/src/core/SkFontDescriptor.cpp),
  [`SkPaintPriv.cpp`](https://github.com/google/skia/blob/main/src/core/SkPaintPriv.cpp),
  [`SkPath_serial.cpp`](https://github.com/google/skia/blob/main/src/core/SkPath_serial.cpp),
  [`SkGradientBaseShader.cpp`](https://github.com/google/skia/blob/main/src/shaders/gradients/SkGradientBaseShader.cpp),
  [`SkBlurMaskFilterImpl.cpp`](https://github.com/google/skia/blob/main/src/core/SkBlurMaskFilterImpl.cpp).
