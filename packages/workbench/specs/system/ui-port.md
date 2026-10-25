# Browser UI port

The whole browser UI, `ui/legacy/` included, moves to this layout and to
Lattice (`@canonic2/lattice`) for application state. Iteration UI and other new
interface work on the ported areas start after the port completes. This page
owns the target layout, the order of the port and its delivery. The
[navigation store](../canvas/navigation-store.md) owns the design of the first
store the port introduces; each capability's own spec keeps its behavior.
[Modules](modules.md#browser-source-layout) describes the shipping layout.

**Status:** structural moves, Lattice delivery and the bounded navigation store
are implemented under the [accepted scope](#port-order). See
[implementation gaps](#implementation-gaps).

## Target layout

```text
ui/src/
  app/<document>/           One entry per document: composition only
    workbench/              Canvas shell (the browser and panel canvas)
    sidebar/                VS Code sidebar webview
    renderer/               Content-only artboard document
    preview-host/           Document that runs one TypeScript preview
    docs/                   Docs lens reading document
    capture/                Capture document used by the screenshot helper
    viewer/                 Portable export viewer
  views/<view>/             One area of the interface
    <view>.js               The view's main element, composing its parts
    bindings.js             Connects the view's elements to capabilities
    components/<name>/      Elements only this view uses
  components/<name>/        Elements shared by several views or documents
  components/shared/        Shared modal presentation styles
  modules/<capability>/     Lattice capabilities, their rules, adapters and tests
  host/                     Transport clients shared across capabilities
  theme/                    Standalone tokens and host theme assets
  vendor/                   Pinned third-party browser assets
```

The layout follows the usual split of a web frontend: thin entry points, one
folder per area of the screen that composes it, shared presentational
elements, and capability modules that own reusable state and behavior. Folders
named `views/`, not `pages/`, because a page is a product term: one entry in a
collection.

## Roles

- **`app/<document>/`** composes one document, as Lattice's composition
  contract describes: it defines the elements the document uses, creates
  capabilities with their real ports (host clients, storage, frames), mounts
  views, binds them for the document's lifetime and ends that lifetime. It
  holds no application state and decides no product rule. The document's HTML
  lives beside its entry and loads only that entry. Served document URLs, such
  as `/_workbench/content.html` and `/_workbench/preview-host.html`, stay as
  they are; the server maps them to the files' new locations.
- **`views/<view>/`** is one area of the interface, such as the top bar or the
  page list. Its main element lays out the view's elements; `bindings.js`
  connects each element to the capabilities it shows, by hand, following
  Lattice's binding contract: selection values to inputs, element events to
  named actions, one lifetime signal. Bindings derive nothing; derived values
  are the capability's selections. A view owns no application state and
  performs no I/O. A view can be mounted by several documents: the page list
  serves the browser canvas and the sidebar webview.
- **`views/<view>/components/<name>/`** holds elements only that view uses.
  An element moves to `components/<name>/` when a second view or document uses
  it; it is not promoted in advance.
- **`components/<name>/`** holds shared elements. Every element, in a view or
  shared, follows the [Web Components contract](../ui/web-components.md) and
  has its design page.
- **`modules/<capability>/`** holds one capability: a factory that creates its
  private store and returns `state`, `select`, named actions and, when it holds
  resources, `dispose()`, as Lattice's capability contract describes. Pure
  rules and the store import no DOM or host API. DOM and host adapters the
  capability alone uses, such as frame sessions or screen capture, sit in the
  module as named adapter files; the entry point passes them in as ports.
  `index.js` is the module's public contract.
- **`host/`** holds transport clients more than one capability uses: HTTP and
  VS Code messaging. A capability-specific adapter stays in its module.

## Dependencies

| From | May import |
| --- | --- |
| `app/` | Views, components, modules' `index.js`, host clients, theme, `@canonic2/lattice/elements` |
| `views/` | Its own and shared components, `@canonic2/lattice/elements`; in `bindings.js` only, modules' `index.js` |
| `components/` | Its own helpers, shared component styles, `@canonic2/lattice/elements` |
| `modules/` | Other modules' `index.js`, `@canonic2/lattice`, browser-safe `contracts/` |
| `host/` | Browser-safe `contracts/` |

Nothing imports `app/`. Modules never import views, components or host clients;
they receive I/O as ports. Elements and element registration use Lattice's
element helpers (`@canonic2/lattice/elements`: input capture and replay,
upgrade-attribute skipping, `defineOnce`) and never the root entry point:
elements import no modules, stores or bindings. Each mutable fact has one
owning capability; other capabilities read it through that owner's `state` and
`select`, never a copy.

## Capabilities

| Module | Owns | Moves from |
| --- | --- | --- |
| `project` | The loaded workbench: spaces, collections, pages, problems, reload | `modules/project/pages.js`, `modules/project/sidebar.js`, `legacy/config.js`, configuration parts of `legacy/workbench.js` |
| `page-tree` | Lenses, scenarios and artboards of the selected page, as rules over `project` | `legacy/lenses.js`, `legacy/scenarios.js` |
| `navigation` | Requested and displayed destination, phase, address reading and writing | `modules/navigation/`, `legacy/address.js` |
| `canvas` | Artboard instances, geometry, camera and zoom, capture locks | `modules/canvas/`, `legacy/zoom.js` |
| `preview-sessions` | Canvas document: one renderer session per artboard, its transport, parking and expiry, and the shared story catalog | `modules/preview-sessions/` except `content.js`, `legacy/preview-sessions.js` |
| `content` | Renderer document: the content it shows, its frame lifecycle, readiness reports and the preview bridge client | `modules/preview-sessions/content.js`, `legacy/content-frames.js`, `legacy/preview-bridge-client.js` |
| `native-surfaces` | Renderer document: simulator and window streams and their input | `legacy/simulator.js` |
| `annotations` | Annotation tools, marks and the DOM mirror | `legacy/annotations.js`, `legacy/dom-mirror.js` |
| `review` | Capture, references, agent context, handoff and export delivery | `modules/review/`, `legacy/capture-*.js`, `legacy/reference.js`, `legacy/agent-context.js`, `legacy/describe.js`, `legacy/modern-screenshot.js` |
| `preview-controls` | Preview inputs, controls and Actions for the selected instance | `legacy/preview-controls.js`, `legacy/actions.js` |
| `shortcuts` | Keyboard shortcuts and their scoping | `legacy/keys.js` |
| `docs` | Reading layout and embedded examples | `modules/docs/` |
| `trace` | Browser timing traces | `modules/trace/` |

The table is the planned split; a capability that turns out to have no state of
its own stays a set of rules inside the module that uses it rather than a store.

A capability instance belongs to one document. The canvas document's
`preview-sessions` decides which renderer sessions exist, keeps or expires them
and shares one story catalog among them; each renderer document's `content`
owns only what happens inside its own frame. Neither keeps a copy of the
other's state: they exchange navigation identity and readiness through the
renderer messages [rendering flow](rendering-flow.md) defines.

## Views

| View | Shows | Moves from |
| --- | --- | --- |
| `page-list` | Collections, groups, pages, problems and search | `views/page-list/` (in place); sidebar host/catalog wiring in `modules/project/sidebar.js` |
| `top-bar` | Breadcrumb, lens, scenario, size and Actions controls, More menu | `views/top-bar/` presentation and control/dialog bindings in place; route/lens rules remain in `legacy/workbench.js` |
| `canvas` | The canvas element, artboard labels and loading feedback | `views/canvas/bindings.js` (in place), `modules/canvas/legacy-chrome.js` |
| `toolbar` | Annotation tools and review actions | `views/toolbar/` bindings, legacy presentation and floating-bar styles in place; renderer annotation state remains in `legacy/annotations.js` |
| `configure-pages` | The Configure pages dialog | `views/configure-pages/` (in place) |
| `export` | The export dialog | `views/export/components/export-dialog/` (in place) |
| `docs` | A docs lens's reading page | `views/docs/` (in place) |
| `viewer` | The portable viewer's interface | `app/viewer/` |

Other existing files move as follows:

- **Sizes.** Size rules and the selected size join `canvas` (the artboard
  widths it lays out), with the size bridge for the classic scripts
  (`modules/canvas/legacy-sizes.js`) folding into the canvas bindings; saving sizes to `workbench.yaml` is a
  `project` action. `size-switcher` and the `size-dialog` and `sizes-editor`
  dialogs it opens are `top-bar` elements, in `views/top-bar/components/`;
  each moves to `components/` if a second view uses it.
  `views/top-bar/register-sizes.js` becomes element definitions in the
  entries that use them.
- **Spaces.** The space list belongs to `project`. `space-switcher` and
  `space-mark` stay shared components, used by the page list in the sidebar
  and by the top bar; `components/register-space.js` becomes element
  definitions in those entries.
- **Docs on the canvas.** The docs bridge for the classic scripts
  (`modules/docs/legacy-rules.js`) and `modules/docs/canvas/` join
  the `docs` capability, with the canvas wiring in the `canvas` view's bindings.
- **Icons.** `legacy/icons.js` becomes a shared module under `components/`
  used by every view that draws icons; `legacy/canonic.svg` moves to `theme/`.
- **Styles.** Legacy CSS moves with the view or document that owns it:
  `views/page-list/page-list.css` and `app/sidebar/sidebar.css` are in place;
  `content.css` and `annotations.css` to the renderer document,
  `capture.css` to the capture document, `preview-controls.css` beside the
  preview controls' elements, and `workbench.css` to the canvas document, its
  theme tokens to `theme/`. Path consumers of a moved stylesheet follow it.
- **Injected script.** `legacy/preview-bridge.js` is not part of any Workbench
  document: it stays a standalone script served at
  `/_workbench/preview-bridge.js` and injected into proxied pages.
- **Project preview adapters.** `ui/preview/` stays outside `ui/src/`: it is
  runtime for project content, loaded by compiled previews, not Workbench UI.

## Port order

The port has two parts with different starting points:

- **Structural moves** (step 1) start now. They relocate existing code into
  the target layout and keep every existing state mechanism; they introduce no
  Lattice dependency, no store, no collection API and no navigation redesign.
- **Accepted Lattice scope.** The store, element helpers and keyed collection
  are accepted. S2 is accepted for selective selection updates. The
  [delivery slice](#lattice-delivery-slice) and the bounded navigation store
  may proceed now. Remaining windowing budgets gate the large page-list
  migration only; that investigation stays with Lattice's owner. Delivery
  introduces no stores or behavior changes, and navigation does not migrate
  large lists or introduce windowing.

1. **Structural moves,** one document at a time, each move reviewable on its
   own: the document's HTML and entry move to `app/<document>/`
   (`legacy/preview-host.js` becomes the preview-host entry,
   `legacy/capture-page.js` the capture entry), and the
   code it loads is separated into views, shared components, capability
   modules and host adapters where that code already has those roles.
   The module that connects capabilities to the existing shell is
   `views/canvas/bindings.js`, so step 2 has a canvas binding to extend; the
   `canvas` step adds the view's element. Until then the bindings also create
   the capabilities and their HTTP client, which belong to the canvas
   document's entry once the `canvas` step moves that composition there.
   No behavior changes; imports, serving, path consumers, webview resources,
   packaging and design pages follow the moved paths. Before the first move,
   the package's developer documentation names these locations, and the
   design-page check also covers `views/<view>/components/`.
2. **Navigation store.** Navigation becomes the first Lattice capability, as
   the [navigation store](../canvas/navigation-store.md) specifies; its
   [adoption](#navigation-store-adoption) lists what that step changes.
3. **Capabilities with their views,** one at a time: `project` and `page-tree`
   with `page-list`; `top-bar` with `preview-controls`; `canvas` and
   `preview-sessions` in the canvas document; `content` and `native-surfaces`
   in the renderer document;
   `annotations` and `review` with `toolbar`; `configure-pages` and `export`;
   `docs` and `viewer`.
4. **Legacy removal.** `ui/legacy/` is deleted once no consumer below uses any
   of its files, by import or by path; the UI README, the legacy README's
   schema and file sections and the package's developer documentation then
   describe the new layout.

Each step ships on its own and:

- deletes the implementation it replaces in the same change; no behavior has
  two implementations at once;
- may keep a temporary adapter where a ported capability meets code not yet
  ported, such as navigation's adapter to the legacy shell's route and sidebar
  code. Each adapter names the capability that owns it and the step that
  removes it, in this page, and holds no state of its own;
- keeps visible behavior, addresses, host messages and captures unchanged;
  public guides change only where a control visibly changes;
- moves tests with the code they cover and keeps the existing suites passing;
- passes a browser check of the affected area through the standalone server,
  plus the acceptance of every path consumer below that the step touches;
- updates this page's tables and the affected specs' paths.

Structural moves follow these conventions, each with a reason:

- **Served addresses stay.** A moved document keeps its `/_workbench/` address;
  the server's table of app documents maps it to the new file, so callers such
  as the capture helper, renderers and compiled previews change nothing. A
  document or file served by its module path under `ui/src/`, such as the Docs
  host or the docs page's script and stylesheet, has only Workbench's own
  callers; its address moves with it and every caller follows.
- **Moves are reviewable as renames.** A file is first committed moved, with
  only the edits its new path requires; formatting it to the UI's formatter
  and fixing its lint findings follow in a separate commit.
- **Tests that load a file by path keep CommonJS.** Tests of classic scripts,
  and tests that load a module's source into a VM, move as `*.test.cjs` beside
  their file. The package's Node test run (`pnpm test`) discovers them; the
  browser UI runner collects only `*.test.js`.
- **One shared preview runtime.** The preview host imports
  `/_workbench/preview-runtime.js` by its served address, as compiled
  previews do, so they share one instance; the UI module-graph check allows
  that one absolute specifier.
- **Shared legacy files stay until their capability moves.** Files a moved
  document shares with other documents or reads by path, such as
  `legacy/annotations.css`, `legacy/dom-mirror.js` and `legacy/describe.js`,
  stay at their `/_workbench/` addresses until the step that ports them.

### Navigation store adoption

- Workbench first adds Lattice as a dependency through the
  [delivery slice](#lattice-delivery-slice): the server serves its modules
  unchanged beside the UI, the canvas, renderer and sidebar entry points
  resolve its import, and VSIX packaging includes it. The server-served canvas
  and renderer documents need no CSP change for explicit-URL imports; the
  sidebar webview gains a resource root for Lattice's files, which its CSP's
  webview source then admits.
- Navigation state, transitions and the grace timer move into a navigation store
  under `ui/src/modules/navigation/`. The binding that writes the canvas's
  `inert`, `aria-busy`, visibility and loading message lives in
  `ui/src/views/canvas/bindings.js`. The canvas bootstrap's displayed and
  painted copies, and the navigation controller's flag set, are replaced by the
  store's fields and selections.
- Renderer messages carry navigation identity as the
  [navigation store](../canvas/navigation-store.md#identity-across-documents)
  specifies.
- The legacy shell keeps its route and sidebar code; an adapter at its boundary
  turns route changes into `picked` and `shown` destinations into route writes.
  The route state stays behind that adapter until the shell's route and sidebar
  code are ported; the [temporary adapters](#temporary-adapters) table names the
  step that removes it.

## Consumers by path

Some consumers read UI files by path rather than importing them. A step that
moves or replaces one of these files updates the consumer in the same change
and passes its acceptance:

| Consumer | Files it reads | Acceptance |
| --- | --- | --- |
| Server preview injection (`serve/files.rs`) | `legacy/keys.js`, `legacy/actions.js`, `legacy/scenarios.js`, read as text and injected into design pages | A served design page answers shortcuts, Actions and `?scenario=` as before |
| Preview proxy (`proxies.rs`) | `/_workbench/preview-bridge.js`, injected into proxied URL and Storybook pages | A proxied page reports readiness and takes live screenshots |
| Portable export (`portable.rs`) | `app/viewer/` files, `legacy/preview-controls.js` and `.css`, `views/docs/` and the trace module it imports | An exported archive opens from a static server and its viewer, controls and docs work |
| Compiled previews and renderers | `/_workbench/preview-host.html`, which the server maps to `app/preview-host/preview-host.html`, and `/_workbench/preview-runtime.js` | A TypeScript preview renders, re-renders a scenario in place and reloads after an edit |
| Screenshot helper (`capture/app/main.cjs`) and its Node host (`server/node/src/modules/host/`) | `/_workbench/capture.html`, which the server maps to `app/capture/capture.html`; `legacy/describe.js` read as text and run in every captured page | An Electron capture and a handoff screenshot succeed |
| Space styles (`space.rs`) | `legacy/annotations.css` read as text | Annotations render in the renderer document |
| VS Code sidebar webview | The sidebar entry through webview resource URLs | The sidebar loads under its CSP in VS Code and in the packaged extension |
| Distribution assembly | Every shipped UI file | The staged and packaged extension resolve every moved path outside the source checkout |

## Temporary adapters

| Adapter | Owner | Removed in |
| --- | --- | --- |
| Legacy shell route and sidebar to navigation's `picked`/`shown` | `navigation` | The `project`/`page-list` and `top-bar` steps, when the shell's route and sidebar code are ported |
| Legacy chrome (`modules/canvas/legacy-chrome.js`) | `canvas` | The `canvas` step |
| Classic single-artboard resize rails in `views/canvas/legacy-bindings.js` | `canvas` | The `canvas` adoption step; dimensions and persistence remain in the shell until then |
| `modules/canvas/legacy-sizes.js` and `modules/docs/legacy-rules.js` publish shared browser rules to classic consumers | `canvas` / `docs` | Their adoption steps; existing globals and ready events stay until then |
| Existing DOM controls in `views/toolbar/bindings.js`, connected by canvas bindings | `annotations` / `review` | The `annotations` / `review` adoption step; commands retain their existing state owners |
| `views/toolbar/legacy-presentation.js` supplies classic toolbar presentation to canvas bindings and the single-artboard annotation adapter | `annotations` / `toolbar` | The annotation adoption step; renderer-only annotation instances receive no toolbar |
| Classic `wbExportBindings` with `modules/review/browser-export.js` | `review` / `export` | The Export adoption step; the shell supplies its current destination and the existing request/progress lifecycle stays intact |
| Classic `wbConfigEditor` in `views/configure-pages/config-editor.js`, with rules and HTTP in `modules/project/configuration.js` | `project` / `configure-pages` | The Configure pages adoption step; draft/selection state remains in the existing dialog closure |
| Classic `wbPageList` API shared by `modules/project/pages.js` and `views/page-list/page-list.js`; sidebar startup through `wbProjectSidebar` | `project` / `page-list` | The `project`/`page-list` adoption step; existing closure state remains until then |
| Classic `wbTopBar` measurement/menu presentation and `wbTopBarPresentation` lens/scenario/source rows | `top-bar` | The `top-bar` adoption step; shell-owned scenario/lens state stays in `legacy/workbench.js` until then |
| Classic top-bar control and size-dialog bindings, and `wbPageList.bindBrowser` | `top-bar` / `page-list` | Their adoption steps; callbacks read the shell's existing state, and size writes go through `project/configuration.js` |

Later steps add a row before introducing an adapter.

## Lattice delivery slice

Before any UI code depends on Lattice, one narrow change establishes that its modules reach every Workbench document that
will import them. The slice changes module delivery only: no navigation,
canvas or other behavior changes, and no store is used yet.

**Scope:**

- **Dependency.** The product root's `package.json` declares Lattice as a local
  production dependency and keeps its own lockfile.
- **Serving.** The server serves Lattice's runtime files unchanged under a fixed
  path `/_workbench/node_modules/@canonic2/lattice/`, following the same rules as `ui/src/`:
  JavaScript modules only, tests, declarations and development files refused.
- **Imports.** Each document in scope imports Lattice's root entry point (the
  store core: `createStore`, `shallow`) and its `elements` subpath by explicit
  URL, without calling them. Explicit URLs need no import map, so documents
  without one need no inline script and no CSP change for it; the sidebar
  webview still needs a resource root for Lattice's files, which its CSP's
  webview source then admits. No export is added to Lattice for the slice.
- **Documents.** The three documents navigation's adoption needs: the canvas
  document (`app/workbench/index.html` through `app/workbench/workbench.js`) and the
  renderer document (`app/renderer/content.html` through `app/renderer/renderer.js`), served by
  the Workbench server inside the panel webview's frame, and the sidebar
  webview, which loads modules through webview resource URLs under its own
  nonce-based CSP and so needs a resource root for Lattice and an import path
  resolved within the staged extension. The preview-host, docs, capture and
  viewer documents gain the import in the port step that first uses Lattice
  there; the viewer's step also adds Lattice's files to portable exports.
- **Packaging.** Distribution assembly copies Lattice's files into the staged
  product as real files. A packaged extension must not contain a symlink or a
  path back into the source checkout.

**Evidence:**

| Check | Establishes |
| --- | --- |
| Server route test | Lattice's runtime files are served byte for byte; tests, declarations and other paths are refused |
| Browser test of the canvas and renderer documents | Both import the root entry point and `elements` without errors, from the server, not a development resolver |
| Sidebar webview check | The sidebar resolves both imports through its resource root under its CSP |
| Assembly test | The staged product contains Lattice's runtime files as real files and no symlink or source-checkout path |
| Packaged launch | The installed extension, run without the source checkout, loads all three documents and their Lattice imports |

The product uses `file:../lattice`; distribution assembly dereferences that
dependency into the staged product. Explicit relative imports address Lattice's
public root and elements entry files, resolving through the server or the
sidebar's webview resource URL without rewriting the modules.

The delivery checks cover byte-preserving serving and refusal of development
paths, a staged product outside the checkout loading canvas and renderer,
sidebar CSP loading, the browser module graph and extension resource roots.
The staged-extension VS Code acceptance checks all three document imports.

## Implementation gaps

- Legacy interface: every document is in `app/`; the code the documents load ships in the
  [current layout](modules.md#browser-source-layout), with most of the
  interface in `ui/legacy/`.
- Capability/view adoption after navigation. Large page-list adoption still
  waits for the remaining windowing budgets and scope decision.
