# Docs

Docs are a page written in Markdown, with live examples rendered through it.
They document a component or a design-system foundation the way a component
library's documentation site does: a title and introduction, sections,
examples with captions and notes, hand-written props tables, and each
example's code. A component's guide can present its variations and usage
alongside the source that renders each example.

Docs are a [lens](../canvas/lenses.md). Any page can have docs and be read through a
docs lens listed before its design and its implementations, so a component's design,
its running code, and its documentation are one page in the page list. A page
whose `src` is the Markdown file itself, a **Markdown page**, is its docs and
nothing else: it has no design lens.

**Status:** docs lenses, Markdown rendering, example compilation and mounting,
Show and Copy code, the docs canvas mode, annotations, capture, handoff, the
design-system export and portable docs are implemented, served through the
warm Docs host. Docs as isolated renderers beside declared artboards are
implemented; arbitrary artboard management is agreed but not implemented.
Remaining unmet requirements are under [implementation gaps](#implementation-gaps).

The user guide is [Docs](../../docs/docs-pages.md). Shared navigation and problem reporting are in [core](../core.md);
compilation, adapters, and environments are in
[TypeScript previews](previews.md); lenses are in [lenses](../canvas/lenses.md) and
[implementations](implementations.md). The code is the
[docs module](../../server/src/docs.rs), with its use in a space in
[space/docs.rs](../../server/src/space/docs.rs).

## Purpose and scope

Docs shown on their own use the whole canvas, in the docs canvas mode below.
Beside other artboards on a [multiple-artboard canvas](../canvas/multiple-artboards.md)
they are an artboard like any other content; see
[in a multiple-artboard canvas](#in-a-multiple-artboard-canvas).

- Docs use the whole [canvas](../glossary.md#canvas). There is no artboard:
  the page is centered on a white canvas and scrolls vertically until its end,
  like a documentation website. The canvas mode follows the lens: switching a
  page from its docs to its design brings its artboard and size back.
- The text comes from a Markdown file that can live anywhere in the project.
  The examples come from project source files, compiled and mounted by the
  same adapters and environments as [TypeScript previews](previews.md).
- Each docs lens is one renderer of the examples, such as web and React
  Native Web. The Markdown is the same in every docs lens. Themes are not
  lenses.
- Annotations, zoom, screenshots, handoffs, and the design-system export work
  in docs lenses.
- Out of scope: generated props tables (tables are written by hand in the
  Markdown), MDX, interactive controls for examples, and examples drawn from a
  preview's scenarios and controls.

## Declaring docs

Docs are declared in `workbench.yaml` on any page, or discovered from a
definition file like any TypeScript preview.

### In workbench.yaml

A page's `docs` key names its Markdown file. A page whose `src` ends in `.md`
is a Markdown page, and its `src` is its Markdown. Docs lenses are
implementations of kind `docs`:

```yaml
implementations:
  web:
    kind: docs
    label: Web
    adapter: react
    styles:
      - src/styles/theme.css
  native:
    kind: docs
    label: React Native Web
    adapter: react-native-web

collections:
  - name: Design system
    items:
      - label: Button
        src: src/button/button.workbench.ts   # its design: a preview
        docs: docs/button.md                  # its docs
        implementations:
          web: src/button/examples/
          native: src/button/native-examples/
      - label: Card
        src: docs/card.md                     # a Markdown page
        lens: native                          # optional: the docs lens shown by default
        implementations:
          web: src/card/card.examples.tsx
```

- A `docs` implementation takes `label`, `adapter` (a built-in or
  project-registered adapter), and optional `styles` and `environment`, with
  paths relative to the project root. `environment` composes with the
  project's `workbench.config.ts` environment as previews' environments do.
  `base`, `url`, `start`, and `catalog` are reported as problems, and `root`
  other than `.`.
- A page maps each docs lens to an [example source](#examples): a directory
  (ending in `/`) or a single file, relative to the project root. A page
  without Markdown that maps a docs lens is a problem.
- `docs` must be a `.md` file inside the project. On a Markdown page it is a
  problem: the page is its own docs.
- A page with Markdown that maps no docs lens has the built-in **Docs** lens,
  keyed `docs`: the Markdown with no examples. This covers foundations whose
  guides need nothing live. Such a page can't also map another implementation
  named `docs`.
- A Markdown page has no design lens and may map other implementations too.
  `lens` names its docs lens selected by default; without it, the first docs
  lens it maps is the default. `lens` on any other page is a problem: a page
  with a design opens on its first docs lens when it has docs, otherwise on
  its design.
- The lens switcher lists a page's docs lenses first, then its design, then
  its other implementations.

### Discovered from a definition

A `*.workbench.ts` or `*.workbench.tsx` file may default-export
`defineDocs({...})` from `@canonic2/workbench` instead of `definePreview`. It
declares a Markdown page with its docs lenses, and is discovered, placed by
its `title`, and merged with authored pages the same way as a preview. Its optional `icon`, a kebab-case Lucide name, is its icon in
the page list; without one it shows `book-open`. An invalid icon fails the
definition, as a preview's does.

```ts
import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'foundations/colors',
  title: 'Design system/Colors',
  docs: './colors.md',
  lenses: {
    web: { label: 'Web', adapter: 'react', examples: './colors/', styles: ['./theme.css'] },
    native: { label: 'React Native Web', adapter: 'react-native-web', examples: './colors.native/' },
  },
  lens: 'web',
});
```

- Paths resolve from the definition file, as in `definePreview`. `docs` must
  name a Markdown file inside the project.
- `lenses` is a map of kebab-case names to `{ label, adapter, examples,
  styles?, environment? }`. `lens` is the default lens, otherwise the first
  in `lenses`. `scenarios` is a map of IDs following the shared ID rule
  ([scenarios](scenarios.md#declaring-scenarios)) to `{ label? }` only;
  labels default to the ID in sentence case, and an absent or empty map
  declares no scenarios.
- `id` and `title` follow the preview rules, and IDs are unique across
  previews and docs pages.
- A preview's inline `docs` text belongs to its preview controls. A full
  Markdown docs lens is declared where YAML places the page, with `docs` and
  docs-lens mappings. These are separate authoring surfaces.

## The canvas for docs

- The docs fill the canvas. The canvas background is white wherever
  the page is not, at every zoom level. There is no artboard label, border, or
  shadow. The floating toolbar and view controls stay.
- The page's content is at most 960 CSS pixels wide, centered, with side
  margins. A canvas narrower than that narrows the content.
- The page scrolls vertically inside the canvas until its end. The wheel
  scrolls the page; ⌘/Ctrl + wheel and pinch zoom at the pointer. The browser
  window an example sees is the visible canvas, so `position: fixed`, `vh`
  units, and portals to `document.body` behave as on an ordinary page.
- **Zoom** scales the page without changing its layout width, as zoom does
  for artboards. Zooming out shows more of the page and more white canvas
  around it. Zooming in past the canvas width allows horizontal panning. The
  layout width is set from the canvas width and changes only when the editor
  is resized.
- **The default view** is 100%, horizontally centered, scrolled to the top.
  **Zoom to fit** and **100%** both return to 100%. **Recenter view** centers
  horizontally at the current zoom and keeps the scroll position.
- The space's size switcher stays in the top bar,
  disabled with no size pressed ("docs fill the canvas"). The page keeps the
  sizes its design supports, and its design lens shows them again. `sizes` on
  a Markdown page is reported as a problem ("sizes don’t apply to a Markdown
  page, which uses the whole canvas.") and ignored.
- **Annotations** work over the page. They belong to positions in the page,
  so they scroll with it and scale with zoom.

### In a multiple-artboard canvas

The [multiple-artboard support APIs](../canvas/multiple-artboards.md) provide isolated
docs renderers. User-facing canvas integration follows this target behavior:

- **One view, whole canvas.** A canvas showing a docs page and nothing else
  uses the docs canvas mode above.
- **Beside other artboards, an artboard.** A docs page placed beside other
  artboards is an artboard like any other content: no white canvas, its name
  and size above it, the canvas's grid around it.
- **Size.** Its layout width defaults to the docs layout, 1056 pixels (the
  960-pixel content and its margins), and its height to 900 pixels. Both can be
  resized like a Resizable artboard; the content keeps its 960-pixel maximum
  and narrows with a narrower artboard.
- **Scrolling.** The page scrolls inside its artboard, as any page does, so the
  wheel over it scrolls it and the wheel over the canvas pans. The browser
  window an example sees is the artboard's viewport, so `position: fixed`
  and `vh` resolve against it, for the same reason a docs page is never a
  full-height artboard.
- **Zoom** is the canvas's, scaling every artboard alike; the docs page's own
  zoom rules apply only in the docs canvas mode.
- **Lens, scenario, and example** belong to the artboard's view, so two artboards
  can show one page in two lenses: its design beside its docs, or its docs in
  two renderers.
- **Capture and handoff** treat it as any artboard: its viewport in the
  complete canvas image, and in the prompt its page, lens, and the examples in
  its view. The design-system export is unchanged: references per lens,
  independent of any arrangement.

## Markdown

- Markdown follows GitHub Flavored Markdown: headings, lists, tables, code
  blocks with syntax highlighting, inline code, links, and images. Front
  matter is stripped and not shown.
- Relative image paths resolve against the Markdown file.
- Headings get anchor IDs. A link to `#anchor` scrolls to it.
- A relative link to another `.md` file that is a declared docs page opens that
  page in Workbench. Other links behave as links in previews do: recorded under
  **Actions**, or followed through **Open on its own**.
- Workbench supplies the docs stylesheet: neutral, light, system fonts, like
  a plain documentation page. Project overrides of the docs stylesheet are
  outside this contract.

### Placing an example

A fenced block with the info string `example` and an example ID places that
example. Optional `key: value` lines in the block set its presentation:

````md
## With custom style

```example with-custom-style
caption: style
```

Every `div` prop passes through, so `style` or `className` adjusts the surface.
````

- `caption` is a short, monospaced line under the panel, such as the props
  the example exercises. `caption` is the only defined key; an unknown key is
  a problem.
- The prose after the block is ordinary Markdown. Notes are written there.
- An ID is kebab-case. Placing the same ID twice is a problem; the second
  placement shows an error panel.

## Examples

### Example sources

The value a page gives a lens is its example source:

- **A directory** (one example per file). Each file directly inside it with a
  script extension the adapter compiles is one example. Its ID is the file
  name without its extension (`with-custom-style.tsx` is `with-custom-style`),
  and its source is the file's default export. A file name that is not
  kebab-case is a problem.
- **A file** (one example per named export). Each named export is one
  example. Its ID is the export name in kebab-case (`withCustomStyle` is
  `with-custom-style`). The default export, if any, is ignored.

Both forms are supported in the same project, and each lens chooses its own.
What an example export is depends on the adapter, exactly as a preview's
`source` export: a component for `react` and `react-native-web`, a mount
function `(canvas, context)` for `html`, and so on.

### Examples and lenses

- A placed example that the current lens does not export shows a placeholder
  panel: *Not available in React Native Web*, naming the lens by its label. The page keeps its layout, so
  switching lenses never moves the text.
- An example a lens exports but the Markdown never places is a problem
  (`<page>: example “<id>” in <lens label> is not placed in <file>`). It is not
  shown.
- A lens whose source is missing or fails to compile shows an error panel in
  place of every example, with the compile error, and reports a problem. The
  Markdown still renders.

### The example panel and code

Example panels follow the layout of component library documentation (for
example Ant Design's):

- The example renders inside a bordered panel with padding, on a slightly
  tinted background. The panel resets inherited typography so the example
  looks as it does on a blank page.
- The panel is the containing block for its fixed-position descendants, so a
  modal or toast example opens inside its panel. Content portalled to
  `document.body` is not contained.
- Below the rendered example, a row of icon buttons: **Show code** (toggles
  the code under the panel) and **Copy code**. Tooltips name them.
- The code is the example's source text from the project file, not compiled
  output, highlighted by file extension. A directory example shows its whole
  file, including imports. A named-export example shows the export's
  declaration only.
- The caption follows the panel, then the Markdown prose that follows the
  block.

### Mounting and readiness

- Each example mounts into its own panel through the adapter's runtime, with
  its own context: `scenario`, `signal`, `action`, and `navigate` as previews have.
  `inputs` is empty and examples have no controls. One example's error shows in
  its panel and does not affect the others.
- The page is ready when the Markdown has rendered, every example has
  mounted, and fonts and visible images have loaded, with the same limits as
  previews. Capture and export wait for readiness.
- Saving the Markdown replaces it in place, keeping the scroll position and
  the mounted examples; saving a lens's example source, or any file in its
  dependency graph, hot-updates the examples
  ([hot updates](../preview-pipeline/hot-updates.md#docs-pages)).

## Scenarios

- In a docs lens the page's scenarios apply to the whole page: every example
  receives the current one as `context.scenario`. The scenario switcher in the top
  bar selects it. A Markdown page declares its own (`scenarios` in YAML or in
  `defineDocs`). Examples see the first scenario as `default`, whatever its
  ID, and any other scenario by its ID. Docs examples have no iterations.
  Scenario rules shared with other page kinds are in [scenarios](scenarios.md).
- The user guide recommends showing a component's variations side by side on
  the page instead of as scenarios.

## Sidebar and address

- A page has no entries for its examples in the page list.
- The address is `#<src>[:<scenario>][!<example>][~<lens>]`, with the page's own
  `src`: `#src/button/button.workbench.ts!disabled~native` for a page with a
  design, `#docs/card.md!with-custom-style~native` for a Markdown page. An
  address that names an example opens the docs scrolled to it; `!` applies
  only in a docs lens. A docs lens has no width part; one given by hand is
  ignored. The default scenario is omitted, and so is a page's default lens: its
  first docs lens (a Markdown page's `lens`), or the design of a page without
  docs. The design of a page with docs is `~design`, and `design` is reserved
  as an implementation name. A `src` cannot contain `:`, `+`, `!` or `~`. Scrolling does not rewrite the address.
- The lens choice persists across pages as for every lens. A page that
  lacks the chosen lens shows its default lens: its first docs lens, or its
  design when it has no docs. A Markdown page always shows one of its lenses.
- A link in the docs to another page's Markdown opens that page in a docs
  lens: the one showing when that page has it, else its default docs lens.

## Top bar

The top bar shows the breadcrumb with its scenario switcher, the lens switcher
when there are two or more lenses (docs lenses first, then the page's design,
then its other implementations), reload, source files, the copyable reference,
**Open on its own**, and **More**. A page with a design lists its design file,
then **Docs** (its Markdown), then its implementations' code and each docs
lens's example source; a Markdown page lists its Markdown first, as **Docs**.
In a docs lens, **Preview controls** shows **Actions** and **Reset scenario**
only.

## Screenshots, handoff, and export

- A screenshot captures the visible part of the docs at the current scroll
  position, with annotations, as the camera does for other lenses. Its size
  is named "Docs, filling the canvas".
- A handoff names the page, the docs lens, scenario, the Markdown file, and the
  examples visible in the captured area with their source files.
- The [design-system export](../review/export.md) captures, for every page with docs
  and every docs lens, one reference per placed example (the example panel
  alone) and one reference of the whole docs at their 960-pixel layout, filed
  under the page. With only the built-in Docs lens, the whole-docs reference
  only. A page with a design gets its design references too; a Markdown page
  has none. Sources include the Markdown and example files.

## Problems

Docs problems join the shared problems list, named by page: a docs lens on a
page without Markdown, `docs` that isn't a `.md` file in the project, `docs`
on a Markdown page, an implementation named `docs` beside the built-in Docs
lens, `lens` on a page with a design or naming no docs lens, missing Markdown
file, unknown example key, duplicate placement, unplaced example,
non-kebab-case example file, `sizes` on a Markdown page, and compile
failures. Example sources compile only in a trusted
workspace. In an untrusted workspace the Markdown renders and every example
shows a placeholder naming the trust requirement.

Problems that need a lens's examples listed (unplaced examples, rejected
files, duplicate IDs, build failures) join the list once that lens has been
listed: the config never waits for a listing. In VS Code the sidebar's
problems update when they arrive.

## System behavior

- **Docs host.** How Docs are served, highlighted, compiled, hosted, reused
  and kept fresh, and the reusable page lifecycle, are the
  [Docs host](../preview-pipeline/docs-host.md).
- **Listing examples.** The compiler lists a directory lens's files, or a file
  lens's named exports read statically with SWC, following `export * from`
  through local files with packages closed
  ([exports.rs](../../server/src/compiler/exports.rs)). A listing is kept until
  a file it read changes; a changed listing raises the server's `catalog`
  notice (`onCatalogChanged` on the host) so problems update.
- **Astro lenses.** An `astro` docs lens takes a directory; each `.astro` file
  renders in the Node evaluator as an Astro preview's scenario does, and the
  bundle hands the page its HTML with asset URLs made absolute against the
  bundle.
- **Serving.** A page's Markdown is served as its docs at the Markdown's path;
  any other `.md` is served as a file. `/_workbench/docs/bundle` answers what
  the page mounts: the module, its stylesheet, the lens's example IDs and the
  page's revision.
- **Highlighting.** Portable code and explicit server highlighting responses
  use the server's Syntect highlighting under highlight.js class names; live
  docs colour code in the browser. Opening docs does not pre-highlight every
  page or example.
- **Docs on the canvas.** The canvas element owns undecorated reading-mode
  presentation and camera geometry and emits viewport dimensions and scroll
  intent, which the host applies through the content renderer. Viewport
  resizing and zoom keep the docs document. A standalone Docs capture has no
  canvas framing or artboard label.
- **Portable docs.** The portable build writes each page in each lens as a
  static page under `browser/docs/`: the production bundle, the authored page
  and mount scripts unchanged, Show code as JSON per example, and the images
  the Markdown references. The portable viewer lists docs pages, offers a Lens
  menu in place of the Scenario menu, and follows docs links.
- **Export planning.** The export plans docs references separately
  ([plan.rs](../../server/src/export/plan.rs)) through the capture helper's
  `fullPage` and `selector` options.

## Design constraints

- Docs shown alone use the canvas viewport so fixed-position content and `vh`
  resolve against the reader's visible area. Beside other artboards they use
  an ordinary sized viewport; full-page document height is reserved for export.
- Implementation constraints (shared light DOM, source-text extraction,
  content-only rendering, shared compiler profiles) are in the
  [Docs host](../preview-pipeline/docs-host.md#design-constraints).
- A declared YAML page wins when a discovered docs definition claims the same
  Markdown file. One Markdown file has one owning page.
- The page list represents pages and scenarios, not docs examples. Examples are
  addressed within their page and navigated in the docs content.
- Whole-page references use document height up to the capture limit;
  per-example references crop to the rendered example stage.

## Acceptance fixture

Acceptance uses a fixture space with a docs lens and three docs pages:

- **A page with a design and docs**: a TypeScript preview placed in
  `workbench.yaml` with `docs` and a directory of examples (one file per
  example), so its design and its docs are two lenses of one page.
- **A Markdown page** declared in `workbench.yaml`, with named exports in one
  file.
- **A foundation page** discovered from `defineDocs`, whose live example reads
  the stylesheet's custom properties.

The space also has a page with a design and no docs.

## Acceptance criteria

- The fixture's docs open on a white canvas, centered at a
  960-pixel maximum, scrolled to the top at 100%, and scroll to their end.
- The page with a design and docs opens on its docs with Docs before Preview
  in the lens switcher and the size switcher disabled; Preview brings its
  artboard and size back, and its address carries `~design`. With Docs on, the
  Markdown page shows its docs and the page without docs its design.
- Zooming out shows more of the page without reflowing it; annotations drawn
  on an example stay on it while scrolling and zooming.
- Switching lenses re-renders the examples without moving the text; an
  example the current lens lacks shows the placeholder.
- An address that names an example opens the page scrolled to it and
  round-trips through a copied link.
- **Show code** shows a directory example's whole file and a named-export
  example's declaration; **Copy code** copies the same text.
- A screenshot and a handoff name the visible examples; the export contains
  per-example and whole-page references for each lens.
- Each listed problem appears in the problems list with its page named.
- Starting twice mounts fresh without duplicate events; stopping during a
  pending source/highlight/revision/mount operation prevents late mutations and
  releases resources. An exact retained return preserves live state and scroll;
  an evicted return mounts fresh. Example anchors still select the requested
  example after reuse, refresh and a docs/design round trip.
- The pool meets [Docs host acceptance](../preview-pipeline/docs-host.md#verification)
  without changing annotations, zoom, readiness, screenshots, handoffs, error
  containment, trust rules or standalone/portable behavior, in the installed
  extension as well as in unit coverage.
- Automated coverage: manifest rules and address parsing (`manifest.test.js`,
  `address.test.js`), Markdown rendering and example placement, discovery of
  `defineDocs`, per-example mounting and readiness, and source-text extraction.
  Canvas behavior is checked in headless Chrome against a docs fixture.

## Verification points

- The server's `docs` module tests: Markdown reading, rendering,
  highlighting, placement, export source, portable pages and the docs service
  with fake I/O; the canvas geometry
  ([docs-layout.test.js](../../ui/src/modules/docs/canvas/docs-layout.test.js)), and
  which lens a page shows ([lenses.test.ts](../../contracts/src/docs/lenses.test.ts)).
- [preview.test.js](../../ui/legacy/preview.test.js): the lens switcher and the
  size switcher in a docs lens, with the canvas's own code.
- [files.rs](../../server/src/serve/files.rs) tests: serving browser modules.
- [preview-docs.test.js](../../server/node/preview-docs.test.js): docs bundles (directory and
  file lenses, `export *`, environments, rebuilds, request validation) and the
  server (pages in each lens, Show code, revision, problems, untrusted
  workspaces, `defineDocs` discovery, export planning, and a page with a
  design whose docs are served, listed in its sources, and named in a review).
- [zoom.test.js](../../ui/legacy/zoom.test.js): docs canvas mode zoom, fit, and scroll.
- [element.test.js](../../ui/src/views/canvas/components/canvas/element.test.js): standalone
  reading-mode presentation, viewport changes and reconnect lifecycle.
  [capture.test.js](../../ui/src/modules/review/capture.test.js) verifies capture
  framing. [smoke-preview-sessions.cjs](../../scripts/smoke-preview-sessions.cjs)
  verifies Docs/Storybook transitions, reading geometry and scroll anchoring.
- [manifest.test.js](../../contracts/manifest.test.js),
  [address.test.js](../../ui/legacy/address.test.js),
  and [config.test.js](../../server/node/config.test.js): the schema and the address.
- [handoff tests](../../server/src/handoff/tests.rs): the handoff prompt through a docs lens.

## Implementation gaps

- **VS Code acceptance.** Check docs routing, focus, examples, capture and
  handoff in the real editor host. Packaged TypeScript loading and standalone
  browser checks alone do not establish those interactions.
- **Show code for an `export *` example** answers that the export isn't in
  the example source: the statement lives in the re-exported module, which the
  listing doesn't record.
- **Multiple-artboard management.** Isolated docs renderers participate in
  declared-artboard composition. Arbitrary artboard management and its
  editor-host acceptance remain [canvas integration work](../canvas/artboard-management.md#delivery-plan).
- **An address naming a page's Markdown** opens nothing when the Markdown
  belongs to a page with a design: the address names the page's own `src`.
- **The portable viewer** lists each page's docs as an entry of its own with a
  Lens menu, apart from the page's preview.
