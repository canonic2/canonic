# Custom adapters

Part of the [adapters](README.md). User guide:
[Custom adapters](../../docs/custom-adapters.md).

**Status:** Implemented natively in `workbench-server`'s compiler
([project.rs](../../server/src/compiler/project.rs),
[options.rs](../../server/src/compiler/options.rs),
[plugins.rs](../../server/src/compiler/plugins.rs)), with JavaScript loaders
in the Node evaluator ([node_loader.rs](../../server/src/compiler/node_loader.rs)).
This topic records no implementation gap.

## Contract

`workbench.config.ts` registers adapters under `adapters`, each a name with:

- `runtime`: the adapter's browser mount, a project module exporting
  `mount(canvas, source, context, environment)` that returns an optional
  cleanup. It is bundled with the project's packages; Workbench installs
  nothing.
- optional `rules` and `plugins`: Rspack module rules and native plugin
  capabilities made with the `@canonic2/workbench` helpers, which apply
  only to the compiler building that adapter's previews, after Workbench's
  own and the config's global ones
  ([plugins](../preview-pipeline/plugins.md#project-plugins)).

Registering a built-in name replaces that adapter, including the built-in
Astro adapter's server rendering. A definition naming an unregistered adapter
fails with `Unknown adapter`. Custom adapters build in the `web` profile and
share the entry, lifecycle and portable output of the built-ins
([previews](../pages/previews.md#adapters-and-project-configuration)).

## Project code on the server

The adapter's JavaScript loaders run in the Node evaluator, one module at a
time; `builtin:` loaders and plugin capabilities run natively. JavaScript
plugin objects are configuration problems and do not run. The browser mount
runs only in the frame.

## Target

- The browser mount needs nothing from the server beyond compiling it, so
  it works unchanged with the Rust server.
- `rules` whose loaders are JavaScript run through the Node evaluator, one
  module at a time, as Vue's single-file components do
  ([Vue build](vue.md#build)).
- Plugin implementations migrate to Rust, preserving their virtual modules,
  transforms and resolver behavior. Their adapter-specific scope is preserved;
  they do not require a Node compiler fallback. See the
  [plugin migration decision](../preview-pipeline/plugins.md#plugin-migration-decision)
  for required capabilities and verification.

## Verification

[preview.test.js](../../server/node/preview.test.js) covers custom
adapters on the shared path, plugin capabilities and loader rules, and
JavaScript plugins reported as problems.
