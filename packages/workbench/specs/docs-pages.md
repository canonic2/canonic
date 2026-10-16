# Docs

Docs are a page written in Markdown, with live examples rendered through it.
They document a component or a design-system foundation the way a component
library's documentation site does: a title and introduction, sections,
examples with captions and notes, hand-written props tables, and each
example's code. A component's guide can present its variations and usage
alongside the source that renders each example.

Docs are a [lens](lenses.md). Any page can have docs and be read through a
docs lens beside its design and its implementations, so a component's design,
its running code, and its documentation are one page in the page list. A page
whose `src` is the Markdown file itself, a **Markdown page**, is its docs and
nothing else: it has no design lens.

**Status:** docs lenses, examples and export are implemented. See
[current status](#current-status) for remaining integration and verification gaps. The user guide is
[Docs](../docs/docs-pages.md). Shared navigation and problem reporting are in [core](core.md);
compilation, adapters, and environments are in
[TypeScript previews](previews.md); lenses are in [lenses](lenses.md) and
[implementations](implementations.md). The code is the
[docs module](../src/modules/docs).

## Purpose and scope

Docs shown on their own use the whole canvas, in the docs canvas mode below.
Beside other artboards on a [multiple-artboard canvas](multiple-artboards.md)
they are an artboard like any other content; see
[in a multiple-artboard canvas](#in-a-multiple-artboard-canvas).

- Docs use the whole [canvas](terminology.md#canvas). There is no artboard:
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
  preview's states and controls.

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
  with a design opens on its design.

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
  in `lenses`. `states` is accepted with the same shape as a preview's.
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

The [multiple-artboard support APIs](multiple-artboards.md) provide isolated
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
- **Lens, state, and example** belong to the artboard's view, so two artboards
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
  the example exercises. No other keys are defined yet; an unknown key is a
  problem.
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
  its own context: `state`, `signal`, `action`, and `navigate` as previews have.
  `inputs` is empty and examples have no controls. One example's error shows in
  its panel and does not affect the others.
- The page is ready when the Markdown has rendered, every example has
  mounted, and fonts and visible images have loaded, with the same limits as
  previews. Capture and export wait for readiness.
- Saving the Markdown replaces it in place, keeping the scroll position and
  the mounted examples; saving a lens's example source, or any file in its
  dependency graph, hot-updates the examples
  ([hot updates](preview-pipeline/hot-updates.md#docs-pages)).

## States

- In a docs lens the page's states apply to the whole page: every example
  receives the current one as `context.state`. The state switcher in the top
  bar selects it. A Markdown page declares its own (`states` in YAML or in
  `defineDocs`).
- The user guide recommends showing a component's variations side by side on
  the page instead of as states.

## Sidebar and address

- A page has no entries for its examples in the page list.
- The address is `#<src>[:<state>][!<example>][~<lens>]`, with the page's own
  `src`: `#src/button/button.workbench.ts!disabled~web` for a page with a
  design, `#docs/card.md!with-custom-style~native` for a Markdown page. An
  address that names an example opens the docs scrolled to it; `!` applies
  only in a docs lens. A docs lens has no width part; one given by hand is
  ignored. The default state is omitted, and so is a page's default lens: its
  design, or a Markdown page's own docs lens. `!` joins `:`, `@`, and `~` as
  characters a `src` cannot contain. Scrolling does not rewrite the address.
- The lens choice persists across pages as for every lens. A page that
  lacks the chosen lens shows its default lens: its design, or a Markdown
  page's own docs lens. A Markdown page always shows one of its lenses.
- A link in the docs to another page's Markdown opens that page in a docs
  lens: the one showing when that page has it, else its default docs lens.

## Top bar

The top bar shows the breadcrumb with its state switcher, the lens switcher
when there are two or more lenses (a page's design first, or a Markdown
page's docs lenses first), reload, source files, the copyable reference,
**Open on its own**, and **More**. A page with a design lists its design file,
then **Docs** (its Markdown), then its implementations' code and each docs
lens's example source; a Markdown page lists its Markdown first, as **Docs**.
In a docs lens, **Preview controls** shows **Actions** and **Reset state**
only.

## Screenshots, handoff, and export

- A screenshot captures the visible part of the docs at the current scroll
  position, with annotations, as the camera does for other lenses. Its size
  is named "Docs, filling the canvas".
- A handoff names the page, the docs lens, state, the Markdown file, and the
  examples visible in the captured area with their source files.
- The [design-system export](export.md) captures, for every page with docs
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

- **Manifest.** `manifest.js` accepts `.md` sources, `lens`, and the `docs`
  kind, and rejects `!` in `src`, for the browser and the server alike.
- **Serving.** The server serves a docs page at its Markdown path and renders
  the Markdown to HTML with example placeholders. Markdown parsing and syntax
  highlighting use pinned dependencies shipped in the extension, like Lucide;
  no runtime network.
- **The page before its examples.** The page answers with its Markdown without
  waiting for a build: its panels are `pending` until the page script learns
  the lens's bundle, which then mounts them or names why not. The examples are
  usually built already: the extension and the command-line server start the
  compile worker with the session, which builds every docs page in each lens
  with the previews as it starts ([warm runtime](preview-pipeline/warm-runtime.md)).
- **Compilation.** Each docs page lens is an entry of the
  [preview pipeline](preview-pipeline/compiler.md#entries), built from that
  lens's example source with the lens's adapter, styles, environment, and the
  project's `workbench.config.ts`. Lenses share the previews' compilers, cache
  and trust rules.
- **The preview host.** Docs pages load into the same warm preview host as
  previews, which renders the page, mounts each example, and reports
  readiness, the examples in view (for handoffs), and scroll position (for
  annotations).
- **Source text.** The server returns an example's source text for **Show
  code**: the whole file for directory examples, the export's declaration for
  named exports.
- **Which lens shows.** [lenses.ts](../contracts/src/docs/lenses.ts)
  decides a page's lenses, the lens it shows, its default lens, the lens a
  docs link opens, the lens the address names, and the canvas mode. It is
  pure; the canvas loads it through
  [bootstrap.ts](../ui/src/modules/docs/canvas/bootstrap.ts) and waits for it
  before applying the config, and Node tests check it. Off a server
  (`file://`) the module can't load and the canvas shows no docs lenses.
- **Docs entries.** The server keeps one entry per Markdown file, naming the
  page it belongs to ([pages.ts](../server/node/src/modules/docs/pages.ts)); the first page
  to claim a file has it. Docs are served at the Markdown's path in the lens
  `?lens=` names, and the routes take the Markdown as `page`. Handoffs, the
  source menu, and export references use the owning page.
- **Schema documentation.** The schema change updates `ui/legacy/README.md`,
  `docs/configuration.md`, `shared/skills/canonic-workbench/SKILL.md`, and
  `shared/rules/canonic-workbench.md`. A new user guide, `docs/docs-pages.md`,
  is added to the docs index and the website's `docGroups`.

## Pilot

The implementation is exercised in this repository's Acme demo (`demo/`),
with one `html` docs lens. Button and Card are the **Components**
collection; Colors is in **Design system**:

- **Button**: the Interactive button TypeScript preview, placed in
  `workbench.yaml` with `docs` and a directory of examples (one file per
  example), so its design and its docs are two lenses of one page.
- **Card**: a Markdown page declared in `workbench.yaml`, named exports in one
  file.
- **Colors**: discovered from `defineDocs`, a foundation page whose live
  swatch example reads the stylesheet's custom properties.

Client-project rollout is separate from this product contract.

## Acceptance criteria

- The Acme demo's three docs open on a white canvas, centered at a
  960-pixel maximum, scrolled to the top at 100%, and scroll to their end.
- Button opens on its design with Workbench and HTML in the lens switcher;
  HTML shows its docs with the size switcher disabled, and Workbench brings
  its artboard and size back. With HTML on, Card shows its docs and Sign in,
  which has no docs, its design.
- Zooming out shows more of the page without reflowing it; annotations drawn
  on an example stay on it while scrolling and zooming.
- Switching lenses re-renders the examples without moving the text; an
  example the current lens lacks shows the placeholder.
- An address that names an example opens the page scrolled to it and
  round-trips through a copied link.
- **Show code** shows Button's whole example file and Card's export
  declaration; **Copy code** copies the same text.
- A screenshot and a handoff name the visible examples; the export contains
  per-example and whole-page references for each lens.
- Each listed problem appears in the problems list with its page named.
- Automated coverage: manifest rules and address parsing (`manifest.test.js`,
  `address.test.js`), Markdown rendering and example placement, discovery of
  `defineDocs`, per-example mounting and readiness, and source-text extraction.
  Canvas behavior is checked in headless Chrome against the demo.

## Design constraints

- Docs shown alone use the canvas viewport so fixed-position content and `vh`
  resolve against the reader's visible area. Beside other artboards they use
  an ordinary sized viewport; full-page document height is reserved for export.
- Prose and examples share the light DOM for annotations, anchors and capture.
  Prose rules exclude `.wb-docs-example` descendants. Highly specific project
  rules can still restyle prose; keep that limitation explicit.
- Named-export source text is extracted by column-zero declaration boundaries,
  including attached comments/decorators, without introducing a second parser.
  The compiler lists export names. Re-exports and multiline templates require
  dedicated source-location handling; see
  [export-source.ts](../server/node/src/modules/docs/export-source.ts).
- Browser and Node use erasable TypeScript with explicit ownership boundaries.
  Node 24 loads host modules; the server strips browser modules in memory.
- Previews and docs examples share compiler profiles, aliases, rules, plugins
  and environments. React Native Web examples use a column flex stage without
  imposing viewport height on panels.
- A declared YAML page wins when a discovered docs definition claims the same
  Markdown file. One Markdown file has one owning page.
- The page list represents pages and states, not docs examples. Examples are
  addressed within their page and navigated in the docs content.
- Whole-page references use document height up to the capture limit;
  per-example references crop to the rendered example stage.

## Current status

Docs lenses, Markdown rendering, example compilation/mounting, source-code
controls, annotations, capture, handoff and portable output have source tests
and Acme browser evidence. Those checks do not establish every VS Code host,
renderer combination or source-extraction edge case. See
[verification points](#verification-points).

### Built

- **Schema.** `manifest.js` reads the `docs` kind, the page's `docs` key, `.md`
  pages, `lens`, and `!` in `src` for both config readers; `address.js` reads
  and writes `!example`; the canvas leaves the width out of a docs lens's
  address and a Markdown page's own docs lens out of its address.
- **Lenses.** [lenses.ts](../contracts/src/docs/lenses.ts) and its tests;
  the canvas's lens switcher, routing, sizes, canvas mode, and docs links in
  [workbench.js](../ui/legacy/workbench.js) decide with it.
- **Discovery.** `defineDocs` from `@canonic2/workbench` (typed in
  [api.d.ts](../sdk/api.d.ts)); the compiler's index validates definitions
  and the server places them by title, in a **Docs** collection when the title has
  no `/`.
- **Astro.** An `astro` lens takes a directory; each `.astro` file renders in the
  worker as an Astro preview's state does, and the bundle hands the page its
  HTML with asset URLs made absolute against the bundle.
- **Portable viewer.** [portable.ts](../server/node/src/modules/docs/portable.ts) writes each page
  in each lens as a static page under `browser/docs/`: the production bundle,
  the page script compiled once, Show code as JSON per example, and the images
  the Markdown references; [viewer.js](../ui/viewer/viewer.js) lists docs
  pages, swaps the State menu for a Lens menu, and follows docs links.
- **Compilation.** `Compiler.docsExamples` lists a lens's examples: a directory's
  files, or a file's named exports from a bundled build with packages external,
  so `export * from` re-exports are listed. The pipeline builds each lens as an
  entry, and the portable export as a production bundle. A listing is kept until
  a file it read changes. Worker routes: `/docs/index`, and `/docs/bundle`,
  which answers with the lens's preview module.
- **Serving.** A page's Markdown is served as its docs at the Markdown's path
  (`?lens=`, `?state=`), with its panels pending; any other `.md` is served as
  a file.
  `/_workbench/docs/bundle` answers what the page mounts (the module, its
  stylesheet, the lens's example IDs, and the page's revision),
  `/_workbench/docs/source` Show code, and `/_workbench/docs/revision` the
  page's revision. `eagerPreviews` on `server.start` warms the worker and
  every docs bundle (`warmDocs` in [server.js](../server/node/server.js)), and
  `onCatalogChanged` tells the host when a listing changes the problems.
  Workbench's own browser code is served from `/_workbench/src/` with types
  stripped ([browser-modules.ts](../server/node/src/server/browser-modules.ts)).
- **The page.** [docs-service.ts](../server/node/src/modules/docs/docs-service.ts) decides each
  panel; [render-markdown.ts](../server/node/src/modules/docs/render-markdown.ts) renders the
  Markdown; [docs-page.ts](../ui/src/modules/docs/page/docs-page.ts) and
  [mount-examples.ts](../ui/src/modules/docs/page/mount-examples.ts) mount the examples,
  run Show code and Copy code, report readiness and actions, send links between docs pages to the canvas, and apply pushed Markdown/example updates with scroll position kept.
- **The canvas.** The docs canvas mode in [zoom.js](../ui/legacy/zoom.js) with
  the geometry from [docs-layout.ts](../ui/src/modules/docs/canvas/docs-layout.ts) (loaded by
  [bootstrap.ts](../ui/src/modules/docs/canvas/bootstrap.ts)); `.wb[data-canvas-mode="docs"]` styles in
  [workbench.css](../ui/legacy/workbench.css); lenses, states, addresses,
  scrolling to an addressed example, and docs links in
  [workbench.js](../ui/legacy/workbench.js).
- **Source menu, handoff, export.** A page's Markdown, when it isn't the
  page's own file, and each docs lens's example source join the page's code
  entries; the handoff names the page, the docs lens, the Markdown, and the
  examples in view with their files; the export plans docs references
  separately (`docsExportPlan` in [server.js](../server/node/server.js)) through the helper's
  `fullPage` and `selector` options
  ([capture-helper/main.cjs](../capture/app/main.cjs)).
- **Problems.** Missing Markdown, placement problems with their line,
  unplaced examples, rejected example files, duplicate export IDs, build
  failures, and blocked examples join `/_workbench/config`'s problems.

### Implementation gaps

- **VS Code acceptance.** Check docs routing, focus, examples, capture and
  handoff in the real editor host. Packaged TypeScript loading and standalone
  browser checks alone do not establish those interactions.

- **Show code for an `export *` example** answers that the export isn't in
  the example source: the statement lives in the re-exported module, which the
  listing doesn't record.
- **Multiple-artboard UI.** Isolated docs renderers participate in support
  APIs, while user-facing multi-artboard composition and controls remain the
  [canvas integration work](multiple-artboards.md#remaining-integration-plan).

- **An address naming a page's Markdown** opens nothing when the Markdown
  belongs to a page with a design: the address names the page's own `src`.
- **The portable viewer** lists each page's docs as an entry of its own with a
  Lens menu, apart from the page's preview.

### Verification points

- [src/modules/docs/](../src/modules/docs) `*.test.ts`: Markdown reading,
  rendering, example IDs, placement, export source, the docs service with fake
  I/O, the canvas geometry
  ([docs-layout.test.ts](../ui/src/modules/docs/canvas/docs-layout.test.ts)), and
  which lens a page shows ([lenses.test.ts](../contracts/src/docs/lenses.test.ts)).
- [preview.test.js](../ui/legacy/preview.test.js): the lens switcher and the
  size switcher in a docs lens, with the canvas's own code.
- [browser-modules.test.ts](../server/node/src/server/browser-modules.test.ts): serving
  and caching browser modules.
- [preview-docs.test.js](../server/node/preview-docs.test.js): docs bundles (directory and
  file lenses, `export *`, environments, rebuilds, request validation) and the
  server (pages in each lens, Show code, revision, problems, untrusted
  workspaces, `defineDocs` discovery, export planning, and a page with a
  design whose docs are served, listed in its sources, and named in a review).
- [zoom.test.js](../ui/legacy/zoom.test.js): docs canvas mode zoom, fit, and scroll.
- [manifest.test.js](../contracts/manifest.test.js),
  [address.test.js](../ui/legacy/address.test.js),
  and [config.test.js](../server/node/config.test.js): the schema and the address.
- [handoff.test.js](../server/node/handoff.test.js): the handoff prompt through a docs lens.
