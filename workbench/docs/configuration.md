# workbench.yaml reference

`workbench.yaml` at the project root is the whole of a project's Workbench
setup. This page lists every key. For a guided introduction, start with
[Getting started](getting-started.md).

The project root is the folder that holds `workbench.yaml`. In VS Code, it has
to be the top of an open folder; see
[Projects in a subfolder](extension.md#projects-in-a-subfolder).

- [A complete example](#a-complete-example)
- [Top level](#top-level)
- [Previews](#previews)
- [Collections](#collections)
- [Groups](#groups)
- [Pages](#pages)
- [States](#states)
- [Sizes](#sizes)
- [Implementations](#implementations)
- [A page's implementations](#a-pages-implementations)
- [Code pointers](#code-pointers)
- [Start commands](#start-commands)
- [Catalogs](#catalogs)
- [Spaces](#spaces)
- [Local overrides](#local-overrides)
- [YAML that the reader accepts](#yaml-that-the-reader-accepts)
- [How problems are reported](#how-problems-are-reported)
- [Editing with the form](#editing-with-the-form)

## A complete example

```yaml
name: Acme
color: green
icon: brand/logo.svg

previews:
  include:
    - packages/ui/**/*.workbench.tsx

implementations:
  preview:
    kind: workbench
  storybook:
    kind: storybook
    url: auto
    root: packages/ui
    catalog:
      icon: book-open
      icons:
        Forms: text-cursor-input
    start:
      command: pnpm storybook
      check:
        port: 6006
      ready:
        url: http://localhost:6006/index.json
      timeout: 90
  dev:
    kind: url
    label: Local app
    base: http://localhost:3000
    root: apps/web
    start:
      command: pnpm dev
      cwd: apps/web
      check:
        port: 3000
  staging:
    kind: url
    base: https://staging.example.com
  web:
    kind: docs
    label: Web
    adapter: react
    styles:
      - packages/ui/src/theme.css

collections:
  - name: Pages
    icon: file-text
    items:
      - group: Auth
        items:
          - label: Sign in
            src: design/pages/sign-in.html
            icon: log-in
            sizes:
              - laptop
              - mobile
            states:
              - id: default
                label: Default
              - id: error
                label: Wrong password
            implementations:
              dev:
                default: /sign-in
                error: /sign-in?error=1
              staging: /sign-in
            code:
              dev:
                - src/routes/sign-in
                - src/components/sign-in-form.tsx

  - name: Components
    icon: component
    items:
      - label: Button
        src: design/components/button.html
        sizes:
          - fit
          - resizable
        docs: design/docs/button.md
        implementations:
          preview: components/button
          storybook: Components/Button
          web: packages/ui/src/button/examples/
        code:
          storybook: src/button
      - label: Card
        src: design/docs/card.md
        implementations:
          web: packages/ui/src/card/examples/
```

## Top level

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | string | no | Titles the sidebar and the browser tab in a standalone browser, names the space in the [space switcher](spaces.md), and names [design-system exports](design-system-export.md). The canvas defaults to `Workbench`, and the switcher and exports to the space's folder name. In VS Code, the canvas tab is always *Workbench*. |
| `color` | string | no | The space's color in the [space switcher](spaces.md#name-color-and-icon): `blue`, `green`, `orange`, `purple`, `pink`, `teal`, `red`, `yellow`, `gray`, or a hex value such as `"#2f7d55"`. Quote a hex value. Defaults to a color chosen from the space's folder. |
| `icon` | string | no | The space's icon in the switcher: a [Lucide](https://lucide.dev/icons/) icon name such as `rocket`, or a project-relative `.svg`, `.png`, `.jpg`, `.webp`, or `.gif` file of at most 256 KB, such as `brand/logo.svg`. Defaults to the first letter of `name`. |
| `collections` | list of [collections](#collections) | no, unless `previews: false` and no [catalog](#catalogs) | The sidebar's collections, in order. |
| `previews` | map or `false` | no | Where [TypeScript previews](#previews) are discovered. `false` turns them off. |
| `implementations` | map of name to [implementation](#implementations) | no | Where pages also exist as running code, and what renders the examples in pages' docs. |
| `spaces` | map of id to [space](#spaces) | no | Several spaces in this one file. Without it, the file is one space. |

With `previews: false`, no catalog, and no usable collection, the config fails
with *nothing to show — a config needs at least one collection with one page in
it.*

## Previews

Workbench discovers [TypeScript previews](workbench-previews.md) in
`**/*.workbench.ts` and `**/*.workbench.tsx` files without any configuration.
Each preview becomes a page, in the collection named by the first segment of
its title, or in **Previews** when the title has no `/`. The same files can
define [Markdown pages](docs-pages.md#declare-a-markdown-page-in-a-definition), which are
discovered the same way. Previews run project code, so in VS Code they need a
trusted workspace.

```yaml
previews:
  include:
    - src/**/*.workbench.ts
  config: workbench.config.ts
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `include` | list of glob patterns | no | Which files are preview definitions, relative to the project root. Replaces the default patterns. Patterns can't start with `/` or contain `..`. |
| `config` | path | no | The preview configuration file, relative to the project root. Defaults to `workbench.config.ts`. See [Custom adapters](custom-adapters.md#configuration-file). |
| `lensLabel` | nonempty string | no | Label of discovered previews' authored lens. Defaults to **Workbench**. An explicit page's `lensLabel` takes precedence. Does not rename docs lenses or `workbench` implementations, which use implementation `label`. |
| `icon` | Lucide icon name | no | Fallback for discovered preview pages and collections. Defaults to `component`. |
| `icons` | map of title prefix to Lucide icon name | no | The longest matching prefix wins; matches end at a `/` boundary or the whole title. |

`previews.lensLabel` sets a display label, such as **Design**, without changing
preview IDs or adding another lens. A page listed in `collections` can
override it with its own `lensLabel`. Invalid or blank values are reported
and ignored; surrounding whitespace is trimmed. See
[Customize lens labels](lenses.md#customize-lens-labels) for a complete example.

For example, choose an icon for a collection and another for its pages:

```yaml
previews:
  icons:
    Web App: app-window
    Web App/Pages: monitor
```

A preview's `definePreview({ icon: 'monitor', ... })` overrides the prefix map
for that page. It does not set the collection icon. Page icons resolve from
the definition, then the longest prefix, then `previews.icon`, then `component`.
Icon settings check kebab-case syntax, not membership in the bundled Lucide
registry. Invalid icon settings are reported and ignored; other previews still
load. An invalid definition icon makes that definition a preview problem.

`previews: false` turns off discovery and never runs preview code. An invalid
`previews` value is reported, and previews stay off until it is fixed.

A page whose `src` is a preview definition, such as
`src: src/button.workbench.ts`, keeps its place in your collections and takes
its states from the definition. Its sizes come from the page's own `sizes`,
not from the definition's.

An explicitly placed preview keeps its handwritten page icon when set;
otherwise it takes the discovered preview icon.

Framework guides: [React](react.md), [React Native Web](react-native-web.md),
[Vue](vue.md), [HTML](html.md), [Astro](astro.md), and
[Custom adapters](custom-adapters.md).

## Collections

A collection is a set of pages, listed in the sidebar's collection list. Each
collection is an entry in `collections`.

```yaml
collections:
  - name: Pages
    icon: file-text
    items:
      - label: Sign in
        src: pages/sign-in.html
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | string | yes | The collection's label. [Previews](#previews), discovered [Markdown pages](docs-pages.md#declare-a-markdown-page-in-a-definition), and [catalog](#catalogs) pages that belong in a collection with the same name are added to it. |
| `icon` | Lucide icon name | no | Shown on the collection and on its pages that don't set their own. Defaults to `file-text`. |
| `items` | list of [pages](#pages) and [groups](#groups) | unless `icon` is set | An icon-only collection styles previews and catalogs imported into the same collection. Collections that remain empty after imports are hidden. An empty collection without an icon is dropped and reported. |

Icon names are [Lucide](https://lucide.dev/icons/) names in kebab-case, such as
`file-text`, `component`, `layout-dashboard`, or `shield-check`. Every Lucide
icon ships with the extension.

To set the icon once for a collection shared by previews and catalogs:

```yaml
collections:
  - name: Web App
    icon: app-window
```

**Configure pages** preserves these declarations when saving. If discovery
finds no matching pages or a catalog fails to load, the empty collection stays
in the configuration and is hidden from the collection list.

Collection icons use the collection name as the prefix lookup target. Highest
priority first: an explicit handwritten collection icon, a `previews.icons`
mapping, a catalog `icons` mapping, configured `previews.icon`, configured
catalog `icon`, then built-in defaults. A collection shared by previews and
Storybook defaults to `component`; a Storybook-only collection defaults to
`book-open`. Conflicting catalog mappings or fallbacks at the same priority
use the alphabetically first icon name. Import order does not decide the icon.
An authored collection without an explicit icon keeps `file-text` when
imported sources use only built-in defaults; mappings and configured fallbacks
override it.

## Groups

A group is a named set of pages inside a collection, written with the
`group:` key. Groups don't nest.

```yaml
items:
  - group: Auth
    items:
      - label: Sign in
        src: pages/sign-in.html
      - label: Reset password
        src: pages/reset.html
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `group` | string | yes | The group's label. |
| `items` | list of [pages](#pages) | yes | An empty group is dropped and reported. A group inside a group is rejected. |

## Pages

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `label` | string | yes | The page's name in the sidebar, in screenshots, and in handoffs. |
| `lensLabel` | nonempty string | no | Label of this page's authored lens. Defaults to **Design** for HTML, or `previews.lensLabel` then **Workbench** for a discovered TypeScript preview. Renames the existing lens without changing its identity. Docs lenses use implementation `label` instead. A Markdown page has no authored lens; `lensLabel` on one is reported. |
| `src` | path | yes | The HTML file, [preview definition](#previews), or Markdown file of a [Markdown page](docs-pages.md#a-markdown-page), relative to the project root. |
| `docs` | path | no | The page's [docs](docs-pages.md): a `.md` file relative to the project root. Gives the page its docs lenses. Reported on a Markdown page, which is its own docs. |
| `icon` | Lucide icon name | no | Overrides the collection's icon for this page. |
| `states` | list of [states](#states) | no | Variations of the page. Shown only when there are two or more. |
| `sizes` | list of [sizes](#sizes) | no | Which artboard sizes the page supports. Defaults to all four. A Markdown page has no artboard; `sizes` on one is reported and ignored. In a docs lens, the docs fill the canvas whatever the sizes. |
| `implementations` | map | no | [Where this page is in each implementation](#a-pages-implementations). For a page with docs, also its docs lenses: each `docs` implementation and its example source. |
| `lens` | implementation name | no | A Markdown page only: the docs lens it opens with, one of its `docs` implementations. Defaults to its first. On any other page, it is reported. |
| `code` | map | no | [Where this page's code lives](#code-pointers), per implementation. |

`src` rules:

- It is relative to the project root. It can't start with `/` or contain `..`.
- It can't contain `:`, `!`, or `~`, which mark the state, an
  example in the docs, and the lens in the [address](canvas.md#links-and-the-address).
- A page without both `label` and `src` is dropped and reported, and the rest
  of the sidebar still builds.

`src` usually points at an HTML file, but any file the server can serve works,
including a page with a query string (`preview/index.html?component=button`).
A `src` ending in `.md` is a [Markdown page](docs-pages.md#a-markdown-page):
docs with live examples and no design, filling the canvas instead of an
artboard. Any other page can have docs too, with `docs`.

## States

```yaml
states:
  - id: default
    label: Default
  - id: error
    label: Wrong password
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | kebab-case string | yes | Travels in the URL (`?state=error`), in the address, and in screenshot filenames. Lowercase letters, digits, and hyphens only. |
| `label` | string | yes | The state's name in the sidebar and the handoff. |

- The **first** state is the page as authored, whatever its id. The page loads
  without a `?state=` parameter for it. By convention it is called `default`.
- A page with fewer than two states shows no states in the page list.
- Declaring a state only adds it to the sidebar. The page has to answer to the
  id; see [Pages and states](pages-and-states.md#states).
- In a [docs lens](docs-pages.md#states), every example receives the page's
  state. A Markdown page can declare states too, and the page list doesn't
  list them.

## Sizes

```yaml
sizes:
  - laptop
  - mobile
  - resizable
```

| Value | Artboard | Export reference size |
| --- | --- | --- |
| `laptop` | Laptop, MacBook Pro 14 | 1512 × 982 |
| `mobile` | iPhone 15 Pro | 393 × 852 |
| `resizable` | An artboard you resize by dragging its edges | Both 1512 × 982 and 393 × 852 |
| `fit` | Fills the available canvas | 1440 × 900 |

- Omitting `sizes` enables all four.
- Sizes in the size switcher that a page doesn't list are disabled while it
  is showing.
- A single value may be written without a list: `sizes: mobile`.
- An unknown value is reported and skipped. If no value is valid, all four
  are enabled.
- [Design-system exports](design-system-export.md) capture one reference per
  listed size, removing duplicate sizes.

## Implementations

Declared once at the top level, then referred to by name from pages. The
name is a kebab-case key, and it is also the lens's label in the lens switcher
unless
`label` says otherwise (`local-dev` is shown as *Local dev*).

```yaml
implementations:
  dev:
    kind: url
    base: http://localhost:3000
```

| Key | Kinds | Required | Description |
| --- | --- | --- | --- |
| `kind` | all | yes | `workbench`, `url`, `storybook`, `docs`, `ios-simulator`, or `window`. |
| `label` | all | no | The lens's display label, independent of kind: for example **Live**, **Prod**, or **Design**. Defaults to the name in sentence case. Changing it preserves the implementation key and addresses. |
| `base` | `url` | yes | The app's origin and optional base path, starting with `http://` or `https://`. Page paths are appended to it. A trailing slash is removed. |
| `url` | `storybook` | yes | Storybook's origin, starting with `http://` or `https://`, or `auto` to [detect a running Storybook](storybook.md#detect-the-port-with-url-auto). |
| `device` | `ios-simulator` | no | `booted` (default) for every booted Simulator, or one exact device name or UDID. |
| `adapter` | `docs` | yes | What renders the examples in a page's docs: `html`, `react`, `vue`, `astro`, `react-native-web`, or an adapter registered in `workbench.config.ts`. |
| `styles` | `docs` | no | Stylesheets loaded with the examples, relative to the project root. |
| `environment` | `docs` | no | An [environment](preview-data.md#environments) around the examples, relative to the project root. |
| `app` | `window` | yes | The macOS application whose window is streamed: its bundle ID, or part of it, such as `com.example.app`. Letters, digits, dots, and hyphens only. |
| `root` | all but `workbench` and `docs` | no | The folder where this implementation's code lives, relative to `workbench.yaml` or absolute. Needed for [code pointers](#code-pointers) and Storybook source paths. Must be a path, not a URL. |
| `catalog` | `storybook`, `ios-simulator` | no | Import pages automatically. See [Catalogs](#catalogs). |
| `start` | `url`, `storybook` | no | A command that starts the implementation in VS Code. See [Start commands](#start-commands). |

A `workbench` implementation shows this space's
[TypeScript previews](#previews). It takes no address, `start`, or `root`: its
root is always the project root, and any other `root` is reported and the
implementation dropped. A `catalog` or `start` on a kind that doesn't support
it is reported and ignored.

A `docs` implementation renders the examples in pages' [docs](docs-pages.md).
It takes no address or `start`, and its root is the project root. It applies
only to pages with Markdown: a Markdown page, or a page with `docs`. Each one
is a docs lens of the pages that map it: the Markdown is the same in every
docs lens, and the lens says what renders the examples; see
[Lenses](docs-pages.md#lenses). A page with Markdown that maps no `docs`
implementation gets the built-in **Docs** lens, keyed `docs`, so no other
implementation it maps can be named `docs`.

Guides: [TypeScript previews](workbench-previews.md), [Docs](docs-pages.md),
[URL implementations](lenses.md), [Storybook](storybook.md),
[iOS Simulator](ios-simulator.md), [App windows](windows.md).

## A page's implementations

A page lists the implementations it exists in, and where. The value depends
on the implementation's kind.

**`docs`**: the docs lens's example source, relative to the project root:
a folder ending in `/`, one example per file, or a file, one example per named
export. See [Examples](docs-pages.md#examples).

```yaml
implementations:
  web: src/components/card/examples/
  native: src/components/card/card.examples.native.tsx
```

**`url`**: one path for every state, or a map of the page's state ids to
paths. Paths start with `/` and are appended to `base`.

```yaml
implementations:
  staging: /sign-in
  dev:
    default: /sign-in
    error: /sign-in?error=1
```

- In a map, the first state's id or `default` gives the path for the page as
  authored, and it is required.
- A state without its own entry uses the default path.
- A map key that isn't one of the page's state ids is reported.

**`workbench`**: a preview ID, such as `components/button`: kebab-case
segments separated by `/`. Each of the page's states opens the preview state
with the same id, and any other state opens the preview's first state. The
preview's source file is added to the page's code pointers.

```yaml
implementations:
  preview: components/button
```

An ID that matches no discovered preview is reported.

**`storybook`**: a story title, exactly as Storybook shows it, including its
prefix. `Components/Button` and `Button` are different titles.

```yaml
implementations:
  storybook: Components/Button
```

**`ios-simulator`**: the device's UDID. The lens streams only a device that the
implementation's `catalog` has imported, so the implementation needs
`catalog: true`. The reader also accepts a device name, but the stream refuses
it. See [iOS Simulator](ios-simulator.md#add-a-simulator-lens-to-a-design-page).

```yaml
implementations:
  simulator: 6A1F2B3C-0000-4000-8000-123456789ABC
```

**`window`**: the window's title, or part of it, ignoring case. When several
of the app's windows match, the largest is streamed.

```yaml
implementations:
  emulator: Example Phone
```

A page may only name implementations declared at the top level.

## Code pointers

`code` maps an implementation to the path, or list of paths, of this page's
code. Paths are relative to that implementation's `root`, or absolute.

```yaml
code:
  dev: src/routes/sign-in
  storybook:
    - src/button/button.tsx
    - src/button/button.css
```

The canvas's **Open the source** menu lists the design file and every code
pointer, and opens one in the editor. Handoffs include their absolute paths, so
an agent edits the implementation the screenshot shows. A code pointer for an
implementation without `root` can't be resolved, and the
[config route](troubleshooting.md#read-the-resolved-config) says so. See
[Code pointers](lenses.md#point-at-the-code).

## Start commands

A `url` or `storybook` implementation can name a command that brings it up.
In a trusted VS Code workspace, Workbench checks whether the implementation is
answering, and runs the command in a visible terminal only when it isn't.

```yaml
start:
  command: pnpm storybook
  cwd: apps/storybook
  check:
    port: 6006
  ready:
    url: http://localhost:6006/index.json
  timeout: 90
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `command` | string | yes | A shell command. |
| `cwd` | path | no | Where to run it, relative to `workbench.yaml`. Defaults to `.`. |
| `check` | probe | yes | How to tell whether it is already running. |
| `ready` | probe | no | How to tell that it has finished starting. Defaults to `check`. |
| `timeout` | integer, 1–300 | no | Seconds to wait for `ready`. Defaults to 60. |

A **probe** has exactly one of:

| Key | Description |
| --- | --- |
| `port` | A TCP port, 1–65535. It is ready when something accepts a connection. Add `host` to check somewhere other than `127.0.0.1`. |
| `url` | An `http://` or `https://` URL. It is ready when the URL answers with a successful response. |

Use `port` for `check` when you only need to know something is listening, and
a `url` for `ready` when the server listens before it can serve, as Storybook
does while it builds. If `ready` times out, the canvas still opens and the
**Workbench** output channel records the timeout. Start commands never run in untrusted workspaces or outside
VS Code. See [Start the server automatically](lenses.md#start-the-server-automatically).

## Catalogs

`catalog` imports pages instead of listing them by hand.

- On a **Storybook** implementation it imports every story title as a page,
  with its stories as states. See [Import the whole catalog](storybook.md#import-the-whole-catalog).
- On an **iOS Simulator** implementation it imports each matching booted
  device as a page. See [iOS Simulator](ios-simulator.md).

`catalog: true` turns it on. A map turns it on and sets icons:

```yaml
catalog:
  icon: book-open            # fallback for imported pages
  icons:                     # Storybook title prefix -> icon; the longest match wins
    UI: palette
    UI/Components: component
    UI/Components/Button: mouse-pointer-click
```

The default icon is `book-open` for Storybook and `smartphone` for the
Simulator. Other kinds don't take `catalog`; TypeScript previews are
[discovered](#previews) without one.

## Spaces

One `workbench.yaml` can describe several spaces, such as a product and its
design system, and the [space switcher](spaces.md) lists each one. List
them under `spaces`, keyed by an id:

```yaml
previews: false              # shared by every space
implementations:
  storybook:                 # shared by every space
    kind: storybook
    url: auto

spaces:
  web:
    name: Acme Web
    color: blue
    collections:
      - name: Pages
        items:
          - label: Sign in
            src: pages/sign-in.html
  design-system:
    name: Acme Design System
    icon: palette
    root: packages/ui        # served from this folder instead
    collections:
      - name: Components
        items:
          - label: Button
            src: button.html
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| *id* | kebab-case string | yes | The space's id in this file, such as `web`. Renaming it makes it a different space to Workbench, which forgets the window's choice of it. |
| `root` | string | no | The folder the space serves and resolves its paths against, relative to this file's folder, or absolute. Defaults to this file's folder, so several spaces can share one. |
| `name`, `color`, `icon` | | no | As at the [top level](#top-level), for this space. They aren't inherited: a space without a `name` is named after its id, such as *Design system*. |
| `collections`, `previews`, `implementations` | | no | As at the top level, for this space. |

Every other key at the top level is shared. A space starts from the top
level, and its own keys replace the shared ones, except `implementations`,
which merges by name: a space can change one shared implementation's `base`
and keep the rest, or add its own.

Every path in a space is relative to its `root`: its `src` values, its `icon`
image, its preview discovery, and the `root` of each implementation it uses,
shared ones included.

**Configure pages** saves the space's own `collections` under its entry in
`spaces`, even when it was showing the shared ones, so the other spaces
keep theirs.

A file with `spaces` that lists no valid space is read as one space from
its top-level keys, and the problem is reported.

## Local overrides

`workbench.local.yaml` beside `workbench.yaml` holds what belongs to one
machine: a port, an absolute `root`, a different start command. Add it to
`.gitignore`.

```yaml
# workbench.local.yaml
implementations:
  dev:
    base: http://localhost:4000
    root: /Users/me/code/acme-web
```

It is merged over `workbench.yaml`:

- `implementations` merges one level deep. Each implementation's keys
  replace the committed ones by name, so the example above changes `dev`'s
  `base` and `root` and keeps its `kind` and `start`. A `start` block is
  replaced whole.
- `spaces` merges by id, and each space merges the way the whole file
  does, so a local file can change one space's implementation and leave
  everything else.
- Every other top-level key, such as `name` or `collections`, replaces the
  committed one whole.

A missing local file is normal. A local file that doesn't parse is reported by
its own name.

Common uses:

| On your machine | In `workbench.local.yaml` |
| --- | --- |
| Your dev server runs on another port | The implementation's `base`, as in the example above |
| The implementation's repository is checked out elsewhere | The implementation's `root`, as an absolute path |
| You start the app differently | A `start` block of your own, which replaces the committed one |
| Storybook's port isn't one `url: auto` finds | The Storybook implementation's `url` |
| You don't want TypeScript previews compiled | `previews: false` |
| You want your own collections and pages for a while | A `collections` list, which replaces the committed one |
| You tell spaces apart differently in the [space switcher](spaces.md) | Your own `name`, `color`, or `icon` |

Saving **Configure pages** edits `workbench.yaml`, not the local file. Saving
`workbench.local.yaml` refreshes the sidebar and canvas in VS Code, as
saving `workbench.yaml` does.

## YAML that the reader accepts

Workbench reads a subset of YAML with its own small parser, the same in the
browser and the server:

- Block mappings, block lists, strings, numbers, `true`, `false`, `null`,
  quotes, and comments.
- No flow collections (`{a: b}`, `[a, b]`), multi-line strings, anchors,
  aliases, tags, or multiple documents.
- Indent with spaces. Tabs are rejected.
- `#` starts a comment at the start of a line or after a space, outside quotes.
  Quote a value that contains a space followed by `#`. `"#00a1ff"` and `a#b`
  are values.

A line the parser doesn't understand is reported with its line number, and
nothing loads until it is fixed.

## How problems are reported

Workbench reports what it can't use rather than guessing, and builds
everything else:

- A page, group, collection, state, or implementation that is invalid is
  dropped. Its problem names where it was, such as
  `Pages › Auth › Sign in: state id “Error” must be kebab-case`.
- In the canvas, every problem found while reading the file is written to the
  browser console.
- `GET /_workbench/config` on the workbench server lists, under `problems`,
  problems with implementations, previews, sizes, pages' implementation
  mappings, and `code`, plus catalogs and previews that couldn't load, and
  [docs problems](docs-pages.md#requirements-and-problems). It sits
  alongside the config as your machine resolves it. A page dropped for a bad
  `src`, label, state id, or group is left out of it without a problem, so
  check the console for those. See
  [Troubleshooting](troubleshooting.md#read-the-resolved-config).
- When previews or a catalog are on, the problems list in VS Code's sidebar
  shows the same problems above the page list.

## Editing with the form

**Configure pages**, in the top bar's **More** menu, edits collections,
groups, pages, labels, source paths, and sizes in a form. See
[Configure pages](canvas.md#configure-pages). Saving rewrites only the
`collections` block of `workbench.yaml`. Implementations, states, implementation
mappings, and code pointers are preserved. Comments outside `collections` are
kept; comments inside it are not.
