# Vue adapter

Part of the [adapters](README.md). User guide: [Vue](../../docs/vue.md).

## Source

A preview's `source` is a Vue 3 single-file component or a component module;
`inputs` are its props. Vue comes from the project.

## Build

- `.vue` files go through Vue Loader with Workbench's bundled
  `@vue/compiler-sfc`, so the project need not install the compiler.
  Development builds enable its hot reload. `VueLoaderPlugin` is applied in
  every compiler.
- Style blocks go through `style-loader` and `css-loader`, because Vue
  Loader's style proxy does not work with Rspack's native CSS
  ([finding 7](../../labs/rspack-pipeline/README.md#findings)). A
  `<style module>` block's class names are its default export, read as
  `$style`.
- Vue's feature flags are set through `define`.
- Script blocks compile with the shared SWC loader. It builds in the `web`
  profile.

## Project code on the server

Vue's single-file-component compiler and Vue Loader are JavaScript. They run
in the compiler process today.

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
  per-component IDs. The candidate is Vize's SFC compiler
  (`vize_atelier_sfc`, published on crates.io, actively released), whose
  README calls it experimental and expects behavior that diverges from Vue.
  It replaces Workbench's bundled `@vue/compiler-sfc` one for one: the
  compiler is Workbench's, not the project's, and compiled Vue 3 output uses
  Vue's stable runtime helpers. It is adopted only if a lab shows render
  parity and hot-update behavior matching `@vue/compiler-sfc`.
- **Otherwise, the Node evaluator:** a loader hands each `.vue` file to it,
  where `@vue/compiler-sfc` returns the script, template and style modules.

Either way, the rest of the build (SWC, CSS, chunks, hot updates) is native,
and Vue's hot-reload runtime is browser code and is unaffected.

## Status

Implemented in the Node compile worker (`pipeline.ts`, `portable.ts`),
covered by [preview.test.js](../../server/node/preview.test.js) with project
Vue versions. Not implemented in Rust.

## Open questions

- **Vize's conformance,** licence and what its dependencies add to the
  server binary, measured in a lab against `@vue/compiler-sfc`:
  `<script setup>`, scoped and module styles, slots, and hot updates.
- **The Node step's cost,** one round trip per `.vue` file and per edit, if
  Vize does not pass.
- **Style blocks under native CSS.** With its own loader, the Rust server
  could hand style blocks to Rspack's native CSS instead of `style-loader`
  and `css-loader`.
