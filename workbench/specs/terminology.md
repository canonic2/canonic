# Terminology

One vocabulary for Workbench: the interface, its labels and accessible names,
the code, the user guides, the specs, handoff prompts, payloads, and the agent
skill all use the terms below. Agreed 2026-10-04.

A term names one thing. When a guide, label, or identifier needs a word for
something listed here, it uses this word and no synonym.

## What a space holds

| Term | Meaning |
| --- | --- |
| **Space** | Collections of pages served together, with their own root and server: one `workbench.yaml`, or one entry in its `spaces`. The space switcher changes the space showing. |
| **Collection** | A set of pages within a space, listed in the collection list. Pages discovered from previews, docs definitions, or catalogs join the collection their title names. `collections` in `workbench.yaml`. |
| **Group** | A named set of pages within a collection. Groups don't nest. `group:` in `workbench.yaml`. |
| **Page** | What the canvas shows: an authored HTML page, a TypeScript preview, a docs page, a Storybook story's title, or a catalog entry. Every entry the page list shows under a collection or group is a page. |
| **State** | One variation of a page, such as *Empty* or *Wrong password*. A page has one or more states; the first is the page as authored. |
| **Docs page** | A page written in Markdown, shown in the docs canvas mode; see [docs pages](docs-pages.md). |
| **Example** | A live render placed in a docs page, shown in its own panel with **Show code** and **Copy code**. |

An address names a page and, when they aren't the defaults, its state, size,
lens, and on a docs page an example; see the [core contract](core.md).

### Lenses

A page can be seen through more than one lens: the page as authored, and the
same page as each implementation of it renders it. The lens switcher picks
one.

| Term | Meaning |
| --- | --- |
| **Lens** | One way of rendering a page: its design lens or one of its implementations. A page has one or more lenses; the lens switcher shows when it has two or more. |
| **Design lens** | The page as authored in the space: the HTML file, or the TypeScript preview. Labeled **Design**, or **Workbench** for a TypeScript preview. A page imported from a catalog and a docs page have none. |
| **Implementation** | Somewhere a page is built, declared once under `implementations` in `workbench.yaml` and named by its key, such as `dev` or `storybook`. A page maps to it with a value that says where the page is in it, such as a path, a story title, a preview ID, or an example source, and the implementation becomes one of that page's lenses, labeled with its `label`. |
| **Kind** | What an implementation is and how its lens renders: `url` (a running app), `storybook` (a Storybook story), `workbench` (a TypeScript preview of a design page), `ios-simulator` and `window` (a live stream of a device or a macOS window), and `examples` (a docs page's examples). |
| **Examples lens** | An implementation of kind `examples`. It renders a docs page's examples with an adapter, and only a docs page has one. A docs page with examples lenses always shows one of them. |
| **Catalog** | An implementation that lists pages of its own instead of lensing pages in `workbench.yaml`: `catalog: true` on a `storybook` or `ios-simulator` implementation. Its pages join the space with that implementation as their only lens. |

Under a Storybook lens, the state switcher lists the title's stories instead
of the page's states.

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
| **Page list** | The chosen collection's groups, pages, and states. |
| **Search** | The field at the bottom of the sidebar that filters every collection's pages, groups, and states. |
| **Problems list** | Configuration, catalog, and preview problems, above the page list. |

The page list shows groups, pages, and states, and the collection list shows
collections. Each is referred to by what it is: picking a state, expanding a
group.

### Top bar

The bar across the top of the canvas, with controls for the page showing.

| Term | Meaning |
| --- | --- |
| **Top bar** | The whole bar. |
| **Breadcrumb** | The space, page, and state showing, at the left. The state part is the **state switcher**. |
| **Actions switch** | Whether links and forms in the page work. |
| **Lens switcher** | One button per lens, in the middle. Shown only when the page has two or more lenses. |
| **Size switcher** | **Fit**, **Laptop**, **Mobile**, and **Resizable**: the artboard's size. Which sizes a page supports is its `viewports`. Disabled in the docs canvas mode. |
| **Reload**, **Open the source**, **Copy reference**, **Open on its own** | Actions on the page showing, named by their labels. |
| **Preview controls** | The panel of a preview's inputs, actions, and **Reset state**. |
| **More menu** | Space-wide commands, such as **Configure pages** and the design-system export, and the top bar's controls that don't fit a narrow window. |

### Canvas

| Term | Meaning |
| --- | --- |
| **Canvas** | The large area that shows the page, gray and dotted, which zooms and pans. Artboards are drawn on it. `#canvas`, inside the `.wb-main` region, which also holds the toolbar and view controls. |
| **Canvas mode** | How the canvas shows the page. **Default**: the page is in an artboard on the canvas. **Docs**: a docs page shown on its own has no artboard; it fills the canvas, on white, and scrolls. A docs page beside other artboards is in an artboard like any page. |
| **Artboard** | The box on the canvas that shows content at a size, labeled with its page's name and size by the **artboard label**. A canvas can hold more than one; see [multiple artboards](multiple-artboards.md). `#artboard`; its clipped `#artboardContent` holds the preview frames. |
| **Preview frame** | The iframe inside an artboard that loads the page. "Frame" means only this. |
| **Toolbar** | The floating bar at the bottom of the canvas: the annotation tools (Select, Scribble, Arrow, shapes, Text, Comment), Undo, Clear, the screenshot, and **Copy handoff**. |
| **View controls** | The floating controls at the bottom right: **Recenter view**, **Zoom out**, the zoom level and its menu, and **Zoom in**. |
| **Annotation** | One drawing made with the toolbar's annotation tools: a scribble, arrow, shape, text, or comment. A page's annotations belong to the page, state, and artboard they were drawn on. |
| **Handoff** | The prompt and screenshot **Copy handoff** prepares for an agent. |

"Page" keeps its ordinary meaning inside a page's own content, such as a
link to another page of the product being designed. "Screenshot" is an image
of the canvas or of a page. "Section" keeps its ordinary meaning for part of a
document, such as a heading's section in a docs page or a guide.

## Decisions

- **2026-10-04: page is what the canvas shows, whatever renders it.** One
  word covers authored pages, previews, docs pages, and catalog entries, as the
  **Configure pages** command does.
- **2026-10-04: space and collection.** A space is what the switcher changes
  between, and a collection is a set of pages within it. "Project" keeps its
  everyday meaning, the codebase, which the guides use throughout (project
  root, project code, trusted project) and which Canonic Studio's project tabs
  also mean. A collection lives in the sidebar, apart from anything on the
  canvas, as design tools place named regions on their canvas.
- **2026-10-04: canvas mode.** The canvas has one mode per way of showing a
  page, named **default** and **docs**.
- **2026-10-04: the schema uses the same words.** `spaces`, `collections`, and
  `group:` in `workbench.yaml`, each the only key for its thing; any other key
  is reported as a problem.
- **2026-10-04: identifiers and payloads follow the terms.** Code, routes,
  payload fields, and prompt text use these words too, including those other
  tools read: the handoff prompt, `/_workbench/config`, and the agent-context
  payloads Shield's hook and MCP server consume. Those consumers change in the
  same release as Workbench.
- **2026-10-04: the toolbar holds the tools.** The bar at the bottom carries
  the drawing and handoff tools; the bar across the top is the **top bar**,
  named by position because it mixes page context, page actions, and
  space-wide commands.
- **2026-10-04: annotations.** What's drawn with the toolbar is an
  annotation, one word for a single drawing and, in the plural, all of them. In
  a tool for HTML and component pages, "markup" means a page's own markup, and
  designers and developers know annotations from design handoff.
- **2026-10-04: lenses are implementations, plus the design.** Each
  implementation a page maps to is one of its lenses; the design lens is the
  page as authored. The kind says how a lens renders, and a catalog is an
  implementation that brings pages of its own.
- **2026-10-04: view controls.** All four controls change the view, not the
  page: recentering as well as zooming.
- **2026-10-04: entries are named by what they are.** A collection, group,
  page, or state in the sidebar is called that, since a layout word doesn't say
  which kind of thing it is.
