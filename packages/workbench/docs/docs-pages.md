# Docs

Docs document a component or a part of your design system the way a
component library's website does: a title and introduction, sections, live
examples with captions and notes, props tables, and each example's code. You
write the docs in Markdown and the examples in your own source files.
Workbench renders the examples with the same adapters as
[TypeScript previews](workbench-previews.md).

Docs are a lens. Any page can have docs, read beside its design and its other
implementations in the lens switcher. A page whose `src` is a Markdown file is
a Markdown page: it is its own docs and has no design.

In a docs lens, the docs use the whole canvas on white, at most 960 pixels
wide and centered, and scroll like a page in a browser.

## Add docs to a page

Add `docs` to a page with the Markdown file, and declare a `docs`
implementation that says how to render its examples:

```yaml
implementations:
  web:
    kind: docs
    label: Web
    adapter: react
    styles:
      - src/styles/theme.css

collections:
  - name: Components
    items:
      - label: Card
        src: src/components/card/card.workbench.ts
        docs: docs/card.md
        implementations:
          web: src/components/card/examples/
```

`docs/card.md` places each example with a fenced block named `example`:

````md
# Card

A plain bordered surface that groups related content.

## Basic

```example basic
caption: children
```

The surface alone.
````

And `src/components/card/examples/basic.tsx` is the example:

```tsx
import { Card } from '../card';

export default function Basic() {
  return <Card>This is a card</Card>;
}
```

Open **Components › Card**. The page opens on its design, here the
TypeScript preview. Choose **Web** in the lens switcher: the docs fill the
canvas, with the example mounted in its panel. Choose the design lens again to
return to the artboard at the size it had.

A page with `docs` and no `docs` implementation mapped still has a docs lens:
the built-in **Docs** lens, keyed `docs`. Its Markdown renders, and each
example panel says there is no docs lens rendering examples.

### A Markdown page

A page whose `src` is a Markdown file is its own docs:

```yaml
collections:
  - name: Design system
    items:
      - label: Card
        src: docs/card.md
        implementations:
          web: src/components/card/examples/
```

A Markdown page has no design lens and opens on its own docs lens. In the page
list, it shows its collection's icon, or its own `icon`, and nothing is listed
under it. It can map other implementations too, such as a `url` or
`storybook` lens, beside its docs lenses.

## Write the Markdown

The Markdown is GitHub Flavored Markdown: headings, lists, tables, code blocks
with syntax highlighting, inline code, links, and images. Front matter at the
top of the file is not shown. Write props tables by hand as Markdown tables.

- **Place an example** with a fenced block whose info string is `example` and
  the example's ID. The block's lines set its presentation:

  | Key | Description |
  | --- | --- |
  | `caption` | A short monospaced line under the panel, such as the props the example shows |

- **Notes** are the Markdown after the block.
- **Images and links** with relative paths resolve against the Markdown file.
- **A link to another page's Markdown** (`[Button](button.md)`) opens that
  page in a docs lens: the current one when the page has it, otherwise the
  page's default docs lens. Other links behave as they do in previews: with
  **Actions** on, they are recorded under **Actions** instead of followed.

Example IDs are kebab-case. Placing the same ID twice shows an error in the
second panel.

## Examples

The value a page gives a docs lens is its example source, relative to the
project root:

| Source | Example | ID |
| --- | --- | --- |
| A folder, written with a trailing `/` | Each file directly inside it, its default export | The file name: `with-icon.tsx` is `with-icon` |
| A file | Each named export | The export name in kebab-case: `withIcon` is `with-icon` |

A folder suits one example per file, and **Show code** then shows the whole
file. A file keeps several examples together, and **Show code** shows the
export's statement. Both forms can be used in one project; each lens chooses its
own.

What an example exports depends on the adapter, as a preview's source does: a
component for `react` and `react-native-web`, a component for `vue`, and a
function `(canvas, context)` that draws into `canvas` for `html`. With
`astro`, each `.astro` file in a folder is an example: Workbench renders it
in Node, as it renders [Astro previews](astro.md), and the docs load its
client scripts and assets. An Astro lens takes a folder, since an `.astro`
file holds one component. Examples get
the preview context: `context.state`, `context.signal`, `context.action(name,
...values)`, and `context.navigate(to)`. They have no inputs or controls.

Each example mounts in its own panel. An example that throws shows its error in
its panel and the others still render. The panel is the containing block for
fixed-position content, so a modal or toast example opens inside it. Content
your component moves to `document.body` is not contained.

The docs' own styles stay out of example panels: an example inherits your
stylesheets' body styles, as it would on a blank page with your CSS.

## Lenses

A `docs` implementation is declared once and used by any page with docs:

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `kind` | `docs` | yes | |
| `adapter` | string | yes | `html`, `react`, `vue`, `astro`, `react-native-web`, or an adapter registered in `workbench.config.ts` |
| `label` | string | no | The lens's label in the lens switcher. Defaults to the name in sentence case: `react-native` is *React native*. |
| `styles` | list of paths | no | Stylesheets loaded with the examples, relative to the project root |
| `environment` | path | no | An [environment](preview-data.md#environments) around the examples, relative to the project root. It wraps inside the project's environment from `workbench.config.ts`. |

Each `docs` implementation a page maps is one docs lens, such as **Web** with
React and **React Native Web** with React Native Web. The Markdown is the same
in every docs lens; the lens says what renders the examples. The lens switcher
lists the page's design lens, its other implementations, and its docs lenses.

| Page key | Description |
| --- | --- |
| `docs` | The page's Markdown file, relative to the project root. Not on a Markdown page, which is its own docs. |
| `implementations` | Maps each docs lens to its example source |
| `lens` | A Markdown page only: the docs lens it opens with. Defaults to its first. |

The lens the top bar is on stays when you open another page that has it. A
page without it shows its default lens: its design, or a Markdown page's own
docs lens.

An example the docs place but the current lens doesn't have shows
*Not available in* and the lens's label, so switching lenses never moves the
text.

## On the canvas

A docs lens puts the canvas in its docs canvas mode: the docs fill the canvas
with no artboard, and scroll. Switching to the design lens or another
implementation restores the artboard and its size.

- **Scrolling.** The wheel scrolls the docs. ⌘ or Ctrl with the wheel, or a
  pinch, zooms at the pointer.
- **Zoom** scales the docs without reflowing them: zooming out shows more of
  them. **Zoom to fit** and **100%** both return to 100%. **Recenter view**
  centers the docs and keeps their scroll position.
- **The top bar** shows the lens switcher when the page has two or more
  lenses, and the state switcher when it has states. The size switcher stays
  visible but is disabled, because the docs fill the canvas. **Open the
  source** lists the page's design file, then **Docs**, its Markdown, then each
  docs lens's example source. On a Markdown page, **Docs** is the first row.
- **Annotations** stay on the docs as they scroll and zoom.
- **Screenshots** capture the part of the docs in view.
- **Handoffs** name the Markdown, the lens, and the examples in view with
  their source files.

Saving the Markdown, an example, or anything an example imports reloads the
docs at the same scroll position.

### Show code and Copy code

Below each panel, **Show code** expands the example's source and **Copy code**
copies it. The code is the source as you wrote it, highlighted: the whole file
for a folder example, and the export's statement, with the comment directly
above it, for a named export.

A named export's statement is found by its layout: from its `export` at the
start of a line to the next statement that starts a line. An example
re-exported from another file with `export * from` has no statement in the
example source, so **Show code** says so.

## Declare a Markdown page in a definition

A `*.workbench.ts` file can define a Markdown page and its docs lenses
together, with paths relative to the definition:

```ts
import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'design-system/colors',
  title: 'Design system/Colors',
  docs: './colors.md',
  lenses: {
    web: { label: 'Web', adapter: 'react', examples: './colors/', styles: ['./theme.css'] },
    native: { label: 'React Native Web', adapter: 'react-native-web', examples: './colors.native/' },
  },
  lens: 'web',
});
```

The page is discovered like a preview and placed by its `title`: the first
segment names the collection, the last the page, and any between are a group. A
title without `/` goes in **Docs**. In the page list, the page shows its
`icon`, a Lucide name, or `book-open` without one. If `workbench.yaml` also lists the same Markdown file, its
entry is used and the definition is skipped.

To give a page with a design its docs, use `docs` on its entry in
`workbench.yaml`, as in [Add docs to a page](#add-docs-to-a-page).

## States

In a docs lens, the page's state applies to the whole of its docs: every
example receives it as `context.state`, and the state switcher in the top
bar's breadcrumb changes it. A page with a design declares its states as
usual. A Markdown page can declare states in `workbench.yaml` or in
`defineDocs` (`states: { loading: { label: 'Loading' } }`), and the page list
doesn't list them. Showing a component's variations side by side in the docs
is usually clearer than states.

## Addresses

The address names the page by its `src`, not its Markdown. In a docs lens it
has no width. `!` names an example and `~` a lens:

```text
#src/components/card/card.workbench.ts!with-custom-style~web
#docs/card.md!with-custom-style~native
```

The docs open scrolled to the example. `!example` applies only in a docs lens.
A Markdown page's own docs lens is left out of its address, as the design lens
is for other pages, and scrolling doesn't change the address. Because `!`
marks the example, no `src` can contain it.

## Export

The [design-system export](design-system-export.md) captures, for each docs
lens of a page with docs, the whole docs at their 960-pixel layout and each
example the lens renders, cropped to its panel. These references are filed
under the page's `src`. A page with a design also gets its usual design
references; a Markdown page has no others. The Markdown and the example
sources are in the export's sources.

The export's `browser/` viewer lists each page's docs as its own entry. Each
opens in each of its docs lenses, with its examples running and **Show code**
working, from any static HTTP server.

## Requirements and problems

Examples run your project's code, so they render only in a trusted workspace
and not when `previews: false` is set. The Markdown still renders, and each
panel says why its example is missing.

The problems list reports, with the page named: a missing Markdown file, an
example block with an unknown key or a duplicate ID (with its line), an example
a lens has that the docs never place, example file names that aren't
kebab-case, and lenses that fail to build. Problems that come from a lens's
examples appear once Workbench has read them, usually a moment after the list
loads.

It also reports page entries that don't fit the docs model:

- `docs` that isn't a `.md` file inside the project, or `docs` on a Markdown
  page.
- A `docs` implementation mapped by a page without Markdown.
- An implementation named `docs` on a page with Markdown and no docs lens of
  its own, since `docs` names its built-in **Docs** lens.
- `sizes` or `lensLabel` on a Markdown page, which has no artboard and no
  design lens.
- `lens` that isn't one of a Markdown page's docs lenses, or `lens` on any
  other page.
