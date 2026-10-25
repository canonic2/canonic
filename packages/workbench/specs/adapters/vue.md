# Vue adapter

Part of the [adapters](README.md). User guide: [Vue](../../docs/vue.md).

**Status:** Implemented through the Node evaluator path of the
[target](#target): native loaders in [vue.rs](../../server/src/compiler/vue.rs)
and [loaders.rs](../../server/src/compiler/loaders.rs), with
`@vue/compiler-sfc` in the evaluator
([vue.ts](../../server/node/src/modules/evaluator/vue.ts)), shared by live and
portable builds. A native compiler is conditional on a lab that has not been
run; Vize is not adopted ([open questions](#open-questions)).

## Source

A preview's `source` is a Vue 3 single-file component or a component module;
`inputs` are its props. Vue comes from the project.

## Build

- `.vue` files go through Workbench's native Vue loader
  ([vue.rs](../../server/src/compiler/vue.rs)), which hands each file to the
  Node evaluator. There Workbench's bundled `@vue/compiler-sfc` returns the
  script with its template and each style block as CSS, so the project need
  not install the compiler. Portable builds inline the template into
  `<script setup>`; live builds keep it a separate render function, as Vue
  Loader does in development, so a template edit can re-render alone.
- Style blocks are Rspack's native CSS, emitted as stylesheets of the build
  rather than injected by JavaScript: scoped by the component's ID, or a CSS
  module for `<style module>`, whose class names are read as `$style`.
- Development builds register each component with Vue's hot-reload runtime
  (`__VUE_HMR_RUNTIME__`). An update whose script, style flags and CSS
  variables are unchanged calls its `rerender`; any other update calls
  `reload`.
- Vue's feature flags are set through `define`.
- Script blocks compile with the shared SWC loader. It builds in the `web`
  profile.

## Project code on the server

Vue's single-file-component compiler is JavaScript. It runs in the Node
evaluator, one round trip per `.vue` file and per edit
([what runs in the Node evaluator](../preview-pipeline/compiler.md#what-runs-in-the-node-evaluator)).

## Browser

[vue.js](../../ui/preview/vue.js) creates an app rendering the component
with its inputs, inside the environment's `wrap`, runs the environment's
`configure(app, context)`, and mounts it. An error during mounting fails the
mount; one after it goes to `context.error`, then to any error handler the
environment set. Cleanup unmounts the app.

## Edits

A template edit re-renders through Vue's hot reload with state kept; a
script edit re-creates the component, resetting its state as Vue defines;
a style block is replaced in place
([hot updates](../preview-pipeline/hot-updates.md#edit-behavior)).

## Target

Vue moves to the [serving model](README.md#how-pages-are-served): native
where it can be, the Node evaluator where it must.

- **Native, if a Rust compiler passes.** SWC alone is not enough: a
  single-file component's template becomes a render function, `<script
  setup>`'s macros and bindings are Vue's own transform, scoped styles add
  `data-v-*` attributes to selectors and markup, and hot reload needs
  per-component IDs. Vize's SFC compiler (`vize_atelier_sfc`) is a candidate
  for investigation, not an adopted or verified dependency. A replacement
  would replace Workbench's bundled `@vue/compiler-sfc` one for one: the
  compiler is Workbench's, not the project's, and compiled Vue 3 output uses
  Vue's stable runtime helpers. It is adopted only if a lab shows render
  parity and hot-update behavior matching `@vue/compiler-sfc`.
- **Otherwise, the Node evaluator:** a loader hands each `.vue` file to it,
  where `@vue/compiler-sfc` returns the script, template and style modules.

Either way, the rest of the build (SWC, CSS, chunks, hot updates) is native,
and Vue's hot-reload runtime is browser code and is unaffected.

## Verification

[preview.test.js](../../server/node/preview.test.js) covers project Vue
versions and scoped styles; the Rust unit tests in
[vue.rs](../../server/src/compiler/vue.rs) cover the loader.

## Open questions

- **Vize's conformance,** licence and what its dependencies add to the
  server binary, measured in a lab against `@vue/compiler-sfc`:
  `<script setup>`, scoped and module styles, slots, and hot updates.
- **The Node step's cost,** one round trip per `.vue` file and per edit,
  is unmeasured.
