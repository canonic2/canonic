# Artboard sizes

Status: **implemented except Configure pages cards and the guides**
(2026-10-04). Sizes, the size switcher with its menu, Custom size…, and Edit
sizes… work; see [Current status](#current-status). The user guides and the
website still describe the four fixed sizes.

## Purpose and scope

Today the size switcher has four fixed sizes: Fit, Laptop (1512 × 982), Mobile
(393 × 852), and Resizable. A page can only narrow that list. A design that
isn't a laptop or phone screen has no size of its own:

- This repository's **Sidebar** page draws the Workbench sidebar: 340 pixels
  wide and as tall as the window. Fit makes it as wide as the canvas, and
  Resizable is one size shared by every page, starting at 1024 × 768
  ([workbench.js](../workbench/workbench.js)).
- A component, panel, popover, or dialog is designed at a known width, often
  with a height that follows the window.

With this work, **a space defines its sizes**: their dimensions, labels,
icons, order, and which of them are buttons in the size switcher. A space that
defines none gets the default sizes. **A page** can limit which of them it
supports, down to one, and add sizes of its own; it can't change the space's
sizes or the size switcher's buttons. Either axis of a size
can **fill** the canvas. **Custom size…** and **Edit sizes…** in the size menu
create and change sizes in `workbench.yaml`.

One word names this everywhere: **size**, and `sizes` in `workbench.yaml` and
preview definitions.

In scope: the schema for spaces and pages, the size switcher and size menu,
fill axes, addresses, handoffs, the design-system export, **Configure pages**,
TypeScript preview definitions, the portable viewer, and the multiple-artboard
model.

Out of scope:

- Docs lenses, which have no artboard size ([docs](docs-pages.md)).
- iOS Simulator and window lenses, which show the device's or window's own
  size letterboxed in the artboard ([implementations](implementations.md));
  their artboard size behaves as it does today.
- Per-page Resizable dimensions. Resizable stays one size per viewer, shared by
  every page.

## Default sizes

A space without `sizes` has these, in this order, each a button in the size
switcher:

| Key | Label | Size | Icon |
| --- | --- | --- | --- |
| `fit` | Fit | Fills the canvas on both axes | `minimize-2` |
| `laptop` | Laptop | 1512 × 982 | `monitor` |
| `mobile` | Mobile | 393 × 852 | `smartphone` |
| `resizable` | Resizable | Dragged by its edges, down to 1 × 1; starts at 1024 × 768 | `scaling` |

`fit` and `resizable` are kinds of size, not dimensions: a space can include,
relabel, re-icon, reorder, or leave them out, but can't give them a `width` or
`height`. `laptop` and `mobile` are ordinary sizes with default dimensions; a
space that lists them can change any of their fields.

## The space's `sizes`

`sizes` is a space's own setting. In a file with one space it is at the top
level; in a file with `spaces`, each entry has its own. A top-level `sizes`
in a file with `spaces` is reported and ignored, because sizes aren't shared
between spaces.

```yaml
sizes:
  fit: true
  laptop: true
  sidebar:
    label: Sidebar
    width: 340
    height: fill
    icon: panel-left
    button: true
  tablet:
    width: 1024
    height: 1366
    icon: tablet
  resizable: true
```

- The map's order is the order of the size switcher's buttons and the size
  menu's entries.
- When a space declares `sizes`, it has exactly those sizes. The defaults are
  not added; list `fit`, `laptop`, `mobile`, or `resizable` to keep them.
- A size's value is a map of the fields below, or `true` for a default size
  as it is. Any other value, including `true` for a key that isn't a default
  size, is reported and the size dropped.
- `workbench.local.yaml` merges a space's `sizes` by key, one level deep, as it
  does for `implementations`: a local size's fields replace the same fields of
  the committed one, and a key the local file adds goes at the end. This holds
  for a space in `spaces` as well as for a single-space file.

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `width` | integer or `fill` | yes, except for a default size | CSS pixels, 1 to 8192, or `fill` |
| `height` | integer or `fill` | yes, except for a default size | CSS pixels, 1 to 8192, or `fill` |
| `label` | string | no | Shown in the size menu, button titles, the artboard label, handoffs, and the export. Defaults to the default size's label, or the key in sentence case (`small-phone` is *Small phone*). |
| `icon` | Lucide icon name | no | The size's icon on its button and in the size menu. Defaults to the default size's icon, `frame` otherwise. |
| `button` | boolean | no | Whether the size is a button in the size switcher. Defaults to `true` for the four default sizes and `false` for others. |

- A size key is kebab-case and can't be all digits.
- `icon` follows the rule for every other icon in `workbench.yaml`: a
  kebab-case Lucide name, reported and replaced by the default when it isn't.
- `width: fill` with `height: fill` is reported: that size is `fit`.
- A size with an invalid or missing dimension, or a `width` or `height` on
  `fit` or `resizable`, is reported and dropped.
- An unknown key inside a size is reported and ignored.
- A space whose `sizes` leaves no valid size is reported and gets the default
  sizes.

Problems name the size, in the form every problem uses
([core](core.md#problem-reporting)), for example:

```text
Sizes › tablet: height must be a whole number from 1 to 8192, or fill.
Sizes › wide: width and height can't both be fill; use fit.
Sizes: sizes go in each space, under spaces.<key>, when the file lists spaces.
Pages › Account menu: size “sidebar” is one of the space’s sizes; a page can list it but not change it.
```

## A page's `sizes`

A page's `sizes` does two things, and nothing else: it limits which of the
space's sizes the page supports, and it can add sizes of its own.

```yaml
- label: Sidebar
  src: design/sidebar.html
  sizes:
    - sidebar
    - resizable

- label: Account menu
  src: design/account-menu.html
  sizes:
    - popover:
        width: 280
        height: 360
        icon: panel-top
    - sidebar
```

- An entry is a size key of the space, or a one-entry map defining a size for
  this page only, with the same fields and rules as a space size.
- A page's own size can't reuse a key of the space's sizes; that entry is
  reported and skipped. A page can't change a space size.
- `button` on a page's own size is reported and ignored: the size switcher's
  buttons belong to the space. A page's own sizes are in the size menu.
- A space size the page doesn't list stays where the space put it, as a
  button or a menu entry, disabled while the page shows.
- A page that lists one size can't be switched: every other size is disabled,
  and the artboard stays at that size.
- Omitting `sizes` supports every size in the space.
- An entry that is neither a space size nor a valid new size is reported and
  skipped. If no entry is valid, the page supports every size in the space.
- On a Markdown page, `sizes` is reported and ignored. A page with a design
  and docs keeps its sizes for its design.
- When the page opens and the current size isn't one of its sizes, the
  artboard takes the page's first listed size. Listing `sidebar` first makes
  the Sidebar page open at 340 × fill.

## Fill

An axis that fills takes the length Fit would give it: the canvas's length on
that axis minus the canvas insets, at least 320 pixels. It follows the canvas
when the editor or window resizes, as Fit does, and not when the canvas zooms.
The other axis keeps its exact length.

- **Zoom to fit** and switching sizes fit the artboard as for any size, never
  above 100%. The filled length doesn't depend on the scale: when the fixed
  axis is too long for the canvas, the whole artboard scales down and the
  filled axis no longer reaches the canvas edges.
- The artboard label shows the resolved size, such as `340 × 812`.
- Content sized with `vh` or `vw` resolves against the artboard, as on any
  page; the filled axis is a real length, not a scrolling page. This differs
  from the full-height artboard that [docs pages](docs-pages.md) rejected,
  which made `vh` resolve against the whole document.

## Resolved sizes

Both config readers resolve sizes with the same rules from `manifest.js`, as
they do for implementations ([core](core.md#configuration-and-serving)). The
resolved config carries:

- `sizes` on the space: every size in order, each
  `{ key, label, icon, button, kind, width, height }`. `kind` is `fit`,
  `resizable`, or `fixed`; `width` and `height` are a number or `'fill'` for a
  `fixed` size and `null` otherwise.
- `sizes` on a page: the keys it supports, in its order, after limits and its
  own sizes. A page's own sizes are in `ownSizes`, in the same shape as the
  space's, with `button: false`.

The canvas, size switcher, export plan, handoff, portable viewer index, and
multiple-artboard adapter all read sizes from these; none keeps its own table.

## The size switcher

The space's sizes with `button` are buttons, in the space's order, each
showing its icon, with its label and dimensions as its title and accessible
name. A menu button follows them and opens the **size menu**:

| Section | Entries |
| --- | --- |
| **Sizes** | The sizes that are buttons, with their icons and dimensions |
| **More sizes** | The space's other sizes, then the page's own sizes, with their icons and dimensions, such as `340 × fill`. Hidden when there are none. |
| (actions) | **Custom size…** and **Edit sizes…**, shown when the workbench server is running, as **Configure pages** is |

- The current size is checked. Sizes the page doesn't support are disabled,
  with the reason in their titles, as disabled buttons have today. A page's
  own sizes are listed only while that page shows.
- When the current size isn't a button, the menu button shows that size's
  icon and is pressed, and its accessible name and title name the size, such
  as *Sidebar, 340 × fill*. Otherwise it shows a chevron.
- In a docs lens, every size is disabled and nothing is pressed or checked.
- The size switcher never folds into **More**. The top bar's compact
  measurement includes however many buttons the space has.
- The menu is a menu of radio items: arrow keys move between enabled entries,
  Enter or Space chooses, and Escape closes it and returns focus to its button.
  Choosing a size resizes the artboard without reloading the page, as the
  buttons do.

### Custom size…

Opens a dialog with **Name**, **Width**, **Height**, **Icon**, **Show as a
button**, and **Only for this page**. Width and height each take a number or
**Fill**. **Add** writes the size to `workbench.yaml` and switches the
artboard to it.

- The key is the name in kebab-case, prefixed with `size-` when that is empty
  or all digits (*1024* is `size-1024`), and made unique by a numeric suffix
  against the space's sizes and every page's own sizes. The name is written as
  `label` when it differs from the key's sentence-case form.
- **Icon** is a text field for a Lucide name, showing the icon as it is
  typed. It starts empty, which means `frame`. **Show as a button** starts
  off.
- By default the size is the space's: it is written to the end of the space's
  own `sizes`. If the space has no `sizes` yet, the write first lists the four
  default sizes, so the switcher keeps what it showed.
- If the current page lists `sizes` in `workbench.yaml`, the new key is added
  to the end of that list in the same write, so the page can show it. This
  also unlocks a page that listed one size.
- **Only for this page** writes the size into the page's `sizes` instead, and
  disables **Show as a button**. It is available only for a page listed in
  `workbench.yaml`.
- A page that isn't listed in `workbench.yaml` (a discovered preview or a
  catalog page) gets the size from the space. If a preview definition lists
  `sizes`, the dialog says the definition must list the new key for this page
  to use it, and the artboard stays where it is.
- On a Markdown page, **Only for this page** is unavailable, and the size is added
  to the space without changing the canvas.
- **Add** stays disabled until the values are valid, and the dialog says why.

### Edit sizes…

Opens a dialog listing the space's sizes in order. Each row edits **Name**,
**Width**, **Height**, **Icon**, and **Show as a button**, can be moved up or
down, and has **Remove**. Width and height are read-only for `fit` and
`resizable`. **Save** writes every change in one write.

- Keys don't change: addresses and page lists refer to them. Renaming changes
  `label`.
- Removing a size also removes its key from every page's `sizes` in the same
  write. A page whose list becomes empty loses its `sizes` key, and so
  supports every size in the space. A preview definition that lists the key
  isn't edited; the key is then reported as unknown for that definition.
- The last size can't be removed; its **Remove** is disabled.
- A space that had no `sizes` gets them written on the first save.
- Sizes that `workbench.local.yaml` defines or overrides are shown read-only,
  with a note that they come from the local file.
- A page's own sizes are edited in its `sizes`; **Edit sizes…** lists only the
  space's.

### Writing `workbench.yaml`

Both dialogs write `workbench.yaml` through the server, the way **Configure
pages** does ([extension](vscode-extension.md)), never `workbench.local.yaml`
or a preview definition:

- The space's sizes are written as the space's `sizes` block: at the top
  level of a single-space file, or under `spaces.<key>`. The block is replaced
  whole; every other line of the file, comments included, is kept.
- When a write also changes pages (adding a key to a page's list, a page's own
  size, or removing a size from pages), the space's `collections` block is
  replaced too, as **Configure pages** replaces it: comments inside it are
  lost, and a space that showed shared collections gets its own copy.
- The file is checked after the change and replaced atomically; a write that
  wouldn't parse is refused, with the reason shown in the dialog.

The sidebar and canvas refresh from the file watcher, and the artboard stays
on the size just added or edited.

### Persistence

The last chosen size is remembered per viewer, by key. A remembered size that
the next space or page doesn't have falls back to the page's first listed
size, then the space's first size.

## Addresses

The size part of an address is the size's key:

```text
#design/sidebar.html@sidebar
#pages/sign-in.html:error@mobile~staging
```

- An address naming a size the page doesn't have is rewritten to the size
  actually shown, as an unknown width is today
  ([core](core.md#selection-and-navigation)).
- Keys are kebab-case, so they never contain the `:`, `!`, `~`, or `@` that
  mark the other parts of an address.
- An address in a docs lens still has no size part.

## Handoffs, references, and agents

- A handoff's size line names the size with its label and declared
  dimensions, then the real artboard size:
  `Size: Sidebar, 340 × fill (artboard is 340 × 812 CSS px)`. Fit and
  Resizable have no declared dimensions: `Size: Resizable (artboard is
  1024 × 768 CSS px)`. A docs lens keeps its own line.
- Handoff screenshot names include the size key, such as
  `sign-in-error-mobile.jpg`, so shots of one page at two sizes don't
  overwrite each other.
- **Copy reference** and the `/_workbench/view` payload still carry no size
  ([agent context](agent-context.md)).
- The multiple-artboard report keeps its numeric `size` per artboard.

## Design-system export

Each page is captured once per size it supports, except Resizable:

- A fixed size is captured at its dimensions. A filled axis takes Fit's export
  length on that axis: 1440 wide or 900 tall. Fit is 1440 × 900, and the
  Sidebar exports at 340 × 900.
- Resizable has no fixed size and isn't captured. A page whose only size is
  Resizable is captured at 1440 × 900.
- Sizes that resolve to the same dimensions are captured once, under the first
  of them in the page's order.
- File names use the size's key and the README uses its label, such as
  `default-sidebar.jpg` under *Sidebar · `340 × 900`*.

A page without `sizes` is captured at every size of the space. A space
chooses its sizes deliberately, so the export shows its pages at those sizes;
a space that wants fewer captures for a page lists that page's `sizes`.

## Configure pages

The size cards in **Configure pages** list the space's sizes with their icons
and dimensions, and turn each on or off for the page. A page's own sizes
follow as cards marked as the page's own; they are always on there and are
changed in the YAML. Choosing every space size removes the page's `sizes`
when it has no sizes of its own, and otherwise leaves its own sizes alone.

## TypeScript previews

- A preview definition's `sizes` lists size keys, resolved against the space the preview is shown in, typed
  `SizeKey = 'fit' | 'laptop' | 'mobile' | 'resizable' | (string & {})`.
- The compiler checks only that each entry is a non-empty kebab-case string.
  The server resolves keys against the space and reports an unknown one,
  naming the definition.
- The portable viewer's index carries each preview's resolved sizes, so its
  **Size** menu offers them without knowing the space. A filled axis there
  takes the viewer's available length.
- A preview placed by an authored page takes that page's `sizes` when it
  lists them, and its definition's otherwise.

## Multiple artboards

- A size resolves to numeric CSS pixels when an artboard takes it: a fixed
  axis exactly, and a filled axis at the canvas's length at that moment.
  After that the artboard is an ordinary numeric size; it doesn't follow the
  canvas.
- `supportsSize` accepts a size the page supports when the fixed axes match
  exactly; a filled axis accepts any length. Artboard dimensions range from
  1 to 8192 pixels.
- The fallback for an unsupported size is the page's first listed size,
  instead of Laptop or Mobile.
- Artboards size themselves directly, whatever sizes the space lists. A space
  that leaves out `resizable` still gets working artboards; their child
  runtimes don't depend on the space having a Resizable size.

## Acceptance criteria

1. A space without `sizes` shows Fit, Laptop, Mobile, and Resizable as
   buttons, and behaves as before; its addresses use `@fit`, `@laptop`,
   `@mobile`, and `@resizable`.
2. With the `sizes` example above and the Sidebar page listing
   `[sidebar, resizable]`, opening the page from Laptop shows a
   340-pixel-wide artboard as tall as Fit's artboard; the label reads
   `340 × <height>`; the address ends in `@sidebar`; Sidebar is a button and
   Tablet is in **More sizes**, disabled.
3. Resizing the window changes that artboard's height and not its width;
   zooming changes neither.
4. On the Account menu page, **More sizes** lists Popover (280 × 360), and
   the space's buttons other than Sidebar are disabled. A page entry
   `- sidebar: { width: 300 }` is reported and skipped. A page listing only
   `popover` can't be switched to any other size.
5. Opening `@tablet` where the page doesn't have it shows the page's first
   size and rewrites the address.
6. **Custom size…** with *Tablet*, 1024, 1366 in a space without `sizes`
   writes `fit`, `laptop`, `mobile`, `resizable`, and `tablet` into the
   space's own entry, keeps comments elsewhere, and shows the page at
   1024 × 1366. With **Only for this page**, it writes to the page's `sizes`.
7. **Edit sizes…** reorders buttons, changes icons, and removing `tablet`
   removes it from `sizes` and every page's `sizes` in one write.
8. Invalid sizes, `fill` on both axes, a top-level `sizes` in a multi-space
   file, and unknown page entries are each reported in the problems list with
   where they came from. The browser and server readers report the same
   problems.
9. A handoff at the Sidebar size names *Sidebar, 340 × fill* and the real
   artboard size.
10. The export of the Sidebar page contains a 340 × 900 capture labeled
    *Sidebar*, and nothing for Resizable.
11. A `workbench.local.yaml` that sets `sizes: { sidebar: { width: 360 } }`
    for the space changes only Sidebar's width; Edit sizes… shows Sidebar
    read-only.
12. Handoffs of one page and state at Laptop and at Mobile save two
    screenshots, not one.
13. A space whose `sizes` leaves out `resizable` still opens multiple
    artboards at the sizes requested.

Tests: `manifest.test.js` for the schema, defaults, page limits and own
sizes, the local merge, and problems; `address.test.js` for size keys;
`preview.test.js` for the switcher, fill sizing, and fallbacks;
`config.test.js` and `server.test.js` for both readers agreeing, writes
(including comments kept outside replaced blocks), and export plans;
`handoff.test.js` for size lines and screenshot names;
`src/canvas/canvas.test.ts` for multiple-artboard sizes. The size switcher,
menu, and dialogs need a browser check of keyboard use and focus with the
[browser harness](../scripts/chrome.cjs).

## Implementation notes

Replacing the fixed sizes touches code that assumes them
([inventory, 2026-10-04](#discoveries)). In particular:

- The size buttons are static markup in `index.html`, and the valid widths are
  read from their `data-width` attributes. The switcher has to be drawn from
  the space's resolved sizes, and addresses validated against them.
- `setWidth` treats any non-built-in mode as a number of pixels and takes the
  height from the pressed button. Sizes need one resolved table, with width,
  height, and fill per axis, shared by the shell, export, handoff, and the
  multiple-artboard adapter.
- `zoom.js` sizes Fit on both axes together; fill needs the same logic per
  axis.
- The CSS sizes icons by literal `data-width` values; icons from the space need
  one rule.
- The preview compiler and its types, the config editor's cards, the export's
  size table, the portable viewer's modes, and `src/canvas/model.ts` each
  hard-code the four sizes. `model.ts`, `legacy-runtime.ts`, and the Resizable
  rails in `workbench.js` bound sizes at 320, and the portable viewer at 240.
- `manifest.merge` replaces a space's keys wholesale except `implementations`;
  `sizes` needs the same key merge, inside `spaces` entries too.
- The writer in `config.js` replaces top-level blocks and
  `spaces.<key>.collections`; it needs the same for `spaces.<key>.sizes`.
- Multiple-artboard children open at `@resizable`, and
  `wbArtboardShell.size` switches to Resizable; that path has to set the
  artboard's size without going through the space's sizes.
- Shield's policy hook (`shield/src/adapters/policy-hook.mjs`) recognizes
  Workbench addresses only with `fit`, `resizable`, `390`, `393`, or `1512`;
  it has to accept any kebab-case size key. The shared Workbench skill and
  rule describe the size part of an address and change with it.
- `annotations.js` builds handoff screenshot names and the width line from the
  pressed button's title; both move to the resolved size.

## Decisions

- **2026-10-04: one word, size.** Spaces, pages, preview definitions, the
  interface, the export, and handoffs say `sizes` and "size". A term names
  one thing ([terminology](terminology.md)), and two words for an artboard's
  dimensions would not.
- **2026-10-04: each space defines its sizes.** A space owns its sizes,
  including which are buttons, their icons, and their order. Sizes aren't
  shared between spaces. A space without `sizes` gets the default four.
- **2026-10-04: pages limit and add, nothing else.** A page can limit which of
  the space's sizes it supports, down to one so it can't be switched, and can
  add sizes of its own, shown in the size menu. It can't change a space size
  or the size switcher's buttons, which belong to the space.
- **2026-10-04: Custom size… creates a named size in `workbench.yaml`.** It
  writes to the space by default, or to the page with **Only for this page**.
  There is no per-viewer ad hoc size besides Resizable.
- **2026-10-04: no new default sizes.** The defaults stay Fit, Laptop, Mobile,
  and Resizable. A concept with Desktop, Tablet, Small phone, and Android
  presets was considered; those are sizes a space declares.
- **2026-10-04: no minimum beyond 1 pixel.** Small components need small
  artboards. Sizes and Resizable's drag go down to 1 × 1. A filled axis keeps
  Fit's 320-pixel floor, since it follows the canvas rather than a choice.
- **2026-10-04: icons are configurable.** Every size takes a Lucide `icon`.
- **2026-10-04: the space's order is the menu's order.** The space decides,
  as it does for buttons.
- **2026-10-04: export every supported size but Resizable.** See
  [Design-system export](#design-system-export).
- **2026-10-04: dialogs write like Configure pages.** Writes that change pages
  replace the `collections` block and lose comments inside it, as **Configure
  pages** does. A line-level YAML editor that keeps them was considered and
  left for later; both features would use it.
- **2026-10-04: a placed preview takes the page's sizes, else its definition's.**
  A page that lists the preview and its `sizes` decides; without them, the
  definition's apply. Before, readers filled every page's sizes, so a
  definition's never did.
- **2026-10-04: no compatibility with the old names.** Workbench has no users
  yet, so `@1512` and `@393` addresses aren't mapped to the new keys.

## Current status

Verified on 2026-10-04 against the working tree.

- **Built:** the size model, schema, export sizes, and edits in
  [src/sizes/](../src/sizes/), with tests beside each; the server reader and
  writer in [config.js](../config.js); `/_workbench/sizes` in
  [server.js](../server.js); the canvas's size choice, fill axes, keyed
  addresses, and pinned artboard sizes in
  [workbench.js](../workbench/workbench.js) and [zoom.js](../workbench/zoom.js),
  through the [bridge](../src/sizes/browser/bootstrap.ts); handoff size lines
  and screenshot names; the export plan and README labels; preview keys and
  the portable viewer; the multiple-artboard model; Shield's pattern. This
  repository's Sidebar page opens at its `sidebar` size, checked in Chrome at
  340 × 836 on a 956-pixel canvas.
- **Sizes are resolved on the server.** The browser's YAML reader and the
  sidebar webview don't resolve them; the canvas reads the space's sizes and
  each page's from `/_workbench/config`, and the defaults when no server
  answers. A canvas opened off disk can't load the size module and has Fit
  only.
- **Configure pages** keeps a page's sizes as written, its own sizes
  included.
- **Interface:** [`wb-size-switcher`](../src/components/size-switcher/element.ts),
  [`wb-size-dialog`](../src/components/size-dialog/element.ts), and
  [`wb-sizes-editor`](../src/components/sizes-editor/element.ts) are Web
  Components ([contract](web-components.md)), in the middle of the top bar
  beside the lens switcher. The canvas controller feeds them and posts to
  `/_workbench/sizes`. Their docs pages are in [design/](../design/); a
  real-Chrome test covers inputs, events, keyboard, focus, the dialogs, and
  reconnection. Checked end to end in Chrome on a copy of the Acme demo:
  picking, adding a Sidebar size from a page, and reordering in Edit sizes.
- **To do:** Configure pages cards for every size of the space (they still
  show the default sizes the space has), and the guides listed at the end.

## Implementation order

Each step ships working and tested on its own:

1. **Schema and readers.** Space and page `sizes`, defaults, own sizes,
   problems, the local merge, and the resolved shape, in `manifest.js` and
   both readers. The canvas keeps its four buttons, mapped from the resolved
   default sizes.
2. **Canvas.** The size switcher drawn from the space's sizes, the size menu
   without its actions, fill sizing in `zoom.js`, addresses and persistence by
   key, and the Resizable drag down to 1. This repository's Sidebar page moves
   to a `sidebar` size.
3. **Handoff, export, previews.** Handoff size lines and screenshot names, the
   export plan, preview definition keys, the portable viewer, and the
   multiple-artboard model, plus Shield's address pattern.
4. **Writing.** **Custom size…**, **Edit sizes…**, the `sizes` writer, and the
   Configure pages cards.
5. **Documentation.** The guides, README, shared skill and rule, website, and
   changelog listed below, for each step's visible change as it ships.

## Discoveries

- **2026-10-04: the four sizes are hard-coded in many places.** An inventory
  of the package found them in the manifest reader, the preview compiler and
  its types, the config editor, the address router, the size buttons and their
  CSS, zoom, the multiple-artboard model and controller, the export plan, the
  portable viewer, handoff text, the user guides, the website overview, and
  the shared Workbench skill and rule. Screenshot names in
  `.canonic/.handoffs/` don't include the size, so shots of one page at two
  sizes overwrite each other; that predates this work.

## Documentation to update when shipped

[Configuration](../docs/configuration.md) (`sizes` on spaces and pages),
[Using the canvas](../docs/canvas.md) (size switcher, size menu, addresses,
Configure pages), [Pages and states](../docs/pages-and-states.md),
[TypeScript previews](../docs/workbench-previews.md), [Design-system
export](../docs/design-system-export.md), the iOS Simulator and window guides,
which recommend a mobile size for streamed lenses, the [workbench
README](../workbench/README.md), the shared Workbench skill and rule, the
website overview's size switcher and address text, and the changelog.
