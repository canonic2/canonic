# Workbench shell and content rendering

Status: content-only renderer implemented. Browser acceptance checks document
requests and child markup as well as warm sessions, labels and review behavior.
Independent UI/backend hosting acceptance remains separate work.

This topic owns the shell/content boundary. [Modules](modules.md) owns UI/backend
independence; [core](../core.md) owns selection, [readiness](../preview-pipeline/readiness.md)
owns render completion, and [multiple artboards](../canvas/multiple-artboards.md) owns instances.

## Required flow

```text
Browser or VS Code host
  Workbench shell — navigation, page list, controls and canvas
    Canvas presentation — artboard labels, placement, camera and selection
      Content renderer — content viewport, bridge and owned rendering lifecycle
        Project content — authored HTML, compiled preview, Storybook, docs or native surface

Workbench UI client → backend API → project metadata, operations and preview services
Host asset delivery → browser-executable Workbench assets
```

One Workbench application shell is mounted per canvas document. A VS Code
webview may embed that shell in an iframe, and the editor sidebar may separately
host the page-list components. Neither case permits another Workbench application
shell inside an artboard.

### Ownership

| Owner | Responsibility |
| --- | --- |
| Workbench shell | Page-list presentation, navigation, selected-instance controls, toolbar and review commands |
| Canvas | Artboard frames and labels, placement, selection decoration, pan/zoom and the single canvas background/grid |
| Content renderer | The selected content's viewport, rendering adapter, readiness, instance-local interactions/annotations, capture bridge and disposal |
| Backend | Project data and operations; compilation, rendering or proxying of project content according to its adapter |
| Host infrastructure | Load UI assets and configure its API/editor adapters; generic asset serving does not compose UI components |

## Content renderer contract

- A renderer is a dedicated content-only entry point or adapter. It must never
  load Workbench's application `index.html`, initialize its shell bootstrap, mount
  navigation/page-list/toolbars, or load a complete shell and hide it with CSS.
- A browser renderer loads authored content, compiled preview output, a docs
  content document, or Storybook's proxied `iframe.html` as appropriate. Native
  adapters provide a native surface. A renderer need not be an iframe.
- Minimal wrapper DOM, a preview bridge, a content mount and annotation overlays
  are permitted. They do not contain another Workbench canvas, grid, camera,
  artboard label, zoom menu, global toolbar or navigation controller.
- The canvas supplies logical viewport dimensions. The renderer fills that
  viewport and may report validated intrinsic bounds for content fitting; it
  does not independently choose canvas placement, zoom or artboard decoration.
- Render requests carry instance/generation and destination identity. Readiness,
  errors, controls, captures and focus reports are accepted only from the owning
  runtime and the current request. DOM menus are not the source of renderer state.
- Each visible artboard owns its mutable document or native session, inputs,
  scroll and annotations. Compiled assets and catalog data can be shared; two
  independent artboards cannot share one mutable preview document.
- Returning to an ordinary page or Storybook lens preserves its retained content
  session within the documented resource bounds. Removing the nested shell must
  not reintroduce full Storybook startup or iframe reloads on every selection.
- Navigation revokes interaction with the outgoing destination before accepting
  new work. Its pixels remain during the bounded grace period under its outgoing
  labels and context; they cannot appear beneath the next destination's label.
  The shell owns transition status;
  renderers report data rather than creating competing application loading UI.
- The target content must actually finish rendering before it is ready. Loading
  the renderer wrapper or hiding shell chrome is not a readiness signal. Failure
  stays an explicit problem for that destination. Any preserved outgoing view
  keeps its actual identity and cannot be presented as the failed target's content.

Docs shown alone retain their reading presentation; they do not acquire an
artboard label or selection border. Docs beside explicitly composed artboards use
a content viewport, not another Workbench shell. Embedded docs examples likewise
mount content. Workbench's own design examples may intentionally render individual
Workbench components as project content; they still do not initialize the application.

## Navigation feedback

Page selection expresses the user's intent immediately, while the canvas keeps
the identity of the content it actually displays. The sidebar marks the picked
page synchronously, before editor messaging, story lookup or rendering finishes.
The breadcrumb may identify the requested destination while its artboard is pending.

| Time or outcome | Canvas presentation |
| --- | --- |
| Pick begins | Keep the outgoing artboard or Docs viewport visible, with its existing label, geometry, camera, reading mode and reference; make it inert |
| Destination ready before 200 ms | Swap to its ready content and metadata immediately, with no loading message |
| Still pending at 200 ms | Conceal the outgoing view and show **Loading preview…**; it no longer contributes visible artboards to reference/context publication |
| Ready after the deadline | Replace loading feedback with the ready destination and its metadata |
| Failure | End the grace period and show the destination's failure without outgoing pixels under its label |

The 200 ms period starts at each new selection, including a selection whose story
metadata is still being resolved. It delays feedback, never the rendering work or
a ready page. On first open there is no outgoing content to retain; the loader
still observes the grace period. Rapid picks restart the deadline for the latest
destination, preserve the last displayed view while its new grace period applies,
and invalidate older readiness/error callbacks. A view already concealed at a
deadline is not resurrected by another pending pick. Completion, clear, failure
and disposal release pending deadlines and transient outgoing renderers.

Only a complete ready declared-artboard group is published as the replacement;
creating the expected instances alone does not establish content readiness.
Pending renderers must already have a correctly sized layout viewport. The
canvas supplies invisible, inert staging slots independently of the displayed
state. It cannot defer all slot creation or use `display:none` until readiness:
the renderer's paint/readiness callbacks may depend on that layout, producing
a startup deadlock. Promotion preserves the prepared iframe document and its
state; it changes presentation without reloading content.
Existing warm ordinary/Storybook/Docs sessions keep their retention semantics.
Outgoing declared instances may remain parked until the grace period ends or the
replacement commits, then are disposed. This transition does not authorize an
unbounded pool of outgoing runtimes.

Screenshots and handoffs remain unavailable throughout pending navigation, even
when outgoing pixels are visible. The canvas reference and agent context describe
the displayed outgoing view during the grace period, then no visible artboards
while loading feedback is shown, then the committed replacement. Incoming labels,
sizes, automatic camera fitting and reading mode must not be applied to the
outgoing presentation.

### Why the grace period exists

Immediate sidebar feedback confirms the pick independently of rendering latency.
Keeping the previous view briefly provides visual continuity through fast page
changes, while suppressing both a blank canvas flash and a loading-message flash.
A page ready at 25 ms appears at once; it does not wait for 200 ms. A slower page
receives explicit loading feedback at the deadline so the old content cannot
linger indefinitely and suggest that the pick was ignored. The threshold is a
product interaction choice, not a claim that rendering completes within 200 ms.

Keeping requested selection separate from displayed identity also prevents a
reference or handoff from describing a new page while showing the old one's
pixels. This improves perceived responsiveness without changing rendering speed,
weakening readiness, or permitting capture of unfinished content.

The navigation controller tests cover deadline cancellation, restart, supersession
and group readiness. Preview-session tests cover transient outgoing retention and
cleanup. The host's [browser regression](../../server/node/src/modules/host/navigation.test.ts)
exercises sidebar selection, outgoing pixels/labels/reference, ready commit,
deadline loading, late readiness and failure through the actual shell and renderer.

## Navigation state

**Status:** agreed, not implemented. Navigation will hold its state in a store
built on Lattice's state management (`canonic-lattice`), which defines stores,
selectors, bindings, phases, transitions and asynchronous work. Today the same
facts live in closures across the legacy shell, the navigation and
preview-session controllers, the canvas bootstrap and the content renderer,
reconciled by explicit calls such as `present()` and per-owner sequence counters.

### State

| Field | Meaning |
| --- | --- |
| `id` | Opaque identity of the current navigation |
| `requested` | The destination being navigated to: page, scenario, lens, example and declared artboards |
| `displayed` | The destination and navigation `id` whose content is presented, or `null` |
| `phase` | `idle`, `pending`, `loading`, `shown` or `failed` |
| `problem` | The failure message when `phase` is `failed` |

| Phase | Meaning | Canvas presentation |
| --- | --- | --- |
| `idle` | Nothing requested yet | Empty canvas |
| `pending` | Requested, inside the grace period | Outgoing view retained and inert |
| `loading` | Requested, grace period elapsed | **Loading preview…**, no outgoing pixels |
| `shown` | Requested content is ready and displayed | Requested content, interactive |
| `failed` | The requested destination failed | Its failure, no outgoing pixels |

Requested and displayed are separate fields, as [navigation feedback](#navigation-feedback)
requires. The canvas accepts input exactly when `phase` is `shown` or `failed`
and no capture is running. One binding writes the canvas's `inert`,
`aria-busy`, visibility and loading message; no other code writes them. The
camera during pan, zoom and reading scroll stays in `<wb-canvas>`; navigation
receives settled values only.

### Events

| Event | Source | Effect |
| --- | --- | --- |
| `picked(destination)` | Sidebar, route, editor | New `id`; `requested` set; `pending` |
| `deadline(id)` | Grace timer | `pending` → `loading` when `id` is current |
| `ready(id, view)` | Renderer | `shown`, `displayed` = view, when `id` is current |
| `failed(id, problem)` | Renderer or activation | `failed` when `id` is current |
| `navigated(from, id, destination)` | Renderer, after a link or form inside the preview | Adopted as the current navigation when `from` is current |
| `captureStarted` / `captureEnded` | Review | Input blocked for the capture's duration |

### Identity across documents

The canvas document and each content renderer own separate stores and exchange
messages that carry navigation identity:

- The shell sends every render request with its navigation `id`. The renderer
  tags readiness, failure and metadata reports with the `id` of the request
  they answer, as the [content renderer contract](#content-renderer-contract)
  requires.
- When the preview navigates itself, through a link or form the shell hands
  back to it, the renderer mints a new `id` in its own namespace and reports
  `navigated(from, id, destination)`. The shell adopts it only when `from` is
  its current `id`; later reports for the new `id` then match.
- Reports whose `id` is not current change nothing. A late report from a
  superseded navigation, a parked session or a reused warm renderer can never
  mark the current navigation ready or replace what is displayed.
- The shell and each renderer runtime mint identities with distinct prefixes,
  unique per document lifetime.

### Invariants

These hold after every event, in any order:

1. When `phase` is `shown`, `displayed.id` equals `id` and `displayed`
   matches `requested`.
2. An event carrying a non-current `id` returns an empty patch.
3. The canvas accepts input if and only if `phase` is `shown` or `failed` and no
   capture is running.
4. In `loading` and `failed`, no outgoing content is presented.
5. A navigation that receives `ready` or `failed` for its `id` leaves
   `pending` and `loading`; a link followed inside the preview, once reported,
   reaches `shown` or `failed` the same way as a pick.

Transition tests drive generated event sequences through these invariants.
Browser regressions through the shell and renderer cover the discoveries below.

### Migration

- Workbench first adds Lattice as a dependency: the server serves its modules
  unchanged beside the UI, the canvas, renderer and sidebar entry points resolve
  its import, the webview CSP admits it, and VSIX packaging includes it.
- Navigation state, transitions and the grace timer move into a navigation store
  under `ui/src/modules/navigation/`, with its binding beside it. The canvas
  bootstrap's displayed and painted copies, and the navigation controller's flag
  set, are replaced by the store's fields and selectors.
- Renderer messages carry navigation identity as specified above.
- The legacy shell keeps its route and sidebar code; an adapter at its boundary
  turns route changes into `picked` and `shown` destinations into route writes.
- Other capabilities move to Lattice stores when they are substantially reworked.

### Discoveries

- **Link inside a preview froze the canvas (2026-10-23).** Acme demo, App ›
  Dashboard, **Actions** on: clicking **Inbox** inside the preview loaded
  `app/inbox.html` in the artboard while the shell's selection stayed on the
  dashboard. The canvas computed itself as pending, set `inert` on the artboards
  and skipped the step that adopts the renderer's destination as the route, so it
  never left that state. Every click and key on the artboard was ignored until
  another page was picked. Cause: the requested destination had two owners (the
  shell's selection and the renderer's target) reconciled by a call that a later
  change gated on the very state it was meant to resolve. Invariants 3 and 5 and
  the `navigated` event cover it.
- **Blank Docs page after rapid collection switches (2026-10-23).** In the
  Workbench space, switching from the Actions switch Docs page to Interface ›
  Sidebar and back within about a second left a blank Docs viewport that stayed
  blank. Observed in a screen recording; not yet reproduced or traced.
  Hypothesis: a late report from the superseded visit or the retained Docs
  session was accepted as readiness for the returning navigation. Invariants 1
  and 2 cover that class of failure.

### Open questions

- Whether a renderer that never reports for its navigation needs a readiness
  timeout that produces `failed`, or whether `loading` may last indefinitely
  for slow compilation.
- Whether the first migration also moves the legacy shell's route state into
  the navigation store, or leaves it behind the adapter until the shell itself
  is migrated.

## Backend and asset delivery

Compiling a project's React/Vue previews, rendering Astro/docs content, or running
and proxying the project's Storybook is expected backend work. That work produces
project content, not Workbench's sidebar, canvas or toolbar. The frontend is
responsible for choosing and mounting the correct content destination.

Workbench UI source is browser-native JavaScript, HTML and CSS. The browser host,
VS Code sidebar and portable exports serve or copy authored assets unchanged;
no UI compiler, type stripping or bundler participates. Browser runtime imports,
including shared rules, resolve directly to JavaScript. A static host can deliver
those assets without knowing their component tree. The UI must also
be usable with a fixture API client; the backend APIs must work without mounting UI.
The source-language or hosting arrangement is not the cause of nested shell rendering.

## Implementation ownership

[`runtime.ts`](../../ui/src/modules/preview-sessions/runtime.js) mounts
[`content.html`](../../ui/legacy/content.html). Its dedicated
[`bootstrap`](../../ui/src/bootstrap/renderer.js) composes the content controller,
retained frame sessions, native surface, annotation overlay and authenticated
canvas protocol. It imports no application bootstrap and mounts no shell controls.

[`content.ts`](../../ui/src/modules/preview-sessions/content.js) derives destinations,
scenarios, lenses and labels from project data through an injected loader/catalog.
Readiness belongs to the view submitted to the frame runtime, so a delayed lookup
cannot inherit the outgoing document's readiness. Shared manifest rules preserve
authored labels/mappings while merging discovered preview scenarios. External interaction preferences do
not restart a pending or retained Storybook document.

[`renderer-bridge.ts`](../../ui/src/modules/preview-sessions/renderer-bridge.js) owns
canvas commands, metadata, capture locking, focus and keyboard forwarding through
explicit content/annotation ports. The existing frame lifecycle and annotations
remain JavaScript under `ui/legacy/`, with explicit factory initialization. Further
annotation/native modularization remains separate work.

## Acceptance and verification gaps

1. A minimal project fixture opens one application shell. Initial load, slow
   compilation, transitions and failures never request the Workbench application
   entry point or run its shell bootstrap inside a content renderer.
2. Inspect renderer documents and requests directly: no hidden Workbench shell,
   nested canvas/grid, global toolbar or application navigation exists at any point,
   including before readiness. Screenshots after CSS suppression are insufficient.
3. Verify authored HTML, compiled React/Vue previews, docs, Storybook and supported
   native adapters through the dedicated contract. Exercise cold start, failure,
   rapid supersession, size changes and close/dispose with owned fixtures.
4. Scenarios remain alternatives; only a scenario's explicit `artboards` create
   multiple views. Each independent view retains its own size, inputs,
   iteration and annotations.
5. Warm Storybook picks and returns reuse the expected content document/channel;
   assert document identity and request counts, not only elapsed time. Retained and
   pending documents cannot navigate or acknowledge a different active destination.
6. The canvas and preview cover the same logical viewport without a second grid,
   camera or decoration. Labels identify the current request on every painted frame.
7. Capture/handoff includes every intended artboard and its local annotations with
   no Workbench controls in content regions. A partial failure releases locks and
   preserves annotations; closing a renderer disposes requests and resources.
8. UI fixture-client tests and backend API tests establish independence separately
   from combined-host integration, editor hosting and staged-product launch checks.

Existing [canvas acceptance](../../scripts/smoke-canvas-ui.cjs),
[retained-session acceptance](../../scripts/smoke-preview-sessions.cjs) and controller
tests remain regression coverage. Canvas acceptance additionally observes requests
from initial navigation, rejects application documents/modules inside content,
and inspects child markup. That assertion failed against the shell-backed renderer.
Controller unit tests cover metadata precedence, delayed and superseded catalogs,
failure and disposal. Session acceptance asserts one Storybook document across lens
switches, title/story changes and parked-artboard returns, including editor acknowledgements.
This does not establish independent hosting or live device QA for every native adapter.
