# Workbench Web Components

## Purpose

Workbench's own browser interface uses Web Components as its target component
architecture. Components ship under `packages/workbench/ui/src/components/<name>/`;
the agreed [target layout](../system/ui-port.md) also places elements only one
view uses under that view's `components/`. They work
in the VS Code sidebar, the embedded canvas, and the standalone browser canvas.
This contract concerns the tool's interface, not the framework used by pages it
previews. It does not change the preview adapters or require projects to adopt
Web Components.

**Status:** the space switcher and space mark (sidebar, breadcrumb), the size
switcher with its Custom size and Edit sizes dialogs, the Actions switch, the
canvas presentation and the export dialog are implemented under this contract,
each with its design page; the existing shell composes them. Migrating the
remaining interface surfaces, which use HTML, JavaScript and shared CSS, and the
port to the target layout (`ui/src/views/`, Lattice bindings) are agreed but not
started. See [evidence and implementation gaps](#evidence-and-implementation-gaps).

Related contracts: [themes](themes.md),
[core behavior](../core.md), and [extension hosting](../integrations/vscode-extension.md).

## Component structure

The UI follows the required [backend-independence contract](../system/modules.md#ui-and-backend-independence).
The host loads browser-executable UI assets and injects a data/operation client.
Components, page-list presentation and artboard rendering do not require the
application backend or project compiler to initialize. Generic host asset delivery
is permitted. Artboards must follow the [content-only rendering flow](../system/rendering-flow.md),
not initialize another copy of the application.

The agreed [target layout](../system/ui-port.md) keeps
shared components under `ui/src/components/`, elements used by one area of the
interface under its `ui/src/views/<view>/components/`, and capability state and
operations under `ui/src/modules/<capability>/`. Components receive inputs and emit events across
that boundary; they do not own capability I/O or application state.

- Use autonomous custom elements extending `HTMLElement`, with `wb-` tag names
  such as `wb-space-switcher`. Expose deliberate properties and event contracts.
  Author browser-native JavaScript and CSS. Hosts serve/copy these modules unchanged;
  no TypeScript, transpilation, bundling or generated implementation is required.
- Give each component a named folder, with its element implementation, owned
  styles, and meaningful behavioral tests; in the target layout, `ui/src/views/<view>/components/<name>/` when one view
  uses it and `ui/src/components/<name>/` once a second view or document does.
  Put component helpers beside their owner; extract shared
  behavior only when it has consumers.
- Use open Shadow DOM for owned markup and styles. Slots expose intentionally
  supplied content; documented `::part` names expose only deliberate styling
  surfaces. Hosts and neighboring components do not query or modify internals.
- Start with native Web Component APIs. A rendering library or a generic base
  class is not required; adopting one needs a separate, scoped design decision.
- Keep registration separate from the exported class so importing a class does
  not implicitly register all components. Host entry points register the elements
  they use once per document. Repeated bootstrap must not throw or silently replace
  an incompatible definition.
- Keep browser classes and registration out of extension/server imports. Pure
  helpers can be tested in Node without creating a fake browser runtime.

The HTML entry points compose components and load their registration. Feature
controllers supply application state and handle user requests. A folder move is
not sufficient: each migrated component must stop depending on document-wide IDs,
global mutable APIs, or another component's private DOM.

## Inputs, events, and ownership

Each component documents its properties, attributes, defaults, events, slots,
parts, and interaction states. Use properties for structured data and attributes
for simple declarative values. Define which attributes reflect properties;
presence-based boolean attributes must follow HTML semantics.

Inputs may arrive before connection or custom-element upgrade. Preserve those
values, handle later updates, and do not mutate caller-owned data. Application
selection and pending/error state belong to the controller; focus, hover, and
temporary interaction state belong to the component. Avoid mirrored application
state that can drift away from its owner. Components do not subscribe to stores.
In the [target layout](../system/ui-port.md), a
capability's binding supplies their inputs, following Lattice's state management;
outside it, their controller sets them.

Emit named, typed custom events from the host for user intent. Events intended
for ancestor controllers bubble and are composed; document their detail payload
and cancellation behavior. Setting an input or rendering a theme update must not
emit a user-action event. Components do not call VS Code APIs, perform server
requests, persist configuration, start processes, or navigate spaces themselves.
Their controller handles those operations and supplies the resulting state.

## Lifecycle and interaction

Construct the owned shadow tree once. Acquire connection-scoped listeners,
observers, timers, and subscriptions in `connectedCallback`; release them in
`disconnectedCallback`. Reconnecting the same element must work without duplicate
listeners. Guard asynchronous work against disconnection or replacement.

Update the affected DOM rather than replacing the entire shadow tree on every
input. Preserve focus, text entry, selection, open controls, and meaningful scroll
where the update permits it. Multiple instances operate independently.

Use native buttons, inputs, and semantic structures inside the component. A custom
element is not automatically an accessible button. Accessible names and label
relationships must work across the chosen shadow boundary; do not assume an
external `for` or `aria-labelledby` reference reaches an internal control.
Implement the widget's keyboard model, visible focus, disabled and pending states,
and focus restoration. Follow the shared UI contracts for menus and dialogs.
Account for event retargeting; use public events and `composedPath()` where needed
rather than making document listeners depend on private internal targets.

All shell components consume the [theme contract](themes.md). Component styles
must load under both the VS Code webview's CSP and an independent browser host.
Do not introduce a CDN or browser-side compilation. The UI owns any asset build;
backend domain services do not decide or initialize UI presentation.

## Migration and delivery

Migrate one bounded control and its consumers at a time. Preserve existing routing,
host messaging, preview isolation, and capture behavior. Compatibility adapters
belong at the existing host boundary, not inside the reusable element.

Browser asset serving must cover intended component and theme assets without
exposing server modules or tests. Check direct webview resource URLs/CSP, local server imports, type
checking, and VSIX inclusion. Use the same implementation in `design/` examples;
update examples that rely on reaching into private DOM to use public inputs
or actual interaction. Do not maintain a parallel mock component.

## Design pages

Every element under `ui/src/components/<name>/`, and under
`ui/src/views/<view>/components/<name>/` once the
[target layout](../system/ui-port.md) is ported, has a docs page in this
repository's **Workbench** space, under **Components**. The page ships in the
same change as the element, and changes with it: a new or changed property,
attribute, event, part, keyboard behavior, or visible state updates the page in
that change. A component is not done without it.

Pages are grouped by the area of the interface the component appears in, the
title's middle segment: **Sidebar**, **Top bar**, **Canvas**, and as components
arrive there, **Toolbar** and **View controls**, the regions
[core](../core.md) names. A component used in more than one area goes where it
mainly lives; its page says where else it appears.

A page is three files in `packages/workbench/design/`, named after the
component folder:

| File | Holds |
| --- | --- |
| `<name>.workbench.ts` | A `defineDocs` definition with `id: 'components/<name>'`, `title: 'Components/<Area>/<Label>'`, `icon: 'component'`, and one `docs` lens (`adapter: 'html'`) whose styles are `../ui/src/theme/defaults.css` and `./design.css`. The space discovers it; `workbench.yaml` needs no entry. |
| `<name>.md` | What the control is and where it appears, which module owns it and which controller owns its state, one section per meaningful state with an `example` block and a caption naming the inputs it shows, and a closing **Public API** section. |
| `<name>.examples.ts` | One named export per example. Each renders the shipping element through its public inputs, and stands in for the controller by applying what the element asks for, so the example responds as the canvas would. |

The **Public API** section lists each property with its default, the attributes
and which reflect, the events with their `detail` and whether they bubble and
compose, the styling parts and custom properties, and the keyboard model.

Shared helpers in `design/` register the elements and supply sample data and
icons (`sizes-ui.ts`, `workbench-ui.ts`, `icons.ts`). Examples import only what
they draw: each docs page compiles its examples on first view, so a whole
icon set or library in a helper slows every component page.

A page is verified by opening it in the Workbench space: every example mounts,
icons draw, and the console has no errors. Interaction tests live beside the
element, not in the page.

## Acceptance criteria

- The same element mounts in a standalone browser and the appropriate VS Code
  webview; it requires no VS Code globals or project styles.
- Two instances have independent state and interactions. Inputs before and after
  connection/upgrade render correctly without emitting action events.
- A controller can observe the documented events without querying the shadow
  tree. Rendering cannot trigger host operations on its own.
- Disconnecting and reconnecting leaves no active connection-owned resources or
  duplicate handlers. Updates preserve usable focus and text entry.
- Real browser checks establish keyboard behavior, names, focus, disabled/pending
  states, and Shadow DOM behavior. Node tests cover pure logic and host integration
  where appropriate; screenshots supplement interaction verification.
- The component follows theme acceptance criteria and is served and packaged with
  its styles. Existing affected host and canvas tests remain green.
- Its [design page](#design-pages) lists it under **Components** in the
  Workbench space, shows each meaningful state with the shipping element, and
  documents its current public API.

## Evidence and implementation gaps

- **Unmigrated surfaces.** Interface surfaces other than the components below
  use HTML, JavaScript and shared CSS.
- **Target layout.** Every shipping element is in `ui/src/components/<name>/`;
  no `ui/src/views/` folder or Lattice binding exists yet.
- [Size switcher](../../ui/src/components/size-switcher/element.js),
  [Custom size](../../ui/src/components/size-dialog/element.js),
  [Edit sizes](../../ui/src/components/sizes-editor/element.js), and the
  [Actions switch](../../ui/src/components/actions-switch/element.js) are registered for the
  canvas only, by [workbench.js](../../ui/src/bootstrap/workbench.js);
  the sidebar's module registers the space controls. Their pure form and row
  logic is tested in Node beside each element.
- Each element requires its [design page](#design-pages), a `defineDocs`
  definition in [design/](../../design/size-switcher.workbench.ts);
  [design-pages.test.js](../../ui/src/components/design-pages.test.js) fails an
  element folder under `ui/src/components/` (it reads no other folder) without
  the three files, the `components/<name>` ID, or a
  Public API section. It checks the page exists, not that it is current: a
  changed API without an updated page is a review finding.
- [Space switcher](../../ui/src/components/space-switcher/element.js) and
  [space mark](../../ui/src/components/space-mark/element.js) use the same contract.
  Their [public API and examples](../../design/space-switcher.md) document properties,
  events, parts and keyboard behavior. Application operations remain in the
  existing sidebar and canvas controllers.
- [Canvas](../../ui/src/components/canvas/element.js) owns keyed artboard shells,
  labels and camera presentation; [export dialog](../../ui/src/components/export-dialog/element.js)
  owns export form presentation. Capability controllers retain application
  state, rendering and I/O. Their colocated browser tests cover the component
  contracts; importing the classes does not register them.
- Component modules and shared theme CSS are delivered as host assets under
  [asset and packaging checks](../system/modules.md#asset-and-packaging-checks):
  [files.rs](../../server/src/serve/files.rs) serves them unchanged, excluding
  tests, declarations, TypeScript and server modules, and the direct sidebar
  webview loads the same JavaScript registration entry through a resource URL,
  authorized by host-owned CSP and local resource roots. Owned styles use
  adopted stylesheets.
- [Browser component tests](../../ui/src/components/space-switcher/element.test.js)
  exercise source loading and nonce bootstrap, upgrade timing, events, keyboard
  focus, reconnect cleanup, defaults and token overrides. They require Chrome;
  real VS Code theme/host acceptance remains a separate manual check.
- [MDN custom elements](https://developer.mozilla.org/en-US/docs/Web/API/Web_components/Using_custom_elements)
  and [Shadow DOM](https://developer.mozilla.org/en-US/docs/Web/API/Web_components/Using_shadow_DOM)
  describe the platform APIs. VS Code webviews run browser content under their
  own resource and security policies; see the
  [webview guide](https://code.visualstudio.com/api/extension-guides/webview).
