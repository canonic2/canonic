# Artboard system and integration

Architecture and bounded integration plan.
This topic owns module/state/transport design and the remaining integration
sequence. [Multiple artboards](multiple-artboards.md) owns product behavior and
acceptance; [modules](../system/modules.md) owns shared architectural rules.
The [product contract](multiple-artboards.md#implemented-support-system)
records support APIs, limits, verification, and remaining work. The current
implementation composes a scenario's declared artboards inside the existing canvas region,
preserving the shipping shell's markup, controls and layout.

The [rendering-flow contract](../system/rendering-flow.md) is authoritative: one application
shell, dedicated content-only renderers, no shell bootstrap or CSS-hidden shell
inside an artboard. The dedicated content entry point implements that boundary.

## Actual module boundaries

| Modules | Ownership |
| --- | --- |
| `ui/src/bootstrap/canvas.js` | A scenario's declared artboards, selected-instance commands, preview controls, review and publication in the existing shell; scenarios remain alternatives. |
| `ui/src/components/canvas/element.js` | Keyed artboard shells, labels, resizing and shared camera; no chrome or server operations. |
| `ui/src/modules/canvas/model.js`, `geometry.js` | Serializable state and pure geometry/size policy. |
| `ui/src/modules/canvas/controller.js` | Sole state writer, instance IDs/generations, runtime creation/replacement/disposal, park/resume outside visible state, capture locks. |
| `ui/src/modules/navigation/controller.js` | Requested destination, pending transitions, supersession, retry and shutdown invalidation. |
| `ui/src/modules/preview-sessions/controller.js` | Ordinary-renderer retention, expiry, declared-artboard activation and cleanup, catalog sharing and refresh. |
| `ui/src/modules/preview-sessions/story-catalog.js` | Canvas-owned catalog cache and authenticated renderer requests; concurrent lookup sharing, retry and cleanup. |
| `ui/src/modules/preview-sessions/runtime.js` | Per-iframe authenticated messaging, request deadlines, capture responses and cleanup. |
| `ui/src/modules/preview-sessions/content.js` | Content selection, model-derived metadata, generation/readiness ownership and injected configuration/catalog access. |
| `ui/src/modules/preview-sessions/renderer-bridge.js` | Authenticated content commands, capture locks, metadata and input forwarding; no application chrome. |
| `ui/legacy/content-frames.js` | Factory owning warm content documents, Storybook channel switching, frame promotion, retention and disposal. |
| `ui/src/modules/review/capture.js`, `reference.js` | Immutable full-canvas capture composition and pure reference/context derivation. |
| `ui/src/modules/review/controller.js`, `browser.js` | Capture/handoff sequencing, locks and partial failure; HTTP and download resources behind browser ports. |
| `ui/src/modules/canvas/publication.js` | Serialized participating-space publication, shared-root discovery policy, heartbeats and withdrawal. |
| `server/src/serve/routes/canvas.rs` | Registered-space, fixed-operation server access. |
| `contracts/src/agent-context/report.js` | Versioned context validation without silent text truncation. |
| `server/src/handoff/canvas.rs` | Per-region handoff sections using the existing local annotation formatter. |
| `capture/src/native-streams/pool.ts` | Independent source/codec producers, subscriber ownership and idle cleanup. |

Keep project preview compilation and pixel acquisition behind their existing
contracts. The content-only entry point composes annotation/capture factories through
explicit ports and retains ordinary content sessions and independent artboard state.
The capability map and contracts
below describe that target direction. New canvas state and its lifecycle have
one controller owner; renderer handles never enter serializable state.

Tests live with these capabilities. `scripts/smoke-canvas.cjs` exercises real
Chrome and the packaged capture runtime through a temporary programmatic harness,
including full handoff persistence. `smoke-canvas-ui.cjs` exercises the production
shell, a scenario's declared artboards, ordinary scenario switching, input/size/annotation isolation, preview
controls and composite native handoff. See the implementation record for actual native-host and
performance verification limits.

## Standards and runtime constraints

- New and substantially reworked capabilities live under
  `ui/src/modules/<capability>/` as browser-native JavaScript ES modules with
  explicit `.js` imports. Registration and lifecycle start through entry points;
  importing a component does not register or mount it. Node-only hosts retain
  their separate TypeScript contract.
- UI asset tooling/delivery is host infrastructure, independent of project
  compiler configuration and lifecycle. Serve or copy authored JavaScript/CSS
  unchanged, with no UI build, type stripping or bundling. Node-only
  adapters are never browser imports. Preserve existing JS assets needed by injected
  previews, portable viewers, and file-based use until their separate delivery
  paths support any migration.
- Use the existing Node runner, pnpm scripts, DOM/CSS vocabulary, and manifest
  rules. Application state moves to Lattice (`@canonic2/lattice`) under the
  agreed [browser UI port](../system/modules.md#browser-source-ownership); this
  capability adds no other framework, new root package, dependency-injection
  container, event bus, or repository-wide migration.
- Organize by capability. A directory owns coherent behavior, types, tests,
  and lifecycle; create separate files only when responsibilities justify them.
  The boundaries below are substantive, not a target file count.

## Future integration capability map

Presentation of a scenario's declared artboards and existing-shell composition are implemented.
The map below also covers deeper extraction and future artboard-management UI.

Locations are relative to `packages/workbench/` and come from the agreed
[target layout](../system/modules.md#browser-source-ownership), which owns
them; the actual module boundaries above retain current paths until the port
moves them. Ownership and dependency constraints are the contract.

| Capability | Target location | Responsibility and public API |
| --- | --- | --- |
| Canvas model | `ui/src/modules/canvas/` | View descriptors, IDs, canvas snapshot, validated pure command transitions; `createCanvasState`, `applyCanvasCommand`, `validateCanvasDescriptor` |
| Canvas geometry | `ui/src/modules/canvas/` | Bounds, automatic placement, CSS-pixel camera math and Fit operations; pure functions, no DOM |
| Canvas workflows | `ui/src/modules/canvas/` | Sole owner of mutable canvas state; add/select/replace/close/reload/refresh; coordinates runtimes and emits snapshots |
| Content-only renderer | `ui/src/modules/content/`, `ui/src/modules/native-surfaces/`, `ui/src/app/renderer/` | Dedicated viewport, model metadata, frame sessions, native streams and annotation/capture ports; no application embedding |
| Canvas presentation | `ui/src/views/canvas/` | Keyed artboard shells, labels/status, selected outline, focus, pan/zoom and resize input; emits commands, never performs space I/O |
| Content runtimes | `ui/src/modules/preview-sessions/` | Renderer contracts and owned instances; mount/update/readiness, source verification, capture acquisition, disposal |
| Space access | `ui/src/modules/project/`, `ui/src/host/` | Public space-scoped request contract, registry lookup and host/server dispatch; uses existing space services rather than a second registry |
| Annotation state/UI | `ui/src/modules/annotations/` | Per-artboard annotations, pure annotation edits and drawing interactions; capture/prompt logic moves out of annotation presentation |
| Canvas review | `ui/src/modules/review/` | Immutable capture manifest, full-canvas reference/context derivation, acquisition workflow, review result and errors |
| Capture composition | `ui/src/modules/review/` | Pure output geometry and helper/renderer orchestration; uses existing capture engine and native pixel acquisition |
| Agent publication | `ui/src/modules/review/` | Versioned report validation/formatting and participating-space publication; adapts existing server view service and agent context reader |
| Browser/host entry points | `ui/src/app/<document>/` and existing server/extension entry points | Compose concrete dependencies and dispose the application; transport registration remains with its owning host |

Persistence is a conditional capability under `ui/src/modules/canvas-storage/`, created
only after its product policy is agreed. It receives descriptors through an
explicit port; it never serializes renderer handles or observes DOM to infer
state. Phase 2 extends geometry/interactions for free placement and Arrange;
it does not introduce a second artboard model.

Dependency direction: presentation calls controller commands; the controller
uses the pure model and public runtime/space APIs. Review reads one snapshot
and requests runtime acquisition. External adapters implement these APIs.
Pure modules do not import controllers, DOM, Node, VS Code, HTTP, or globals.
Presentation receives view models; it cannot inspect another module's maps,
iframe cache, or private state. Composition is explicit at entry points.

## State and types

The following is an API sketch, not a source file or settled wire schema:

```ts
type ArtboardId = string;
type SpaceId = string;

interface ViewTarget {
  spaceId: SpaceId;
  src: string;
  scenario: string | null;
  artboard?: string;           // declared artboard ID; absent for an implicit artboard
  iteration: string | null;    // selected iteration of that artboard
  lens: string | null;
}

interface ArtboardDescriptor {
  id: ArtboardId;
  target: ViewTarget;
  size: { width: number; height: number; mode: string };
  position: { x: number; y: number };
}

type RenderStatus =
  | { kind: 'loading'; generation: number; requested: ViewTarget }
  | { kind: 'ready'; generation: number; rendered: ResolvedView }
  | { kind: 'replacing'; generation: number;
      rendered: ResolvedView; requested: ViewTarget }
  | { kind: 'error'; generation: number; requested: ViewTarget;
      rendered: ResolvedView | null; problem: ArtboardProblem };

interface CanvasSnapshot {
  id: string;
  revision: number;
  artboards: readonly ArtboardSnapshot[];
  selectedId: ArtboardId | null;
  camera: { x: number; y: number; scale: number };
}
```

`ResolvedView`, `ArtboardSnapshot`, and `ArtboardProblem` belong to these
capabilities: resolved space/source/story/lens/code pointers, descriptor plus
render status and annotation snapshot, and structured error with operation,
space, artboard, and preserved cause respectively. Resolved view metadata is
transport-neutral; it contains no DOM or renderer handles. Story IDs and
ephemeral URLs come from resolution rather than replacing source identity.

- The controller is the only writer of serializable canvas/artboard state.
  A pure transition returns new state and explicit workflow effects; it runs
  no asynchronous work. Runtime completion is a typed input to the controller.
  Keep command/effect variants limited to real workflows.
- Runtime instances own frame nodes, pending loads, decoders, subscriptions,
  timers, and controls/application state inside content. They report events;
  they do not independently mutate selection or catalog state. The controller
  records reported readiness/status, not a duplicate runtime implementation.
- Annotations are owned per artboard through annotation transitions; drawing
  widgets own only transient drag/focus/menu state. Apply edits through controller
  commands so review reads a consistent snapshot.
- The catalog is space-owned shared data; renderer instances refer to it.
  UI labels, top bar availability, selected view, and context text are derived
  from snapshots rather than stored as competing copies.
- Parse external values as `unknown`. Validate space/source identities,
  finite bounded geometry, supported sizes, unique IDs, and selection
  membership. Types never replace request/message validation. Persisted schema
  versions and single-view address adaptation have explicit parsers.

## Runtime and transport contracts

```ts
interface ArtboardRuntime {
  update(view: ResolvedView, generation: number,
    signal: AbortSignal): Promise<void>;
  acquireCapture(signal: AbortSignal): Promise<AcquiredSnapshot>;
  dispose(): Promise<void>;
}

interface RuntimeFactory {
  mount(input: RuntimeInput, callbacks: RuntimeCallbacks): ArtboardRuntime;
}
```

`RuntimeInput` supplies the mounting element, artboard ID, space access,
and resolved content capabilities. `RuntimeCallbacks` reports readiness,
problems, controls, focus, navigation, and captured source revisions with
artboard ID and generation. Register concrete factories at bootstrap, selected
by resolved renderer kind. No lookup through `window.wbView`, global
`iframe.is-active`, or hidden import initialization in new code.

Iframe runtime owns active/pending frames and warm-session reuse within one
artboard. Separate artboards cannot share a mutable iframe, even for identical
URLs or stories. A canvas-owned retention budget accounts for hidden sessions;
the runtime owns their disposal. Visible instances cannot be evicted as idle
merely because another instance is selected. Reuse existing preview-host
mount/reset and bridge APIs behind explicit adapters; do not rewrite compiler
or framework mounts for this feature.

Native runtime owns decoder/client subscriptions, not the device/window itself.
The renderer's stream client (`ui/legacy/simulator.js`) moves to the
`native-surfaces` capability of the [target layout](../system/modules.md#capabilities).
The producer pool, `capture/src/native-streams/pool.ts`, is keyed by
validated source and codec, with reference-counted subscriptions. Closing the
last subscriber stops owned capture resources; other devices/codecs/subscribers
continue. Simulator input is space/device-qualified. Window runtime exposes
view-only capabilities. Acquisition returns the last valid decoded frame with
its timestamp; no frame means an explicit unavailable capture.

Space access has explicit operations for resolving a target, loading catalogs,
opening sources, config editing, native input, review storage, and context
publication. Separate read-only resolution from mutations. In browser hosting,
the canvas server coordinates calls to space services through the existing
registry; in VS Code the wrapper relays space-qualified UI commands to its
host services. Do not create a general arbitrary-URL HTTP proxy. Each dispatch
verifies the known space and configured source and keeps current origin/path
checks. Surface server, startup, and trust failures per space/artboard.

## How workflows execute

1. **Add or duplicate:** validate the descriptor, allocate a stable ID, place it,
   and create a loading artboard. Resolve against its space and mount its
   renderer. Completion updates that instance's generation, then publishes the
   canvas snapshot. Duplication mounts a fresh instance; it cannot clone native
   external state.
2. **Select and edit:** selection changes the model only; existing DOM/runtime
   instances remain keyed by ID. Derive top bar/catalog state and notify the
   host. Size/lens/scenario edits target a named artboard, not a global frame.
3. **Navigate or reload:** increment that instance's generation, cancel its old
   pending work, and retain its settled runtime until the replacement settles.
   Outgoing pixels follow [navigation feedback](../system/rendering-flow.md#navigation-feedback):
   visible only under their outgoing identity during the grace period, and
   never after the replacement settles or fails.
   Ignore superseded completions/messages. Navigation from content routes to
   its own space/artboard and does not steal focus after a later user selection.
4. **Refresh/remove space:** invalidate space resolution, reconcile only
   dependent artboards, and cancel affected loads. Preserve other instances.
   Removed sources/spaces become explicit problems/placeholders. Update
   participating-space context subscriptions after reconciliation.
5. **Capture/handoff:** create an immutable manifest from one model revision,
   including every artboard and its annotations. Acquire renderer snapshots
   using bounded concurrency and short-lived capture leases. A renderer cannot be
   disposed or updated halfway through acquisition; defer its resource change
   until acquisition finishes, then release the lease in `finally`. Closing may
   remove UI immediately while the lease retains owned capture resources.
   Compose acquired pixels with pure geometry, save through the agreed host
   destination, then build one prompt from that manifest and stored images.
   Each acquisition verifies its render generation against the frozen manifest;
   a view changed before its lease was obtained fails explicitly. Timeouts and
   cancellation cannot wait indefinitely for uncooperative content: adapters
   have bounded acquisition and forced owned-resource cleanup paths.
   On failure/cancel, release all leases and temporary output; report named
   failures, never claim success for omitted ready artboards. Loading/error
   placeholder policy and image limits follow the implemented support contract;
   memory budgets and any additional output formats remain delivery decisions.
6. **Context:** derive a complete versioned record from the same snapshot and
   publish it under one canvas ID to participating space services. Include
   each space's artboards and all other participants, selected ID, and actual
   versus requested views. Host/user activity ordering is distinct from load
   completion/heartbeat ordering. Retain complete textual output for agent
   integrations; validate size limits rather than truncating.
7. **Successful handoff annotations:** preserve current clear-after-handoff
   behavior only for annotations included in the completed review. A
   concurrent new annotation must survive; failures/cancellation clear none.
   Use stable annotation IDs and captured annotation revisions. If the
   product instead keeps notes after handoff, record that interaction decision
   before changing this policy.
8. **Close canvas:** cancel workflows, stop context publication and withdraw its
   records, dispose view/listeners, and await runtime cleanup. Release subscriptions
   and leases without stopping services owned by other canvases or spaces.
   Log cleanup failures with cause and identity; observe every asynchronous task.

Resource creation APIs must document their cancellation point and return an
owned disposer/subscription where applicable. Creation that finishes after
cancellation disposes its result instead of attaching it. Reference counts are
updated once per subscription; disposal is idempotent. Diagnostics carry canvas,
artboard, space, operation and generation IDs with bounded errors, not page
HTML, images, or full handoff prompts.

## Renderer acceptance

1. The browser regression inspects initial requests and child documents,
   not just visible pixels. It must fail when an artboard requests Workbench's
   application entry point or runs its shell bootstrap, even if CSS later hides it.
2. Content resolution/mounting and per-instance annotation/capture use explicit ports.
   Renderer state and metadata come from those models, not hidden shell controls.
3. `createRuntime` loads the dedicated content entry point. Preserve typed commands,
   origin/source checks, readiness identity and disposal.
4. Keep the warm ordinary renderer/content cache and Storybook channel alive across
   page/lens changes within their bounds; separate artboards keep separate documents.
5. Child documents contain no shell startup/markup, duplicate canvas/controls or
   CSS chrome suppression. Verify every adapter, transition, size, failure, capture and editor
   relay against [rendering-flow acceptance](../system/rendering-flow.md#acceptance-and-verification-gaps).
   Preserve the outer UI layout. Do not substitute a UI language/build rewrite.

## Remaining delivery sequence

| Slice | Integration work | Acceptance evidence |
| --- | --- | --- |
| Artboard management UI | Design adding, removing and choosing arbitrary artboards beyond composing a scenario's declared artboards; preserve the existing shell and one-view behavior | Accessible repeated-view workflows, keyboard/focus, geometry and cleanup |
| Space and native host acceptance | Verify the implemented owning-space routing and pooled native subscriptions on real hosts | Mixed-space and shared-root recovery, real native capture/input, independent subscriptions |
| Review and context host acceptance | Verify the integrated camera, handoff, reference and participating-space publication paths in supported editors | Complete composite/prompt, hooks and MCP, race/failure cleanup |
| Product completion | Resolve arrangement storage/sharing, recovery, host limits and public guides; single-view restoration follows [persistence](persistence.md) | Phase 1 acceptance in browser and VS Code |
| Phase 2 arrangement | Extend geometry and interactions for free positioning and row/grid placement | Stable renderer state, capture mapping and accessible movement |

The browser host serves authored JavaScript/CSS bytes through
[files.rs](../../server/src/serve/files.rs). The sidebar
loads its module through a VS Code resource URL; portable docs copy their modules.
No UI compiler or bundler participates. Preserve native module resolution, CSP,
resource roots and packaging while maintaining the content-only rendering boundary.

## Verification plan

- UI-owned unit tests next to model, geometry, annotation, report, and
  composition modules: invalid descriptors, command invariants, selection,
  bounds, ordering, complete reports, and coordinate mapping.
- Workflow tests with controlled async renderer/transport boundaries: superseded
  loads, no cross-instance mutation, capture leases, close/cancel/reload races,
  full/failed review output, space reconciliation, and cleanup errors.
- Server/host tests exercise real space fixtures, origins, source validation,
  unchanged JavaScript/CSS serving, runtime ESM loading, view publication, and review storage.
  Extend existing capture/preview/window/space test boundaries; replace
  source-slice tests only where the extracted public API replaces that behavior.
- Real browser and VS Code validation: keyboard/iframe focus, external lenses,
  responsive sizing, simultaneous native streams and source sharing, mixed
  spaces, composition fidelity, process/subscription cleanup, and packaging.
  Unit stubs cannot establish those host/resource contracts.
- Package checks: `pnpm --dir packages/workbench test` and
  `pnpm --dir packages/workbench run check`. Context reader changes also run
  that reader's tests. Website guide/UI changes use its test/build and
  screenshot workflow. Finish with `git diff --check` and relevant untracked
  file review. Record platform/prerequisite limits separately from passing
  coverage; measure memory, input responsiveness, and capture size/time before
  adopting limits. Never invent performance pass thresholds from source review.

The implemented support system's evidence and host limitations are recorded in
[multiple artboards](multiple-artboards.md#implemented-support-system).
Future integration requirements above do not establish host acceptance.
