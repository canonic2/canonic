# Glossary

One vocabulary for Workbench: the interface, its labels and accessible names,
the code, the user guides, the specs, handoff prompts, and payloads all use
the terms below.

A term names one thing. When a guide, label, or identifier needs a word for
something listed here, it uses this word and no synonym.

## What a space holds

| Term | Meaning |
| --- | --- |
| **Space** | Collections of pages served together, with their own root and Workbench server: one `workbench.yaml`, or one entry in its `spaces`. The space switcher changes the space showing. See [page tree](pages/page-tree.md#spaces). |
| **Collection** | An ordered list of pages and groups within a space, listed in the collection list. `collections` in `workbench.yaml`. Previews and docs definitions join the collection their title names; a catalog's pages form a collection named by the catalog implementation's label. |
| **Group** | A named, ordered list of pages within a collection. Groups don't nest. `group:` in `workbench.yaml`, or the middle parts of a definition's title. |
| **Page** | What the canvas shows: an authored HTML page, a TypeScript preview, a Markdown page, or a catalog page. Every entry the page list shows under a collection or group is a page; its `src` is its identity. |
| **Page tree** | Everything a space lists, in order: its collections, their groups and pages, and each page's scenarios. Each surface reads the authored tree from `workbench.yaml` and merges in the collections the Workbench server imports from definitions and catalogs; see [page tree](pages/page-tree.md). |
| **Scenario** | One of a page's alternative conditions, such as *Empty*, *Loading*, *With filters* or *Wrong password*. A page has one or more; scenarios are alternatives, shown one at a time. See [scenarios](pages/scenarios.md). |
| **Default scenario** | A page's first scenario: what it shows when the address names none. A page that declares no scenarios has one, with the ID `default`. |
| **Declared artboard** | An entry in a scenario's `artboards`: an ID, label, fixed width and height, inputs and iterations. A scenario's declared artboards are shown together. |
| **Implicit artboard** | The one artboard of a scenario that declares none. It takes the size picked in the size switcher. |
| **Iteration** | Another take on one declared or implicit artboard, such as *Compact table* or *Illustrated*, kept beside it to try an idea. With none selected, the artboard shows its design as authored. Iterations are not listed in the page list. See [artboards and iterations](pages/iterations.md). |
| **Docs** | A page's Markdown with live examples, read through a docs lens in the docs canvas mode; see [docs](pages/docs-pages.md). Any page can have docs (`docs` in `workbench.yaml`). |
| **Markdown page** | A page whose `src` is its Markdown: its docs, with no design lens. A `defineDocs` definition declares one. |
| **Example** | A live render placed in docs, shown in its own panel with **Show code** and **Copy code**. |
| **Native format** | A page format Workbench defines and renders end to end: TypeScript previews and docs (Markdown pages, `defineDocs`, and their examples). Authored HTML, implementations and catalogs are not native formats. Unrelated to React Native and the native window stream; see [performance](system/performance.md). |

An address names a page and, when they aren't the defaults, its scenario, the
iterations its artboards show, its size, its lens, and in a docs lens an
example; see the [core contract](core.md#selection-and-navigation).

### The page hierarchy

```text
Space › Collection › (Group) › Page › Scenario › Artboard › Iteration
```

Every level below the page exists even when nothing declares it: a page that
declares no scenarios has one, with the ID `default`, and a scenario that
declares no artboards has one implicit artboard. So a page with one scenario
and its implicit artboard is the same as a page that declares neither; a
single *declared* artboard is not, because it has a fixed size and an ID.
Iterations are optional and belong to one artboard of one scenario. Size and
lens are how an artboard is viewed, not levels of the hierarchy.

| Level | Owner |
| --- | --- |
| Space, collection, group, page | [Page tree](pages/page-tree.md) |
| Scenario | [Scenarios](pages/scenarios.md) |
| Artboard, iteration | [Artboards and iterations](pages/iterations.md) |

### Lenses

A page can be seen through more than one lens: the page as authored, and the
same page as each implementation of it renders it. The lens switcher picks
one.

| Term | Meaning |
| --- | --- |
| **Lens** | One way of rendering a page: its design lens or one of its implementations. A page has one or more lenses; the lens switcher shows when it has two or more. |
| **Design lens** | The page as authored in the space: the HTML file, or the TypeScript preview. Its customizable label defaults to **Design**, or **Preview** for a TypeScript preview; see the [lens contract](canvas/lenses.md). A page imported from a catalog and a Markdown page have none. |
| **Implementation** | Somewhere a page is built, declared once under `implementations` in `workbench.yaml` and named by its key, such as `dev` or `storybook`. A page maps to it with a value that says where the page is in it, such as a path, a story title, a preview ID, or an example source, and the implementation becomes one of that page's lenses, labeled with its `label`. |
| **Kind** | What an implementation is and how its lens renders: `url` (a running app), `storybook` (a Storybook story), `workbench` (a TypeScript preview of a design page), `ios-simulator` and `window` (a live stream of a device or a macOS window), and `docs` (a page's docs, its examples rendered with an adapter). |
| **Docs lens** | A page's docs seen through one renderer of its examples: an implementation of kind `docs`, or the built-in **Docs** lens of a page with Markdown and no docs implementation. Only a page with Markdown has one. A Markdown page always shows one of its lenses, its own docs lens by default. |
| **Catalog** | An implementation that lists pages of its own instead of lensing pages in `workbench.yaml`: `catalog: true` on an `ios-simulator` implementation, one page per booted device matching its `device` (every booted device by default). Its pages form a collection named by the implementation's label, with that implementation as their only lens. |

Under a Storybook lens, the scenario switcher lists the title's stories instead
of the page's scenarios.

Every lens's display label is customizable independently of its kind. The
[lens contract](canvas/lenses.md) defines labels, defaults, and stable identity for
both the authored lens and implementation lenses.

"Project" is not a Workbench term. It keeps its everyday meaning: the codebase
a space's root belongs to, as in the project root, project code, or a trusted
project.

## The interface

```text
┌─────────────────┬──────────────────────── top bar ────────────────────────┐
│ space switcher  │ breadcrumb · Actions   lens switcher   size switcher · … │
│                 ├──────────────────────────────────────────────────────────┤
│ collection list │                                                          │
│                 │               canvas                                     │
│ page list       │            ┌───────────────┐                             │
│                 │            │   artboard    │                             │
│                 │            └───────────────┘                             │
│ search          │        toolbar                         view controls     │
└─────────────────┴──────────────────────────────────────────────────────────┘
       sidebar
```

### Sidebar

The left side: the **Workbench** view in VS Code's activity bar, or the left
edge of a standalone browser window.

| Term | Meaning |
| --- | --- |
| **Sidebar** | The whole left side: space switcher, collection list, page list, search, and the problems list. |
| **Space switcher** | The space showing and the others to switch to, at the top of the sidebar. |
| **Collection list** | Every collection of the space, with how many pages each holds. Picking one shows its pages in the page list. |
| **Page list** | The chosen collection's groups, pages, and scenarios. |
| **Search** | The field at the bottom of the sidebar that filters every collection's pages, groups, and scenarios. |
| **Problems list** | Configuration, catalog, and preview problems, above the page list. Hidden when there are none. |

The page list shows groups, pages, and scenarios, and the collection list shows
collections. Each is referred to by what it is: picking a scenario, expanding a
group.

### Top bar

The bar across the top of the canvas, with controls for the page showing.

| Term | Meaning |
| --- | --- |
| **Top bar** | The whole bar. |
| **Breadcrumb** | The space, page, and scenario showing, at the left. The scenario part is the **scenario switcher**. |
| **Actions switch** | Whether links and forms in the page work. |
| **Lens switcher** | One button per lens, in the middle. Shown only when the page has two or more lenses. |
| **Size switcher** | The space's configured size buttons, defaulting to **Fit**, **Laptop**, **Mobile**, and **Resizable**. A page's `sizes` limits or extends its choices. Disabled in the docs canvas mode. |
| **Size menu** | Defined in [artboard sizes](canvas/sizes.md): the menu at the end of the size switcher, with the sizes that aren't buttons, **Custom size…**, and **Edit sizes…**. |
| **Reload**, **Open the source**, **Copy reference**, **Open on its own** | Actions on the page showing, named by their labels. |
| **Preview controls** | The panel of a preview's inputs, actions, and **Reset scenario**. |
| **More menu** | Space-wide commands, such as **Configure pages** and the design-system export, and the top bar's controls that don't fit a narrow window. |

### Canvas

| Term | Meaning |
| --- | --- |
| **Canvas** | The large area that shows the page, gray and dotted, which zooms and pans. Artboards are drawn on it. It shares the main region with the toolbar and view controls; its canvas element (`wb-canvas`) owns the keyed artboards. |
| **Canvas mode** | How the canvas shows the page. **Default**: the page is in an artboard on the canvas. **Docs**: a page in a docs lens, shown on its own, has no artboard; its docs fill the canvas, on white, and scroll. The mode follows the lens. Docs beside other artboards are in an artboard like any page. |
| **Artboard** | The box on the canvas that shows one declared or implicit artboard of a page and scenario, with its **artboard label**: the page's name and size, with a declared artboard's label after the page's name (*Feedback / Warning*). When a spec means this box rather than the hierarchy level, it says **artboard instance**; an instance has its own ID, distinct from a declared artboard's ID. See [multiple artboards](canvas/multiple-artboards.md). Each instance is a keyed artboard shell of the canvas element; its clipped content holds that instance's content renderer. |
| **Preview frame** | The content iframe inside an artboard, loading a page or preview rather than the Workbench application. "Frame" means only this. |
| **Content renderer** | An owned content-only viewport/adapter with mounting, readiness, interaction, capture and disposal. It may own preview frames or a native surface. It never initializes the Workbench application shell; see [rendering flow](system/rendering-flow.md). |
| **Toolbar** | The floating bar at the bottom of the canvas: the annotation tools (Select, Scribble, Arrow, shapes, Text, Comment), Undo, Clear, the screenshot, and **Copy handoff**. |
| **View controls** | The floating controls at the bottom right: **Recenter view**, **Zoom out**, the zoom level and its menu, and **Zoom in**. |
| **Annotation** | One drawing made with the toolbar's annotation tools: a scribble, arrow, shape, text, or comment. A page's annotations belong to the page, scenario, artboard, and iteration they were drawn on. |
| **Handoff** | The prompt and screenshot **Copy handoff** prepares for an agent. |

"Page" keeps its ordinary meaning inside a page's own content, such as a
link to another page of the product being designed. "Screenshot" is an image
of the canvas or of a page. "Section" keeps its ordinary meaning for part of a
document, such as a heading's section in docs or a guide.

## Behind the canvas

| Term | Meaning |
| --- | --- |
| **Workbench server** | What the VS Code extension, or the standalone command under `server/node/`, starts for a space: it answers the canvas, serves the page tree and serves pages through their adapters. Its Rust binary, `workbench-server` in `server/`, answers the public port, holds the page index and compiles previews with Rspack's Rust crates; its host under `server/node/` answers what belongs to the editor and the capture helper ([server](system/server.md)). Every request goes through it; its child processes take no TCP port ([processes and connections](system/server.md#processes-and-connections)). |
| **Page index** | The part of the Workbench server that finds preview and docs definitions and keeps the saved index; see [page index](preview-pipeline/page-index.md). |
| **Adapter** | What renders one kind of page: `html`, `react`, `react-native-web`, `vue`, `astro`, or a custom adapter registered in `workbench.config.ts`. A definition or docs lens names its adapter. Storybook stories are the adapter that renders through Storybook's own server: one the Workbench server runs on a local socket, or one the user runs at `url`; see [adapters](adapters/README.md). |
| **Node evaluator** | A Node child process of the Workbench server that runs project code an adapter or the page index needs, such as computed definitions, `workbench.config.ts` and Astro renders. One per space, started on need in a trusted workspace, reached only by the server over stdin and stdout or a local socket, never a TCP port ([processes and connections](system/server.md#processes-and-connections)). |

## Naming across interfaces

The schema uses `spaces`, `collections` and `group:` for their respective
concepts. Routes, payload fields, copied references, handoff prompts and agent
integrations use the same vocabulary. Changes to these public names update
consumers and guides in the same release. A sidebar entry is named by its
content: collection, group, page or scenario.
