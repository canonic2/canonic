# React adapter

Part of the [adapters](README.md). User guide: [React](../../docs/react.md).

## Source

A preview's `source` is a React component; `inputs` are its props. A docs
example of a `react` lens is a component too. React and React DOM come from
the project.

## Build

Rspack's built-in SWC loader compiles TypeScript, JSX and TSX with the
automatic JSX runtime, and React Refresh for project sources outside
`node_modules` and generated entries. CSS is Rspack's native CSS. It builds
in the `web` profile ([compiler](../preview-pipeline/compiler.md#one-compiler-per-project-and-profile)).
The shared rules are in [plugins](../preview-pipeline/plugins.md#workbench-format-handling).

## Project code on the server

None.

## Browser

[react.js](../../ui/preview/react.js) renders the component with
`createRoot` into the canvas, inside the environment's `wrap` when there is
one, under an error boundary. The mount resolves after the first layout
effect; an error before then fails the mount, and one after it goes to
`context.error`. Cleanup unmounts the root.

## Edits

React Refresh updates a component in place with its state kept
([hot updates](../preview-pipeline/hot-updates.md#edit-behavior)).

## Target

Everything React needs is in Rspack's Rust crates: the SWC loader, React
Refresh and native CSS. The lab's Rust host builds React pages natively and
passes the stage 2 correctness checks
([report](../../labs/instant-first-page/README.md#stage-2-report)).

## Status

Implemented in the Node compile worker. Prototyped in the lab's Rust host,
not in the product.
