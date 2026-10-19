# Workbench shell and content rendering

Status: content-only renderer implemented. Browser acceptance checks document
requests and child markup as well as warm sessions, labels and review behavior.
Independent UI/backend hosting acceptance remains separate work.

This topic owns the shell/content boundary. [Modules](modules.md) owns UI/backend
independence; [core](core.md) owns selection, [readiness](preview-pipeline/readiness.md)
owns render completion, and [multiple artboards](multiple-artboards.md) owns instances.

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
- Navigation revokes the outgoing destination before publishing new metadata.
  Old pixels may remain only while identified as the outgoing view; they cannot
  appear beneath the next destination's label. The shell owns transition status;
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

[`runtime.ts`](../ui/src/modules/preview-sessions/runtime.js) mounts
[`content.html`](../ui/legacy/content.html). Its dedicated
[`bootstrap`](../ui/src/bootstrap/renderer.js) composes the content controller,
retained frame sessions, native surface, annotation overlay and authenticated
canvas protocol. It imports no application bootstrap and mounts no shell controls.

[`content.ts`](../ui/src/modules/preview-sessions/content.js) derives destinations,
states, lenses and labels from project data through an injected loader/catalog.
Readiness belongs to the view submitted to the frame runtime, so a delayed lookup
cannot inherit the outgoing document's readiness. Shared manifest rules preserve
authored labels/mappings while merging discovered preview states. External interaction preferences do
not restart a pending or retained Storybook document.

[`renderer-bridge.ts`](../ui/src/modules/preview-sessions/renderer-bridge.js) owns
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
4. Ordinary states remain alternatives; only a page's explicit `artboards` create
   multiple views. Each independent view retains its own size, inputs and annotations.
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

Existing [canvas acceptance](../scripts/smoke-canvas-ui.cjs),
[retained-session acceptance](../scripts/smoke-preview-sessions.cjs) and controller
tests remain regression coverage. Canvas acceptance additionally observes requests
from initial navigation, rejects application documents/modules inside content,
and inspects child markup. That assertion failed against the shell-backed renderer.
Controller unit tests cover metadata precedence, delayed and superseded catalogs,
failure and disposal. Session acceptance asserts one Storybook document across lens
switches, title/story changes and parked-artboard returns, including editor acknowledgements.
This does not establish independent hosting or live device QA for every native adapter.
