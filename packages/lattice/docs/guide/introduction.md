# Introduction

Imagine a task shown in two places: a checkbox in a row and a summary above it.
When the task is completed, both should change. When its title changes, the
completion control should stay untouched.

Lattice gives you a small set of tools for expressing that relationship.

```js
import { createStore } from '@canonic2/lattice';

const task = createStore({ text: 'Write the guide', done: false });

const status = {
  fields: ['done'],
  read: (state) => (state.done ? 'Complete' : 'In progress'),
};

task.watch(status, (value) => console.log(value));

task.update({ done: true });
// Logs 'In progress' when watched, then 'Complete'.
```

A **store** owns the task's state. A **selection** derives a value from named
fields. A **watcher** receives that value immediately and when it changes.

The example uses the console so you can see the sequence. In an interface,
the callback can assign a value to an element.

## From state to an interface

As an application grows, give each part a clear job:

```text
User intent → action → state → selection → element input
```

An action describes something the application can do, such as completing a
task. A binding assigns selected values to element inputs and routes element
events back to actions. An element renders its inputs and handles its own
interaction.

These are ordinary JavaScript functions and native elements. A binding is a
function you write; an action is a method your application exposes.

You can begin with a store and native controls, then introduce a reusable
component when you need one. The [quick start](quick-start.md) takes that first step.

## What Lattice provides

Lattice provides stores, keyed collections, a few Web Component helpers and
selection-testing helpers. It ships native JavaScript with no runtime
dependencies.

Stores and collections work without a DOM, including in Node. Browser code runs
unchanged in the supported browser; a build step is not required. Components
use the platform's `HTMLElement`, properties and `CustomEvent`.

Lattice does not provide a template language, component compiler, router or
widget library. Your application chooses its markup, styles and external services.
See the [package README](../../README.md) for supported runtimes.

## How to read this guide

If you are new to Lattice, follow the Essentials chapters in order. Each
introduces one idea with a small example before explaining its behavior.

When you reach collections, you will reuse the same `update`, `fields`,
`read` and `watch` vocabulary. The owner changes from one record to many
records; the basic pattern stays familiar.

The [API reference](../api/README.md) collects signatures, options and edge cases.
You do not need to learn all of it before building something.

**Next:** [Quick start](quick-start.md)
