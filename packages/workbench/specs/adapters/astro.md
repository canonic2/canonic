# Astro adapter

Part of the [adapters](README.md). User guide: [Astro](../../docs/astro.md).

## Source

A preview's `source` is an Astro component or page, its module's default
export or a named one; `inputs` are its props. A docs example of an
`astro` lens is a `.astro` file in the lens's directory. The project
installs Astro 4.9 or newer, which has the Container API. Workbench does not
run `astro.config.*`, integrations or the application server; a whole
application stays a [URL lens](../pages/implementations.md#url-implementations).

## Build

Two builds, because frontmatter is server code:

1. **Server side.** The component and its imports are bundled for Node.
   `.astro` files compile with the **project's** Astro compiler:
   `@astrojs/compiler-rs` from Astro 7, `@astrojs/compiler` before it.
   `astro/*` imports stay external and resolve to the project's Astro.
   `astro:*` application modules, such as `astro:content`, need an `aliases`
   stand-in, or the render fails naming the module. Style blocks with a
   preprocessor (`lang` other than `css`) are rejected, and so are client and
   server islands, which need application integrations.
2. **Client side.** The rendered HTML and the resources Astro emitted
   (styles, client scripts) become modules of the preview's build, through
   the [HTML adapter's loaders](html.md#build), so they get shared chunks and
   hot updates like any other format. Root-relative URLs resolve against the
   project's `public/` folder.

## Project code on the server

Frontmatter and rendering. The bundled module renders through Astro's
`experimental_AstroContainer` with `renderToString(component, { props,
partial: true })`. Around each render, `globalThis.fetch` answers from the
scenario's request mocks, relative URLs resolve against `http://localhost/`,
and a render aborts after 10 seconds
([previews](../pages/previews.md#data-request-mocks-and-the-project-environment)).

## Browser

[astro.js](../../ui/preview/astro.js) mounts the pre-rendered HTML of the
selected scenario with the [HTML adapter](html.md#browser). When the inputs
differ from the scenario's, it asks the server's render URL for a new render and
mounts that, resolving its packaged assets against the render's own base.
Without a live server (a portable build), input edits fail with a message.

## Edits

The server watches every file a render read. On a change it renders again
and replaces the HTML module only when the output changed; the page then
re-renders in place without a document reload
([hot updates](../preview-pipeline/hot-updates.md#edit-behavior)).

## Portable output

Every authored scenario is pre-rendered. Controls are removed and the docs get a
note, since input edits need a live render.

## Target

Required for the Rust server ([decisions](README.md#decisions)). Decided by
the owner on 2026-10-16:

- **The server side runs in the Node evaluator:** compiling with the
  project's own Astro compiler, bundling the frontmatter, and rendering
  through the container. Rendering runs
  the project's code, so every Astro page needs the Node evaluator whatever
  compiles it; compiling there adds no process, and the compiler always
  matches the project's Astro runtime. From Astro 7 that compiler is
  `@astrojs/compiler-rs`, Rust loaded as a Node addon. The server asks the
  evaluator for each render over its private channel; a frame's input edit
  goes to the server's render route, never to the evaluator itself
  ([processes and connections](../system/server.md#processes-and-connections)).
- **The client side builds in the Rust server:** the rendered HTML and its
  resources go through the [HTML adapter's](html.md#target) Rust loaders,
  with shared chunks and hot updates.
- **Saved renders (proposed).** Each scenario's rendered HTML is saved beside
  the compiler cache, keyed by the files its render read. On a warm start
  the server serves it without the Node evaluator, which starts only to
  render again: on an edit to one of those files or an input change. When an
  Astro page opens, the server starts the evaluator in the background, so
  the first edit does not wait for it.
- **Not chosen:** linking Astro's Rust compiler into the server. Its crate
  (`astro_codegen` in `withastro/compiler-rs`) is unpublished
  (`publish = false` as of 2026-10-16) and built on Oxc with its own
  `lightningcss`, and a compiler bundled with the server serves one Astro
  runtime, so it drifts from the project's Astro as either upgrades.

## Status

Implemented as the target describes, except saved renders: the server side
runs in the Node evaluator
([astro.ts](../../server/node/src/modules/evaluator/astro.ts),
[astro.cjs](../../server/node/preview/astro.cjs)), with frontmatter bundled
by esbuild, and the client side builds natively in `workbench-server`
through the HTML loader ([astro.rs](../../server/src/compiler/astro.rs)).
Covered by [preview-astro.test.js](../../server/node/preview-astro.test.js).
Saved renders are not implemented: every warm start renders each scenario in
the evaluator again.

## Open questions

- **Project rules on frontmatter.** The server side bundles with esbuild,
  without project Rspack rules and plugins
  ([plugins](../preview-pipeline/plugins.md#status)). Whether the Node
  evaluator should apply project loaders there is open.
- **The container's stability.** `experimental_AstroContainer` is
  experimental in Astro; a change in it breaks rendering whatever compiles
  the component.
