# Docs pages

A docs page documents a component or a part of your design system the way a
component library's website does: a title and introduction, sections, live
examples with captions and notes, props tables, and each example's code. You
write the page in Markdown and the examples in your own source files.
Workbench renders the examples with the same adapters as
[TypeScript previews](workbench-previews.md).

A docs page uses the whole canvas on white, at most 960 pixels wide and
centered, and scrolls like a page in a browser.

## A first docs page

A docs page is a page whose `src` is a Markdown file. Its lenses are
`examples` implementations, which say how to render the examples:

```yaml
implementations:
  web:
    kind: examples
    label: Web
    adapter: react
    styles:
      - src/styles/theme.css

collections:
  - name: Design system
    items:
      - label: Card
        src: docs/card.md
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

Open **Design system › Card**. The page renders, with the example mounted in
its panel. In the page list, the page shows its collection's icon, or its own
`icon`, and nothing is listed under it.

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
- **A link to another docs page** (`[Button](button.md)`) opens that page in
  Workbench. Other links behave as they do in previews: with **Actions** on,
  they are recorded under **Actions** instead of followed.

Example IDs are kebab-case. Placing the same ID twice shows an error in the
second panel.

## Examples

The value a docs page gives a lens is its example source, relative to the
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
in Node, as it renders [Astro previews](astro.md), and the page loads its
client scripts and assets. An Astro lens takes a folder, since an `.astro`
file holds one component. Examples get
the preview context: `context.state`, `context.signal`, `context.action(name,
...values)`, and `context.navigate(to)`. They have no inputs or controls.

Each example mounts in its own panel. An example that throws shows its error in
its panel and the others still render. The panel is the containing block for
fixed-position content, so a modal or toast example opens inside it. Content
your component moves to `document.body` is not contained.

Docs page styles stay out of example panels: an example inherits your
stylesheets' body styles, as it would on a blank page with your CSS.

## Lenses

An `examples` implementation is declared once and used by any docs page:

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `kind` | `examples` | yes | |
| `adapter` | string | yes | `html`, `react`, `vue`, `astro`, `react-native-web`, or an adapter registered in `workbench.config.ts` |
| `label` | string | no | The lens's label in the lens switcher. Defaults to the name in sentence case: `react-native` is *React native*. |
| `styles` | list of paths | no | Stylesheets loaded with the examples, relative to the project root |
| `environment` | path | no | An [environment](preview-data.md#environments) around the examples, relative to the project root. It wraps inside the project's environment from `workbench.config.ts`. |

Lenses switch what renders the examples, such as React and React Native
Web. The Markdown is the same in every lens. A docs page
always shows one of its lenses: the one the lens switcher is on when the page
has it, otherwise the page's `lens`, otherwise its first.

| Page key | Description |
| --- | --- |
| `implementations` | Maps each lens to its example source |
| `lens` | The lens the page opens with |

An example the page places but the current lens doesn't have shows
*Not available in* and the lens's label, so switching lenses never moves the
text.

## On the canvas

A docs page shown on its own puts the canvas in its docs canvas mode: the page
fills the canvas with no artboard, and scrolls.

- **Scrolling.** The wheel scrolls the page. ⌘ or Ctrl with the wheel, or a
  pinch, zooms at the pointer.
- **Zoom** scales the page without reflowing it: zooming out shows more of the
  page. **Zoom to fit** and **100%** both return to 100%. **Recenter view**
  centers the page and keeps its scroll position.
- **The top bar** shows the lens switcher when the page has two or more
  lenses, and the state switcher when it has states. The size switcher stays
  visible but is disabled. **Open the source** lists the Markdown and each
  lens's example source.
- **Annotations** stay on the page as it scrolls and zooms.
- **Screenshots** capture the part of the page in view.
- **Handoffs** name the docs page, the lens, and the examples in view with
  their source files.

Saving the Markdown, an example, or anything an example imports reloads the
page at the same scroll position.

### Show code and Copy code

Below each panel, **Show code** expands the example's source and **Copy code**
copies it. The code is the source as you wrote it, highlighted: the whole file
for a folder example, and the export's statement, with the comment directly
above it, for a named export.

A named export's statement is found by its layout: from its `export` at the
start of a line to the next statement that starts a line. An example
re-exported from another file with `export * from` has no statement in the
example source, so **Show code** says so.

## Declare a docs page in a definition

A `*.workbench.ts` file can define a docs page and its lenses together, with
paths relative to the definition:

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

## States

A docs page can declare states, in `workbench.yaml` or in `defineDocs`
(`states: { loading: { label: 'Loading' } }`). The state applies to the whole
page: every example receives it as `context.state`, and the state switcher in
the top bar's breadcrumb changes it. The page list doesn't list a docs page's
states. Showing a component's variations side by side on the page is
usually clearer than states.

## Addresses

A docs page's address has no width. `!` names an example and `~` a lens:

```text
#docs/card.md!with-custom-style~native
```

The page opens scrolled to the example. The page's own lens is left out, and
scrolling doesn't change the address. Because `!` marks the example, no `src`
can contain it.

## Export

The [design-system export](design-system-export.md) captures, for each lens of a
docs page, the whole page at its 960-pixel layout and each example the lens
renders, cropped to its panel. The Markdown and the example sources are in the
export's sources.

The export's `browser/` viewer also lists the docs pages. Each page opens in
each of its lenses, with its examples running and **Show code** working, from
any static HTTP server.

## Requirements and problems

Examples run your project's code, so they render only in a trusted workspace
and not when `previews: false` is set. The Markdown still renders, and each
panel says why its example is missing.

The problems list reports, with the page named: a missing Markdown file, an
example block with an unknown key or a duplicate ID (with its line), an example
a lens has that the page never places, example file names that aren't
kebab-case, and lenses that fail to build. Problems that come from a lens's
examples appear once Workbench has read them, usually a moment after the list
loads.
