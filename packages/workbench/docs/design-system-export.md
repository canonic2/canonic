# Design-system export

**More** › **Export…** opens a dialog for the current page, selected pages, a
collection, or the whole current space. Choose a source package (ZIP), PDF,
images (ZIP), or portable browser viewer (ZIP).

Source packages include the selected pages' implementation sources, imports,
reference screenshots, and a README per page. They default to all declared
states and sizes. A single-page PDF or image export defaults to the current
state, size, and lens; choose all declared variants to include every state and
size instead. Select several entries in the page list with your platform's
multiple-selection keys.

Visual PDF pages preserve the artboard's width and full document height.
Documentation uses paginated A4 or Letter paper. A document can contain both
kinds, with different page sizes. Text remains selectable. Visual pages taller
than 19,200 CSS pixels exceed the renderer's PDF limit and produce a warning.
PNG or JPEG image exports capture the full page up to 8,192 pixels high; use
PDF for longer pages. Unsupported native lenses report an error.

Portable browser exports include compiled TypeScript previews and pages' docs
with their declared variants. Authored HTML and native views do not have a
portable viewer; a selection with no supported pages fails with an explanation.

The export covers the space open in the canvas. It does not combine every
space listed in the switcher or in `workbench.yaml`. To export another space,
switch to it and download its export separately.

Use it to give an agent the complete context for a redesign or an
implementation, or to check whether anything changed since the last export.

The export needs the workbench server, in VS Code or
[standalone](extension.md#running-without-vs-code). It doesn't work from a
page opened from disk.

## What goes in

The source package starts from the selected pages' entry points in the current space:

- the design page (`src`),
- its [code pointers](lenses.md#point-at-the-code),
- for Storybook, each story's file and component, from Storybook's index,
- for a [TypeScript preview](workbench-previews.md), its definition, its source
  file, and the local files the compiler resolves for it,
- for a page with [docs](docs-pages.md), its Markdown file and each docs
  lens's example source.

From there it follows local imports and referenced assets, and includes:

| Included | Notes |
| --- | --- |
| `workbench.yaml` | Included when it is inside the space's exportable source roots. The original file is copied as written, so it may declare other spaces; that does not include those spaces' pages in this export. A manifest outside those roots is omitted. |
| Design pages and their local CSS, scripts, images, and fonts | Paths stay project-relative. |
| Implementation sources and their local imports | Workspace files keep their project-relative paths at the archive root. Sources outside the workspace go under `implementations/`. |
| Linked local packages | Followed through their `package.json` exports, instead of being treated as installed dependencies. |
| Storybook configuration | From `.storybook`, or a `--config-dir`/`-c` named in a local package's Storybook scripts. |
| Asset folders referenced statically | `new URL("./assets/", import.meta.url)` includes the folder, which covers plugins that build sprites or font sets from a directory. |
| SVG sprite sources | Source SVGs for a Vite `iconDirs` folder named through `path.resolve(process.cwd(), "…")`. |
| Package configuration | The `package.json`, `tsconfig`/`jsconfig`, and Vite, Tailwind, and PostCSS configuration files the included sources need. |
| Reference screenshots | One JPEG per design state, imported story, and TypeScript preview state, at each of the page's sizes, and per docs lens, one of the whole docs and one of each example. |
| Compiled TypeScript previews and docs | A `browser/` viewer you can open without Workbench. See [The browser viewer](#the-browser-viewer). |

TypeScript imports written with `.js` extensions resolve to the `.ts` or
`.tsx` file when the JavaScript file doesn't exist. Imports starting with `@/`
or `~/` resolve to the `src/` folder.

When the project has no root `package.json`, the export adds one that lists the
external packages the sources import, so you can install them before running
the copied setup.

Left out: installed dependencies (`node_modules`), build output, secrets,
tests, and any source the current space doesn't reach. Shared sources needed
by this space are included even when other spaces also use them.

## Reference screenshots

The export captures a reference JPEG for every declared state of every design
page, every story of every imported Storybook title, and every state of every
TypeScript preview, at each of the page's
[`sizes`](pages-and-states.md#sizes):

| Size | Captured at |
| --- | --- |
| `laptop` | 1512 × 982 |
| `mobile` | 393 × 852 |
| `fit` | 1440 × 900 |
| Custom fixed size | Its configured width and height |
| Size with a filled axis | 1440 wide or 900 tall on that axis |
| `resizable` | Skipped; if it is the page's only size, one 1440 × 900 reference |

A page without `sizes` is captured at its space's sizes, except Resizable.
Sizes that resolve to the same dimensions are captured once, using the first
size's key and label.

Design pages are captured as designed, with actions off; lenses on a design
page aren't captured.

A page's [docs](docs-pages.md) have no sizes. For each docs lens of a page
with docs, the export captures the whole docs at their 960-pixel layout, 1056
pixels wide with their margins and as tall as the docs (up to 8192 pixels),
and each example the lens renders, cropped to its panel. Files are named after
the lens key and the example ID: in a `web` lens, `web-page.jpg` is the whole
docs and `web-basic.jpg` the `basic` example. An example a lens doesn't have
gets no reference in that lens. These references are filed under the page that
owns the docs, by its `src`, beside its design references. A Markdown page has
no other references.

Capture runs as a background job with a progress bar over the canvas. It uses
up to four renderers in parallel and doesn't change what the canvas shows. A
screenshot that fails, or a page that can't be captured, such as an
[iOS Simulator](ios-simulator.md) page or one whose design file is missing,
is listed under `captureWarnings` in `canonic-export.json`, and the rest of the
export continues. When the download finishes, a message gives the total
number of export warnings, covering unresolved sources, screenshots and portable
preview builds.

## The browser viewer

When the current space has [TypeScript previews](workbench-previews.md) or
pages with [docs](docs-pages.md) and examples, the archive also contains a
`browser/` folder with each preview, and each page's docs in each of its docs
lenses, compiled for the browser.

The viewer includes `browser/CANONIC-LICENSE.txt` for Canonic-owned code and
generated authoring types. Your project content and third-party components
retain their own licensing.
To view them, serve the extracted archive with any static HTTP server and open
`browser/index.html`. You don't need Workbench, Electron, or the project's
packages installed.

The viewer lets you search previews, pick a state and a size, and use the
same preview controls as the canvas. Each page's docs are listed with them as
their own entry, marked Docs: the **State** menu becomes a **Lens** menu, the
**Size** menu is hidden, and **Show code** works under each example. Links
between docs open the other docs in the viewer. See [Portable exports](workbench-previews.md#portable-exports) for its controls and
what each adapter supports there. To build the viewer on its own, without the
rest of the export, use the `build` command in
[Command-line tools](workbench-previews.md#command-line-tools).

The original definitions and sources stay editable at their project-relative
paths. When the project has no `workbench-env.d.ts`, the export adds one with
the `@canonic2/workbench` types. `browser/workbench.json` lists the previews and
their states, and the docs and their lenses. `canonic-export.json` lists
the compiled entries and any previews that failed to build under `browser`.

## The archive

When everything fits in one 10 MB ZIP, the download is that ZIP, named after
the current space's `name`, such as `acme-design-system.zip`. A larger export
downloads as one outer ZIP of numbered parts:

```text
acme-design-system-parts.zip
├── README.md
├── acme-design-system-part-01-of-03.zip
├── acme-design-system-part-02-of-03.zip
└── acme-design-system-part-03-of-03.zip
```

- Each part is at most 10,000,000 bytes, for tools with a 10 MB attachment
  limit.
- Every part extracts into the same top-level folder, and repeats the global
  `README.md` and `canonic-export.json`.
- Each part has a `canonic-export-part-NN.json` listing the files in it.

To use a split export, extract the outer ZIP. Then either upload all the
numbered parts together, or extract them all into one folder to get the
complete tree.

Inside the extracted tree:

| File | Contents |
| --- | --- |
| `README.md` | What the export is and how to use it. |
| `canonic-export.json` | The manifest: `files`, external packages under `dependencies`, one record per page under `pages`, unresolved references under `warnings`, failed screenshots under `captureWarnings`, compiled TypeScript previews under `browser`, and the archive's `parts`. |
| `<page folder>/README.md` | Beside each page's main design file or component: its hash, entry points, included files, reference screenshots, and guidance for an agent. |
| `<page folder>/screenshots/` | That page's reference JPEGs. |
| `browser/` | The [browser viewer](#the-browser-viewer), when the project has TypeScript previews or docs with examples. |

When several pages share a folder, each gets its own
`<page name>.README.md` and `screenshots/<page name>/` folder, so they
don't collide. A page with no design or source entry point goes under
`workbench-pages/`.

## Content hashes

Each page record, and each page README, has a SHA-256 hash over the
page's files: their sorted archive paths and raw contents. If a page's
hash matches between two exports, its design and source entry points and
everything they import locally are unchanged.

Generated READMEs, screenshots, archive metadata, and package configuration
added by the exporter aren't part of the hash, so recapturing screenshots
doesn't change it.

## Tips

- Set `root` on every implementation and add `code` pointers, so the export
  includes implementation sources and not only design pages.
- Check `warnings` in `canonic-export.json` for imports the exporter couldn't
  follow, such as aliases it doesn't understand.
- Check `captureWarnings` for missing screenshots, and `browser.warnings` for
  TypeScript previews that didn't build.
- A preview that fails to build is a warning. If the preview worker cannot
  build the viewer at all, the export still includes sources and reference
  screenshots, leaves out `browser/`, and records the cause under `warnings`.
  Read **Workbench: Show Log**,
  or run the `check` command in
  [Command-line tools](workbench-previews.md#command-line-tools) to find the
  failing preview.
