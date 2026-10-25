# Adapters

An adapter is everything Workbench needs to render one technology's pages:
how its sources are compiled, what of it runs as project code on the
server, and how it mounts in the preview frame. A preview definition names
its adapter (`adapter: 'react'`), and so does each `docs` lens. The user
guides have one page per adapter, listed in
[Workbench previews](../../docs/workbench-previews.md); these specs hold
each adapter's contract, how it is built and what remains open.

[TypeScript previews](../pages/previews.md) owns what a preview is: definitions,
scenarios, controls, data, readiness and errors, the same for every adapter.
The [preview pipeline](../preview-pipeline/README.md) owns compiling,
serving, warm documents and hot updates. An adapter spec owns only what
differs for its technology, and links to those topics for the rest.

**Status:** The [serving model](#how-pages-are-served) is implemented in
`workbench-server`'s preview compiler ([compiler.rs](../../server/src/compiler.rs))
and the Node evaluator; each adapter spec states its own status at its top.
Adapters as separate units, an agreed [decision](#decisions), are not
implemented ([gaps](#gaps)).

## Adapters

| Adapter | Spec | Compiles with | Project code on the server | Mounts with |
| --- | --- | --- | --- | --- |
| `html` | [HTML](html.md) | Workbench's HTML loader, SWC for its scripts | None | [html.js](../../ui/preview/html.js) |
| `react` | [React](react.md) | Rspack's built-in SWC loader, React Refresh | None | [react.js](../../ui/preview/react.js) |
| `react-native-web` | [React Native Web](react-native-web.md) | As React, in the `react-native-web` profile | None | [react.js](../../ui/preview/react.js) (`mountNative`) |
| `vue` | [Vue](vue.md) | Workbench's Vue loaders, with its bundled `@vue/compiler-sfc` | Vue's compiler (JavaScript) | [vue.js](../../ui/preview/vue.js) |
| `astro` | [Astro](astro.md) | The project's Astro compiler | Compiling, frontmatter and rendering, through Astro's container | [astro.js](../../ui/preview/astro.js), then `html.js` |
| Storybook | [Storybook](storybook.md) | The project's Storybook, with its own builder | Storybook's dev server | Storybook's own `iframe.html` |
| A registered name | [Custom adapters](custom.md) | The project's Rspack rules and native plugin capabilities | The project's JavaScript loaders | The project's `runtime` module |

Docs is not an adapter: it is a lens of a page whose examples render through
one ([docs](../pages/docs-pages.md)). A preview definition is not one either: it
describes a page and names the adapter that renders its source.

## What every adapter supplies

Adapters render project content, not Workbench application chrome. Their browser
mount runs in a [content-only renderer](../system/rendering-flow.md); none may use
Workbench's application `index.html` as its artboard entry point. This leaves
project compilation/evaluation/server rendering in the backend where required.

| Part | Where it runs | What it is |
| --- | --- | --- |
| Build settings | The Workbench server's compiler | Rules, loaders, resolution and defines for its sources, and the profile it builds in ([compiler](../preview-pipeline/compiler.md#one-compiler-per-project-and-profile)) |
| Project-code step | The Node evaluator, only when the adapter has one | Work that runs JavaScript from the project or its framework, such as server rendering or a JavaScript compiler |
| Browser mount | The preview frame | `mount(canvas, source, context, environment)`, returning a cleanup. [browser.js](../../ui/preview/browser.js) runs the shared lifecycle around it |
| Edit behavior | The frame's hot client | What an edit to each kind of source does ([hot updates](../preview-pipeline/hot-updates.md#edit-behavior)) |
| Docs examples | The docs lens | What an example export is for the adapter ([docs](../pages/docs-pages.md)) |
| Portable output | The production compiler | What the [portable build](../pages/previews.md#portable-build-and-cli) carries for its pages |

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
| React, React Native Web, HTML | Native compilation and serving | No adapter-specific evaluation; definitions/configuration can still need JavaScript |
| Vue | Everything but compiling single-file components; a Rust compiler takes that over only if one passes conformance ([target](vue.md#target)) | Workbench's bundled `@vue/compiler-sfc`, for each `.vue` file |
| Astro | Rendered HTML, client scripts and styles | Renders: startup, edits, input changes ([target](astro.md#target)) |
| Storybook | Index and story requests, proxied | Storybook's dev server, started and owned by the server on a local socket ([how](storybook.md#a-storybook-the-server-runs)) |
| Custom adapters | The browser mount, native plugin capabilities and Rust-native rules | The project's JavaScript loaders |

Storybook's dev server is a separate owned child, not an operation inside the
Node evaluator. The server's
[processes and connections](../system/server.md#processes-and-connections) own
every child process: the Node evaluator (one per space, not one per adapter),
the definition-index evaluator and Storybook's dev server, their lifetimes,
deadlines and sockets.

- **An adapter declares which parts are Rust and which are Node,** so adding
  an adapter does not change the server's core.

The [glossary](../glossary.md#behind-the-canvas) defines the Workbench
server, the page index, adapters and the Node evaluator.
"Runtime" alone names neither the server nor an adapter: the preview runtime
is the browser module every frame shares ([browser.js](../../ui/preview/browser.js)),
and the bundled runtime is the Electron build Workbench ships
([capture](../review/capture.md#bundled-electron-runtime)).

## Decisions

Decided by the owner on 2026-10-16, after the
[instant first page comparison's stage 2](../../research/instant-first-page.md#stage-2-2026-10-16):

- **The Workbench server is one Rust runtime:** HTTP, the page index and
  the compiler (Rspack's Rust crates) in one binary. Node runs only as the
  Node evaluator, for code that has to be JavaScript: computed definitions,
  `workbench.config.ts`, Vue's compiler, Astro rendering and project JavaScript
  loaders. Project plugin capabilities migrate to Rust
  ([decision](../preview-pipeline/plugins.md#plugin-migration-decision)).
  A project that needs none of the JavaScript steps starts no Node process.
  The server's [processes and connections](../system/server.md#processes-and-connections)
  list the Node processes beside the evaluator.
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

## Gaps

- **Adapters are not separate units.** Format handling is shared code: the
  profiles and Workbench's rules are in [options.rs](../../server/src/compiler/options.rs),
  the adapter-to-module map in [project.rs](../../server/src/compiler/project.rs),
  Vue in [vue.rs](../../server/src/compiler/vue.rs), Astro in
  [astro.rs](../../server/src/compiler/astro.rs) and the evaluator's
  [astro.ts](../../server/node/src/modules/evaluator/astro.ts), and project
  plugin capabilities in [plugins.rs](../../server/src/compiler/plugins.rs).
  Where each adapter should live is an [open question](#open-questions).

## Open questions

- **The cost of the Node evaluator's loader step,** per transformed module,
  for Vue and project loaders. It is measured before Vue's target is fixed.
- **Where each adapter lives in the code:** a module per adapter, spanning
  the Rust server, the Node evaluator and `ui/preview/`, under the
  [module layout](../system/modules.md).
