# Custom adapters

Part of the [adapters](README.md). User guide:
[Custom adapters](../../docs/custom-adapters.md).

## Contract

`workbench.config.ts` registers adapters under `adapters`, each a name with:

- `runtime`: the adapter's browser mount, a project module exporting
  `mount(canvas, source, context, environment)` that returns an optional
  cleanup. It is bundled with the project's packages; Workbench installs
  nothing.
- optional `rules` and `plugins`: Rspack module rules and plugins that apply
  only to the compiler building that adapter's previews, after Workbench's
  own and the config's global ones
  ([plugins](../preview-pipeline/plugins.md#project-plugins)).

Registering a built-in name replaces that adapter, including the built-in
Astro adapter's server rendering. A definition naming an unregistered adapter
fails with `Unknown adapter`. Custom adapters build in the `web` profile and
share the entry, lifecycle and portable output of the built-ins
([previews](../previews.md#adapters-and-project-configuration)).

## Project code on the server

The adapter's loaders and plugins, which are JavaScript. The browser mount
runs only in the frame.

## Target

- The browser mount needs nothing from the server beyond compiling it, so
  it works unchanged with the Rust server.
- `rules` whose loaders are JavaScript run through the Node evaluator, one
  module at a time, as the [Vue adapter](vue.md#target) proposes.
- `plugins` cannot run in Rspack's Rust crates. How they are supported is
  the [open question](README.md#open-questions) on project plugins.

## Status

Implemented in the Node compile worker (`project.ts`, `options.ts`), covered
by [preview.test.js](../../server/node/preview.test.js) (custom adapters and
plugins on the shared path). Not implemented in Rust; the lab's prototype
builds a plain-module custom adapter without rules or plugins.
