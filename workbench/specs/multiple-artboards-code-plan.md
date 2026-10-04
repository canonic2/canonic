# Multiple artboards: code and migration plan

Architecture and bounded migration record, 2026-10-04.
The [product contract](multiple-artboards.md#implemented-support-system--2026-10-04)
records support APIs, limits, verification, and remaining work. The current
implementation is infrastructure only; it does not change the shipping UI.

## Actual module boundaries

| Modules | Ownership |
| --- | --- |
| `src/canvas/model.ts`, `geometry.ts` | Serializable state and pure geometry/size policy. |
| `src/canvas/controller.ts` | Sole state writer, instance IDs/generations, runtime creation/replacement/disposal, capture locks. |
| `src/canvas/runtime.ts` | Per-iframe authenticated messaging, request deadlines, capture responses and cleanup. |
| `src/canvas/legacy-runtime.ts` | Authenticated command/data adapter for the existing shell/annotation ports, without chrome or style changes. |
| `src/canvas/review.ts` | Complete reference/context derivation and immutable full-canvas capture composition. |
| `src/canvas/publication.ts` | Serialized participating-space publication, shared-root discovery policy, heartbeats and withdrawal. |
| `src/spaces/coordinator.ts` | Registered-space, fixed-operation server access. |
| `src/agent-context/report.ts` | Versioned context validation without silent text truncation. |
| `src/canvas-review/prompt.ts` | Per-region handoff sections using the existing local annotation formatter. |
| `src/native-streams/pool.ts` | Independent source/codec producers, subscriber ownership and idle cleanup. |

This migration keeps the existing renderer, preview compiler, annotation
engine, and acquisition engine behind narrow ports. Singleton state is isolated by
renderer instance. Their deeper TypeScript extraction is architectural follow-up;
it was not expanded into a package-wide rewrite. The capability map and contracts
below describe that target direction. New canvas state and its lifecycle have
one controller owner; renderer handles never enter serializable state.

Tests live with these capabilities. `scripts/smoke-canvas.cjs` exercises real
Chrome and the packaged capture runtime through a temporary programmatic harness,
including full handoff persistence. Production controls and layout are unchanged. See the implementation record for actual native-host and
performance verification limits.

## Standards and runtime constraints

Follow the source [Workbench guide](../../../.canonic/guides/workbench/AGENTS.md),
[module architecture](../../../.canonic/rules/module-architecture.md),
[UI contracts](../../../.canonic/rules/ui-contracts.md), and
[behavioral testing](../../../.canonic/rules/behavioral-testing.md).
Use [develop-module](../../../.codex/skills/develop-module/SKILL.md) for bounded
implementation, review-module for boundary/lifecycle review, create-ui-component
for presentation, and write-tests for behavioral verification.

- New and substantially reworked capabilities live under `src/` as TypeScript
  ES modules. Use strict checking, erasable syntax, explicit `.ts` imports, and
  `import type`. No enums, namespaces, parameter properties, top-level await,
  or import-time effects. Existing CommonJS hosts load synchronous ESM modules
  under the package's Node 24 contract; asynchronous work starts through APIs.
- Browser TypeScript is stripped and served as JavaScript by the local server,
  with no maintained compiled copy. Node-only adapters are never reachable
  through browser imports. Preserve existing JS assets needed by injected
  previews, portable viewers, and file-based use until their separate delivery
  paths support any migration.
- Use the existing Node runner, pnpm scripts, DOM/CSS vocabulary, and manifest
  rules. This capability does not require a framework, new root package,
  dependency-injection container, event bus, or repository-wide migration.
- Organize by capability. A directory owns coherent behavior, types, tests,
  and lifecycle; create separate files only when responsibilities justify them.
  The boundaries below are substantive, not a target file count.

## Future integration capability map

Presentation, annotation UI extraction, and host composition below are
future integration work, outside the current infrastructure-only scope.

Suggested paths are relative to `packages/workbench/`. Names may be adjusted
to fit concurrent work; ownership and dependency constraints are the contract.

| Capability | Proposed files | Responsibility and public API |
| --- | --- | --- |
| Canvas model | `src/canvas/model.ts` | View descriptors, IDs, canvas snapshot, validated pure command transitions; `createCanvasState`, `applyCanvasCommand`, `validateCanvasDescriptor` |
| Canvas geometry | `src/canvas/geometry.ts` | Bounds, automatic placement, CSS-pixel camera math and Fit operations; pure functions, no DOM |
| Canvas workflows | `src/canvas/controller.ts` | Sole owner of mutable canvas state; add/select/replace/close/reload/refresh; coordinates runtimes and emits snapshots |
| Canvas presentation | `src/canvas/view.ts`, `src/canvas/interactions.ts` | Keyed artboard shells, labels/status, selected outline, focus, pan/zoom and resize input; emits commands, never performs space I/O |
| Content runtimes | `src/artboards/runtime.ts`, `src/artboards/iframe.ts`, `src/artboards/native.ts` | Renderer contracts and owned instances; mount/update/readiness, source verification, capture acquisition, disposal |
| Space access | `src/spaces/access.ts`, `src/spaces/host.ts` | Public space-scoped request contract, registry lookup and host/server dispatch; uses existing space services rather than a second registry |
| Annotation state/UI | `src/annotations/model.ts`, `src/annotations/view.ts` | Per-artboard annotations, pure annotation edits and drawing interactions; capture/prompt logic moves out of annotation presentation |
| Canvas review | `src/canvas-review/model.ts`, `src/canvas-review/controller.ts` | Immutable capture manifest, full-canvas reference/context derivation, acquisition workflow, review result and errors |
| Capture composition | `src/capture/composition.ts`, `src/capture/service.ts` | Pure output geometry and helper/renderer orchestration; uses existing capture engine and native pixel acquisition |
| Agent publication | `src/agent-context/report.ts`, `src/agent-context/publisher.ts` | Versioned report validation/formatting and participating-space publication; adapts existing server view service and Shield reader |
| Browser/host entry points | `src/canvas/bootstrap.ts`, existing server/extension entry points | Compose concrete dependencies and dispose the application; transport registration remains with its owning host |

Persistence is a conditional capability under `src/canvas-storage/`, created
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
  state: string | null;
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
  finite bounded geometry, supported viewport modes, unique IDs, and selection
  membership. Types never replace request/message validation. Persisted schema
  versions and legacy address adaptation have explicit parsers.

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
Move the substantially changed stream manager into TypeScript under a coherent
`src/native-streams/` capability. Its server-side producer pool is keyed by
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
   host. Size/lens/state edits target a named artboard, not a global frame.
3. **Navigate or reload:** increment that instance's generation, cancel its old
   pending work, and keep its settled preview visible until replacement is ready.
   Ignore superseded completions/messages. A problem preserves any usable
   outgoing view under its actual identity. Navigation from content routes to
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
   placeholder policy and output limits require the product decisions.
6. **Context:** derive a complete versioned record from the same snapshot and
   publish it under one canvas ID to participating space services. Include
   each space's artboards and all other participants, selected ID, and actual
   versus requested views. Host/user activity ordering is distinct from load
   completion/heartbeat ordering. Retain complete textual output for Shield
   consumers; validate size limits rather than truncating.
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

## Delivery and migration sequence

Each slice remains reviewable and verified. Intermediate one-artboard slices
are migration steps, not a release that narrows the agreed phase 1 scope.

| Slice | Code change | Evidence before moving on |
| --- | --- | --- |
| 1. Browser delivery | Establish restricted `src/` module serving with stripped types, JavaScript MIME, safe path validation and caching; import browser bootstrap; verify VSIX carries `.ts` sources and `src/package.json` | Real server serves/imports nested `.ts` modules; Node and VS Code can load host modules; package file inspection |
| 2. Single-artboard ownership | Extract model, geometry, controller, and iframe runtime from shell globals; connect the existing top bar/host via temporary explicit adapters | Existing navigation, spare-frame readiness, annotations, capture, controls, and source behavior preserved |
| 3. Independent instances | Key artboard DOM by ID, per-instance generations/controls/notes, automatic placement, common camera and retention budget | Duplicate previews/stories stay independent; resizing/focus and eviction/disposal verified |
| 4. Spaces and native content | Space-aware host access and catalog routing; multi-source native pool and per-instance subscriptions | Two spaces sharing roots or source names resolve correctly; native sources/codecs coexist in real host runs |
| 5. Full-canvas review/context | Capture manifest/leases/composition and bundle storage; versioned complete context publication and Shield compatibility | Mixed-renderer capture, race/failure cleanup, all-artboard prompt and hooks/MCP verified |
| 6. Product completion | Resolve chosen persistence and legacy links, recovery, limits, guides and website refresh | Phase 1 acceptance criteria pass; no remaining renderer/space exclusions hidden as future work |
| 7. Phase 2 | Free positioning and row/grid arrangement in existing geometry/interactions modules | Layout changes preserve runtime state, camera, annotations, capture mapping and accessible movement |

The current server's `serveFile` does not establish browser TypeScript stripping;
the guide's delivery direction is a prerequisite to verify/implement, not assumed
infrastructure. Serve only the intended browser dependency graph under a fixed
Workbench namespace; never expose Node adapters or arbitrary repository files.
Preserve extension packaging and forwarded URL behavior. A file-based canvas and
portable preview viewer do not have this server: define their support/delivery
explicitly before removing legacy assets they use.

Existing `workbench.js`, `zoom.js`, `annotations.js`, `simulator.js`, controls, and
reference/context scripts become bounded adapters or have their affected
responsibilities migrated. Remove obsolete global listeners/state when their
owner moves; do not run old and new loading/capture pipelines together.
Portable preview controls are a separate consumer: retain a compatible entry
point or explicitly adapt its own delivery before removing the shared script.
Manifest validation, capture fidelity, compiler/framework adapters, and catalog
export remain existing capabilities, reused through their public boundaries.

Keep Shield integration changes in `shield/src/`. Test canonical code first;
regeneration/rollout into distributed copies requires its own authorized scope.
Do not alter the other session's runtime files as part of writing this plan.

## Verification plan

- Pure TypeScript tests next to model, geometry, annotation, report, and
  composition modules: invalid descriptors, command invariants, selection,
  bounds, ordering, complete reports, and coordinate mapping.
- Workflow tests with controlled async renderer/transport boundaries: superseded
  loads, no cross-instance mutation, capture leases, close/cancel/reload races,
  full/failed review output, space reconciliation, and cleanup errors.
- Server/host tests exercise real space fixtures, origins, source validation,
  `.ts` serving, runtime ESM loading, view publication, and review storage.
  Extend existing capture/preview/window/space test boundaries; replace
  source-slice tests only where the extracted public API replaces that behavior.
- Real browser and VS Code validation: keyboard/iframe focus, external lenses,
  responsive sizing, simultaneous native streams and source sharing, mixed
  spaces, composition fidelity, process/subscription cleanup, and packaging.
  Unit stubs cannot establish those host/resource contracts.
- Package checks: `pnpm --dir packages/workbench test` and
  `pnpm --dir packages/workbench run check`. Shield reader changes also run
  `node --test shield/src/integrations/workbench-context.test.mjs` and any
  affected pipeline tests. Website guide/UI changes use its test/build and
  screenshot workflow. Finish with `git diff --check` and relevant untracked
  file review. Record platform/prerequisite limits separately from passing
  coverage; measure memory, input responsiveness, and capture size/time before
  adopting limits. Never invent performance pass thresholds from source review.

This planning-only change validates links and Markdown diffs. It does not run
behavior tests, demonstrate module serving, or establish runtime acceptance.
