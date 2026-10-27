# Lattice documentation

Lattice helps you keep JavaScript state and a browser interface in step. You
change state, select the values a view needs, and apply those values to native
elements or Web Components.

Start with the guide if you are learning Lattice. Use the API reference when
you already know which function you need.

## Getting started

- [Introduction](guide/introduction.md) — what Lattice does and how the pieces fit.
- [Quick start](guide/quick-start.md) — build a working task control in one HTML file.

The guide assumes you know JavaScript functions, objects, modules and DOM
events. The component chapters also introduce the Web Component patterns they use.

## Essentials

Read these in order. The examples grow from one task into a task list.

1. [State](guide/state.md) — create, read and update a store.
2. [Selections](guide/selections.md) — derive the values your interface needs.
3. [Watchers](guide/watchers.md) — respond to changes and end subscriptions.
4. [Components](guide/components.md) — build a reusable element with plain inputs.
5. [Bindings and actions](guide/bindings.md) — connect an element to application state.
6. [Lifetimes](guide/lifecycle.md) — mount, disconnect and clean up a view.
7. [Collections](guide/collections.md) — manage independently changing items.
8. [Rendering lists](guide/list-rendering.md) — retain rows, handle selection and replace items.

## Going further

- [Choosing state ownership](guide/state-ownership.md) — local state, shared snapshots and independent items.
- [Maintain counts and sums](guide/aggregates.md) — incrementally update reversible collection summaries.
- [Large lists](guide/windowing.md) — windowing, focus and the costs to measure.
- [Testing](guide/testing.md) — test results, dependencies and observable behavior.
- [TypeScript and JSDoc](guide/typescript.md) — type selections and expose read-only interfaces.

## API reference

[API overview](api/README.md) · [Stores](api/store.md) ·
[Collections](api/collection.md) · [Element helpers](api/elements.md) ·
[Testing helpers](api/testing.md)

## Runnable examples

The repository includes three runnable examples:

- [Guide tutorial](../examples/tutorial/README.md) runs the component, reflected
  input and retained-list recipes shown in this guide. Its browser cases import
  those source implementations directly.

- [Filters](../examples/filters/index.html) shares a query across several fields.
  Its [entry](../examples/filters/app.js), [capability](../examples/filters/filters.js)
  and [bindings](../examples/filters/bindings.js) show the full state-to-element flow.
- [Tasks](../examples/tasks/README.md) combines a collection, selection and a
  fixed-height windowed list. Start here after the list chapters.

Use the [quick start](guide/quick-start.md) to serve these pages locally.
Repository setup and development commands belong to the [package README](../README.md).
