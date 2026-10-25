# Lattice developer guides

Lattice provides native JavaScript primitives for application state and Web
Components. Capabilities own state and actions, bindings supply element inputs
and handle intent events, and elements own their presentation.

Use these guides to choose a state model, connect it to your interface and
understand the costs of each approach. There is no single best model for every
workload. Start with [Choosing an approach](choosing-an-approach.md).

## State

| Guide | Use it when |
| --- | --- |
| [Element-local state](local-state.md) | A fact belongs to one element's interaction, such as an open disclosure |
| [Ordinary stores](stores.md) | Related application fields need immutable snapshots and batched changes |
| [Items in one store field](shared-state-collections.md) | A collection is small or consumers need one snapshot containing all items |
| [Independent item stores](independent-items.md) | Each item is a separately owned capability with its own lifetime |
| [Keyed collections](keyed-collections.md) | Items change independently, and bulk actions must commit together |

## Lists

| Guide | Use it to |
| --- | --- |
| [List selection](list-selection.md) | Change selection without evaluating every row |
| [Fully mounted lists](mounted-lists.md) | Keep keyed rows and their bindings mounted while their items exist |
| [Windowing](windowing.md) | Limit mounted rows to a viewport and overscan; fixed-height application technique |
| [Row recycling](row-recycling.md) | Evaluate reusing row elements; experimental application technique |

State and presentation are separate choices. For example, a keyed collection
can drive a fully mounted list or a windowed one.

## Using the examples

Lattice is unpublished. These guides assume your project already makes
`@canonic2/lattice` available as a local dependency or through browser module
URLs; they do not describe a registry installation. The
[package README](../README.md) owns package setup, runtime requirements and
JavaScript/JSDoc type-check configuration.

The supported imports are:

| Import | Provides |
| --- | --- |
| `@canonic2/lattice` | `createStore`, `shallow` and store types |
| `@canonic2/lattice/collection` | `createCollection` and collection types |
| `@canonic2/lattice/elements` | Native custom-element helpers; importing them registers nothing |
| `@canonic2/lattice/testing` | `checkSelection`, for tests only |

A native browser needs an import map or explicit module URLs; it does not
resolve package names from `package.json`. For a host that serves Lattice's
source under `/vendor/lattice/`, the map can be:

```html
<script type="importmap">
{
  "imports": {
    "@canonic2/lattice": "/vendor/lattice/src/index.js",
    "@canonic2/lattice/collection": "/vendor/lattice/src/modules/collection/collection.js",
    "@canonic2/lattice/elements": "/vendor/lattice/src/modules/elements/elements.js"
  }
}
</script>
```

Use URLs your host actually serves and permits under its content policy.
There is no production transpilation or bundling requirement.

Code fragments in these guides illustrate individual operations; elements and
capability names such as `row` refer to your application's objects, not built-in
Lattice widgets. For an end-to-end runnable application, see the
[filters example](../examples/filters/index.html) and its
[composition code](../examples/filters/app.js). Serve the package directory
over HTTP with its imports resolved; opening the HTML directly from disk is
not the supported example host.
