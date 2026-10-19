# Adapters

An adapter is everything Workbench needs to render one technology's pages:
how its sources are compiled, what of it runs as project code on the
server, and how it mounts in the preview frame. A preview definition names
its adapter (`adapter: 'react'`), and so does each `docs` lens. The user
guides have one page per adapter, listed in
[Workbench previews](../../docs/workbench-previews.md); these specs hold
each adapter's contract, how it is built and what remains open.

[TypeScript previews](../previews.md) owns what a preview is: definitions,
states, controls, data, readiness and errors, the same for every adapter.
The [preview pipeline](../preview-pipeline/README.md) owns compiling,
serving, warm documents and hot updates. An adapter spec owns only what
differs for its technology, and links to those topics for the rest.

## Adapters

| Adapter | Spec | Compiles with | Project code on the server | Mounts with |
| --- | --- | --- | --- | --- |
| `html` | [HTML](html.md) | Workbench's HTML loader, SWC for its scripts | None | [html.js](../../ui/preview/html.js) |
| `react` | [React](react.md) | Rspack's built-in SWC loader, React Refresh | None | [react.js](../../ui/preview/react.js) |
| `react-native-web` | [React Native Web](react-native-web.md) | As React, in the `react-native-web` profile | None | [react.js](../../ui/preview/react.js) (`mountNative`) |
| `vue` | [Vue](vue.md) | Vue Loader with `@vue/compiler-sfc` | Vue's compiler (JavaScript) | [vue.js](../../ui/preview/vue.js) |
| `astro` | [Astro](astro.md) | The project's Astro compiler | Compiling, frontmatter and rendering, through Astro's container | [astro.js](../../ui/preview/astro.js), then `html.js` |
| Storybook | [Storybook](storybook.md) | The project's Storybook, with its own builder | Storybook's dev server | Storybook's own `iframe.html` |
| A registered name | [Custom adapters](custom.md) | The project's Rspack rules and plugins | The project's loaders and plugins | The project's `runtime` module |

Docs is not an adapter: it is a lens of a page whose examples render through
one ([docs](../docs-pages.md)). A preview definition is not one either: it
describes a page and names the adapter that renders its source.

## What every adapter supplies

Adapters render project content, not Workbench application chrome. Their browser
mount runs in a [content-only renderer](../rendering-flow.md); none may use
Workbench's application `index.html` as its artboard entry point. This leaves
project compilation/evaluation/server rendering in the backend where required.

| Part | Where it runs | What it is |
| --- | --- | --- |
| Build settings | The Workbench server's compiler | Rules, loaders, resolution and defines for its sources, and the profile it builds in ([compiler](../preview-pipeline/compiler.md#one-compiler-per-project-and-profile)) |
| Project-code step | The Node evaluator, only when the adapter has one | Work that runs JavaScript from the project or its framework, such as server rendering or a JavaScript compiler |
| Browser mount | The preview frame | `mount(canvas, source, context, environment)`, returning a cleanup. [browser.js](../../ui/preview/browser.js) runs the shared lifecycle around it |
| Edit behavior | The frame's hot client | What an edit to each kind of source does ([hot updates](../preview-pipeline/hot-updates.md#edit-behavior)) |
| Docs examples | The docs lens | What an example export is for the adapter ([docs](../docs-pages.md)) |
| Portable output | The production compiler | What the [portable build](../previews.md#portable-build-and-cli) carries for its pages |

Built-in and custom adapters meet the same contract: a custom adapter's
`runtime` is its browser mount, and its `rules` and `plugins` are its build
settings.

An adapter needs only the parts its pages use. It may produce a page in
any of three ways, and the contract assumes no more than that a page loads
in a preview frame at a size:

- **compiled by the Workbench server** into modules a mount renders, as
  React, React Native Web, HTML and Vue are;
- **rendered on the server** by project code in the Node evaluator, then
  served, as Astro is;
- **served by a server the adapter runs**, framed through the Workbench
  server, as Storybook is.

Pages that are not components or markup, such as a WebGL canvas for an
artwork, are adapters too. The contract must not assume a component model,
a DOM tree to mount into, or Workbench's compiler.

## How pages are served

Every request goes through the Workbench server, which hands it to the
page's adapter. The adapter serves what it can from Rust, and uses the Node
evaluator only for the parts that have to run JavaScript:

| Adapter | Served by the Rust server | Node evaluator |
| --- | --- | --- |
| React, React Native Web, HTML | Everything | Not started |
| Vue | Everything, with a Rust compiler that passes conformance ([target](vue.md#target)) | Vue's own compiler, only where the Rust one does not pass |
| Astro | Rendered HTML, client scripts and styles; saved renders on a warm start | Renders: first open, edits, input changes ([target](astro.md#target)) |
| Storybook | Index and story requests, proxied | Storybook's dev server, started and owned by the server on a local socket ([how](storybook.md#a-storybook-the-server-runs)) |
| Custom adapters | The browser mount, and Rust-native rules | The project's JavaScript loaders and plugins |

- **One Node evaluator per space,** not one per adapter. The server starts
  it on first need, with the space's trust and configuration; it loads only
  what the adapters in use need, restarts when `workbench.config.ts`
  changes, and exits with the server. A space that needs no JavaScript on
  the server never starts it.
- **An adapter declares which parts are Rust and which are Node,** so adding
  an adapter does not change the server's core.
- A child process of any kind, the Node evaluator or Storybook's dev server,
  is the Workbench server's: started on need, reached only by the server over
  stdin and stdout pipes or a local socket (a Unix socket, or a named pipe on
  Windows) and never on a TCP port, and stopped with it. A process the user
  runs on any port never conflicts with it
  ([processes and connections](../server.md#processes-and-connections)).

The [glossary](../glossary.md#behind-the-canvas) defines the Workbench
server, the page index, adapters and the Node evaluator. Project code never
runs in the server itself, the extension host or the capture renderer.
"Runtime" alone names neither the server nor an adapter: the preview runtime
is the browser module every frame shares ([browser.js](../../ui/preview/browser.js)),
and the bundled runtime is the Electron build Workbench ships
([capture](../capture.md#bundled-electron-runtime)).

## Decisions

Decided by the owner on 2026-10-16, after the
[instant first page lab's stage 2](../../labs/instant-first-page/README.md#stage-2-report):

- **The Workbench server is one Rust runtime:** HTTP, the page index and
  the compiler (Rspack's Rust crates) in one binary. Node runs only as the
  Node evaluator, for code that has to be JavaScript: computed definitions,
  `workbench.config.ts`, Vue's compiler, Astro rendering and project loaders
  and plugins. A project that needs none of them starts no Node process.
- **Each adapter is a separate unit** that owns its build settings,
  project-code step, browser mount and documentation, instead of having its
  format handling spread through the pipeline.
- **Astro and React Native Web must be supported by the Rust server,** or the
  move does not go ahead. React Native Web compiles natively with SWC. Astro
  compiles and renders in the Node evaluator with the project's own Astro
  compiler, Rust from Astro 7, and the server builds its client side
  natively. HTML and React compile natively.
- **Everything goes through the Workbench server and an adapter;** Rust
  serves directly where it can, and a Node child process runs where an
  adapter needs one ([how pages are served](#how-pages-are-served)). Astro,
  Vue and Storybook move to this model.
- **The adapter format stays open** to kinds of page Workbench does not
  render yet ([what every adapter supplies](#what-every-adapter-supplies)).
- **Storybook is an adapter;** its pages are declared in `workbench.yaml`,
  with no catalog import ([Storybook adapter](storybook.md#decisions)).
- The name stays **adapter**, as in the `adapter` key and the user guides.

## Status

Implemented in the Node compile worker
([server/node/src/modules/preview-pipeline/](../../server/node/src/modules/preview-pipeline)):
every adapter in the table. Its format handling is shared code rather than
separate units: React Native Web's profile is in `options.ts`, Vue's and
HTML's rules in the shared rule list, Astro's rendering in
[astro.cjs](../../server/node/preview/astro.cjs), and the adapter-to-module
map in `project.ts`. That worker listens on a loopback TCP port, which the
target's children do not ([processes and connections](../server.md#processes-and-connections)).
A Storybook the server runs is implemented in the Node server as the target
describes, on a local socket ([Storybook](storybook.md#status)).

The Rust server is not implemented. The lab's prototype compiles React and
plain-module pages only ([stage 2 report](../../labs/instant-first-page/README.md#stage-2-report)).

## Open questions

- **Project plugins in a Rust compiler.** A project plugin is a JavaScript
  object with `apply(compiler)` that uses `compiler.rspack`
  ([plugins](../preview-pipeline/plugins.md#project-plugins)). Rspack's Rust
  crates cannot call it. Loaders can run in the Node evaluator, one module at
  a time; plugins that hook the compilation cannot. Decide whether project
  plugins keep their current form, become loaders only, or keep a Node
  compiler for projects that use them.
- **The cost of the Node evaluator's loader step,** per transformed module,
  for Vue and project loaders. It is measured before Vue's target is fixed.
- **Where each adapter lives in the code:** a module per adapter, spanning
  the Rust server, the Node evaluator and `ui/preview/`, under the
  [module layout](../modules.md).
