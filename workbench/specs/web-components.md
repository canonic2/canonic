# Workbench Web Components

## Purpose and status

Workbench's own browser interface uses Web Components as its target component
architecture. Components live under `packages/workbench/src/components/` and work
in the VS Code sidebar, the embedded canvas, and the standalone browser canvas.
This contract concerns the tool's interface, not the framework used by pages it
previews. It does not change the preview adapters or require projects to adopt
Web Components.

**Status (2026-10-04): first component migration implemented.** The space switcher
and its decorative space mark use this contract in the sidebar, breadcrumb and
design examples. The size switcher and its two dialogs, Custom size and Edit
sizes, follow it in the canvas top bar ([sizes](sizes.md)), as does the Actions switch beside it. The rest of the interface remains legacy HTML, JavaScript and
shared CSS. Related contracts: [themes](themes.md),
[core behavior](core.md), and [extension hosting](vscode-extension.md).

## Component structure

The [module layout contract](modules.md) keeps shared components under
`src/components/` and capability controllers and operations under
`src/modules/<capability>/`. Components receive inputs and emit events across
that boundary; they do not own capability I/O or application state.

- Use autonomous custom elements extending `HTMLElement`, with `wb-` tag names
  such as `wb-space-switcher`. Write erasable TypeScript ES modules with explicit
  `.ts` imports, following the package's existing source and serving conventions.
- Give each component a named folder under `src/components/`, with its element
  implementation, owned styles, and meaningful behavioral tests. Put component
  helpers beside their owner; extract shared behavior only when it has consumers.
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
state that can drift away from its owner.

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
must load under both the VS Code webview's CSP and the standalone server. Do not
introduce a CDN, browser-side compilation, or a maintained compiled source copy.

## Migration and delivery

Migrate one bounded control and its consumers at a time. Preserve existing routing,
host messaging, preview isolation, and capture behavior. Compatibility adapters
belong at the existing host boundary, not inside the reusable element.

Before shipping the first component, extend the browser asset serving allowlist
to cover the intended component and theme assets without exposing server modules
or tests. Check direct webview resource URLs/CSP, local server imports, type
checking, and VSIX inclusion. Use the same implementation in `design/` examples;
update legacy examples that rely on reaching into private DOM to use public inputs
or actual interaction. Do not maintain a parallel mock component.

## Design pages

Every element under `src/components/<name>/` has a docs page in this
repository's **Workbench** space, under **Components**. The page ships in the
same change as the element, and changes with it: a new or changed property,
attribute, event, part, keyboard behavior, or visible state updates the page in
that change. A component is not done without it.

Pages are grouped by the area of the interface the component appears in, the
title's middle segment: **Sidebar**, **Top bar**, and as components arrive
there, **Canvas**, **Toolbar**, and **View controls**, the regions
[core](core.md) names. A component used in more than one area goes where it
mainly lives; its page says where else it appears.

A page is three files in `packages/workbench/design/`, named after the
component folder:

| File | Holds |
| --- | --- |
| `<name>.workbench.ts` | A `defineDocs` definition with `id: 'components/<name>'`, `title: 'Components/<Area>/<Label>'`, `icon: 'component'`, and one `docs` lens (`adapter: 'html'`) whose styles are `../src/theme/defaults.css` and `./design.css`. The space discovers it; `workbench.yaml` needs no entry. |
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

- [Size switcher](../src/components/size-switcher/element.ts),
  [Custom size](../src/components/size-dialog/element.ts),
  [Edit sizes](../src/components/sizes-editor/element.ts), and the
  [Actions switch](../src/components/actions-switch/element.ts) are registered for the
  canvas only, by [canvas-bootstrap.ts](../src/components/canvas-bootstrap.ts);
  the sidebar's bundle registers the space controls. Their pure form and row
  logic is tested in Node beside each element.
- Each of the six elements has its [design page](#design-pages), a `defineDocs`
  definition in [design/](../design/size-switcher.workbench.ts);
  [design-pages.test.ts](../src/components/design-pages.test.ts) fails an
  element folder without the three files, the `components/<name>` ID, or a
  Public API section. It checks the page exists, not that it is current: a
  changed API without an updated page is a review finding.
- [Space switcher](../src/components/space-switcher/element.ts) and
  [space mark](../src/components/space-mark/element.ts) are the first elements.
  Their [public API and examples](../design/space-switcher.md) document properties,
  events, parts and keyboard behavior. Application operations remain in the
  existing sidebar and canvas controllers.
- [browser-modules.ts](../src/server/browser-modules.ts) serves component modules
  and shared theme CSS with the existing type-stripping route, excluding tests
  and server modules. [webview-components.ts](../src/server/webview-components.ts)
  bundles the same registration entry in memory with the packaged compiler for
  the direct sidebar webview. A per-document CSP nonce authorizes that script.
  No compiled source copy is maintained. Owned styles use adopted stylesheets.
- [Browser component tests](../src/components/space-switcher/element.test.ts)
  exercise source loading and nonce bootstrap, upgrade timing, events, keyboard
  focus, reconnect cleanup, defaults and token overrides. They require Chrome;
  real VS Code theme/host acceptance remains a separate manual check.
- [MDN custom elements](https://developer.mozilla.org/en-US/docs/Web/API/Web_components/Using_custom_elements)
  and [Shadow DOM](https://developer.mozilla.org/en-US/docs/Web/API/Web_components/Using_shadow_DOM)
  describe the platform APIs. VS Code webviews run browser content under their
  own resource and security policies; see the
  [webview guide](https://code.visualstudio.com/api/extension-guides/webview).
