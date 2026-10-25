# HTML adapter

Part of the [adapters](README.md). User guide: [HTML](../../docs/html.md).

## Source

A preview's `source.entry` is either:

- **an HTML file:** its markup, linked stylesheets, inline and local scripts
  and assets render in the frame; or
- **a JavaScript module** whose default export is a mount function
  `(canvas, context)`, returning an optional cleanup. A docs example of an
  `html` lens is the same kind of function.

## Build

HTML is not served as a static file. Workbench's
HTML loader ([html.rs](../../server/src/compiler/html.rs), registered in
[loaders.rs](../../server/src/compiler/loaders.rs) as
`builtin:workbench-html-loader`) turns the page into a module whose default export is its HTML, so it enters
the build like any other source:

- linked local stylesheets and inline `<style>` blocks become CSS imports,
  resolving their `url()` assets and updating in place;
- inline scripts and local `<script src>` become modules of the build,
  compiled with SWC, so they can import local modules, packages and CSS, and
  their top-level declarations stay private. Each is replaced by a marker;
- local images and media become asset URLs;
- `<title>`, `<meta>` and `<base>` are dropped.

JSON data blocks and external scripts stay in the HTML. The part loader
(`builtin:workbench-html-part-loader`) serves each extracted style and script through a resource query on the same
file. The [Astro adapter](astro.md) uses the same loaders for its rendered
HTML.

## Project code on the server

None. Scripts run only in the frame.

## Browser

[html.js](../../ui/preview/html.js) parses the HTML, copies the attributes of
its `<html>` and `<body>` onto the frame's (restored on cleanup), inserts its
head and body content into the canvas, applies the scenario, and waits for
linked stylesheets. It then runs the scripts in document order: a marked
script by importing its compiled module (imports evaluated once, the
module's default function on every render), any other script by replacing
it with a live copy. A module script's URL gets a render number, so it runs
again. Resources that fail or take over 5 seconds fail the render. The
environment's `mount` runs last.

## Edits

Markup re-renders in place from the new module; stylesheets are replaced in
place ([hot updates](../preview-pipeline/hot-updates.md#edit-behavior)).

## Target

The HTML loader and part loader are Workbench's own code, without project
JavaScript, so they are Rust loaders in the Workbench server.

## Status

Implemented as Rust loaders in `workbench-server`'s compiler, as the target
describes, covered by [preview.test.js](../../server/node/preview.test.js)
(scripts, inline modules, CSS, `srcset`, assets) and the unit tests in
[html.rs](../../server/src/compiler/html.rs).
