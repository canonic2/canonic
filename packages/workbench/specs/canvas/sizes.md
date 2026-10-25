# Artboard sizes

**Status:** implemented: the size schema and defaults, server-resolved sizes,
fill axes, keyed addresses, the size switcher and menu, Custom size… and Edit
sizes…, handoff size lines, export sizes, preview keys and the portable viewer,
and multiple-artboard sizes. Agreed but not implemented: Configure pages cards
for every size of the space, keeping declared artboard sizes that a page's
`sizes` don't accept, and public guides for the size controls; see
[implementation gaps](#implementation-gaps).

## Purpose and scope

A space defines its artboard sizes: dimensions, labels, icons, order and which
appear as buttons. A space without configuration gets Fit, Laptop, Mobile and
Resizable. A page may limit those choices and add its own sizes, but cannot
change the space's size definitions or buttons. Either axis can fill the canvas.
This supports screens, narrow panels, popovers and other components at their
intended dimensions. Custom size… and Edit sizes… write `workbench.yaml`.

The contract covers configuration, canvas sizing, addresses, handoffs, export,
Configure pages, TypeScript definitions, the portable viewer and the
multiple-artboard model. Docs lenses use their own canvas layout. Simulator
and window content keeps its native dimensions, letterboxed inside the chosen
artboard. Resizable dimensions are viewer-local and shared across pages.

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
([core](../core.md#problem-reporting)), for example:

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
  from a full-height artboard, which [docs pages](../pages/docs-pages.md) do not
  use because it makes `vh` resolve against the whole document.

## Resolved sizes

Both config readers resolve sizes with the same rules from `manifest.js`, as
they do for implementations ([local overrides](../pages/page-tree.md#local-overrides)). The
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

Sizes are resolved on the server. The browser's YAML reader and the sidebar
webview don't resolve them; the canvas reads the space's sizes and each page's
from `/_workbench/config`, and the defaults when no server answers. A canvas
opened off disk can't load the size module and has Fit only.

## The size switcher

The switcher, the Custom size… dialog and the Edit sizes… editor are the
[`wb-size-switcher`](../../ui/src/components/size-switcher/element.js),
[`wb-size-dialog`](../../ui/src/components/size-dialog/element.js) and
[`wb-sizes-editor`](../../ui/src/components/sizes-editor/element.js) Web
Components ([contract](../ui/web-components.md)), in the middle of the top bar
beside the lens switcher. The canvas controller feeds them and posts to
`/_workbench/sizes`.

The space's sizes with `button` are buttons, in the space's order, each
showing its icon, with its label and dimensions as its title and accessible
name. A menu button follows them and opens the **size menu**:

| Section | Entries |
| --- | --- |
| **Sizes** | The sizes that are buttons, with their icons and dimensions |
| **More sizes** | The space's other sizes, then the page's own sizes, with their icons and dimensions, such as `340 × fill`. Hidden when there are none. |
| (actions) | **Custom size…** and **Edit sizes…**, shown when the workbench server is running, as **Configure pages** is |

- The current size is checked. Sizes the page doesn't support are disabled,
  with the reason in their titles. A page's
  own sizes are listed only while that page shows.
- When the current size isn't a button, the menu button shows that size's
  icon and is pressed, and its accessible name and title name the size, such
  as *Sidebar, 340 × fill*. Otherwise it shows a chevron.
- In a docs lens, every size is disabled and nothing is pressed or checked.
- Source format does not disable sizes: a Markdown-backed page's Storybook or
  URL lens supports the space's sizes. Returning from Docs restores those
  controls; selecting a size resizes the retained preview without reloading it.
- Reusing an ordinary renderer applies the requested preset when metadata for
  the matching destination arrives. Docs viewport dimensions do not become the
  next preview's dimensions. A size pick replaces that pending choice. Each
  choice is applied once so later content-fit measurements can settle normally;
  explicitly declared artboards retain their authored sizes during navigation
  ([gap](#implementation-gaps)).
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
pages** does ([extension](../integrations/vscode-extension.md)), never `workbench.local.yaml`
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

The last chosen size is remembered per viewer, by key, under the
[persistence contract](persistence.md#what-is-saved). A remembered size that
the next space or page doesn't have falls back to the page's first listed
size, then the space's first size.

## Addresses

The size part of an address is the size's key:

```text
#design/sidebar.html@sidebar
#pages/sign-in.html:error@mobile~staging
```

- An address naming a size the page doesn't have is rewritten to the size
  actually shown ([core](../core.md#selection-and-navigation)).
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
- **Copy reference** and the `/_workbench/view` payload carry no size
  ([agent context](../integrations/agent-context.md)).
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
- File names use the size's key and the README heading uses the reference's
  label and the size's label, with the dimensions on the line below: such as
  `default-sidebar.jpg` under `### Default · Sidebar`, then `340 × 900`.

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

- A scenario's declared artboards use their authored `width` and `height`
  (integers from 1 to 8192), independent of the space's and the page's sizes;
  the size switcher does not resize them. A scenario's implicit artboard follows
  the size switcher like any single view. See
  [artboards and iterations](../pages/iterations.md).
- A size resolves to numeric CSS pixels when an artboard takes it: a fixed
  axis exactly, and a filled axis at the canvas's length at that moment.
  After that the artboard is an ordinary numeric size; it doesn't follow the
  canvas. Returning to one artboard does not implicitly change its dimensions.
  Native content retains its renderer's aspect ratio and coordinate mapping.
- `supportsSize` accepts a size the page supports when the fixed axes match
  exactly; a filled axis accepts any length. Artboard dimensions range from
  1 to 8192 pixels.
- The fallback for an unsupported size is the page's first listed size. It applies to the implicit artboard, not to
  declared artboards ([gap](#implementation-gaps)).
- Artboards size themselves directly, whatever sizes the space lists. A space
  that leaves out `resizable` still gets working artboards; their child
  runtimes don't depend on the space having a Resizable size.

## Acceptance criteria

1. A space without `sizes` shows Fit, Laptop, Mobile, and Resizable as
   buttons, and uses those defaults; its addresses use `@fit`, `@laptop`,
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
12. Handoffs of one page and scenario at Laptop and at Mobile save two
    screenshots, not one.
13. A space whose `sizes` leaves out `resizable` still opens multiple
    artboards at the sizes requested.

Tests: `manifest.test.js` for the schema, defaults, page limits and own
sizes, the local merge, and problems; `address.test.js` for size keys;
`server/node/preview.test.js` for the switcher, fill sizing, and fallbacks;
`server/node/config.test.js` and `server/node/server.test.js` for both readers resolving the same sizes and problems, writes
(including comments kept outside replaced blocks), and export plans;
`server/src/handoff/tests.rs` for size lines;
`ui/src/modules/canvas/canvas.test.js` for multiple-artboard sizes;
[`size-switcher/element.test.js`](../../ui/src/components/size-switcher/element.test.js),
a real-Chrome test, for the components' inputs, events, keyboard use, focus,
the dialogs and reconnection, with
[`size-dialog/form.test.js`](../../ui/src/components/size-dialog/form.test.js) and
[`sizes-editor/rows.test.js`](../../ui/src/components/sizes-editor/rows.test.js)
for the dialog and editor rules. Not covered: host acceptance of picking,
adding and reordering sizes, in particular in the VS Code host.

## Implementation gaps

- Configure pages must list every size resolved for the space, including
  page-owned sizes, instead of limiting cards to the default sizes the space
  has. It keeps a page's sizes as written, its own sizes included.
- Declared artboards can lose their authored size: the view report for every
  instance, declared or implicit, carries the page's sizes
  (`ui/src/modules/preview-sessions/content.js`), and the canvas controller
  replaces an instance size that none of them accepts with the page's first size
  (`ui/src/modules/canvas/controller.js`).
- Public guides and the website must describe the configured size switcher,
  editing actions and keyed addresses (listed below).
- The size controls are not verified in the VS Code host.

## Public-guide alignment

[Configuration](../../docs/configuration.md) (`sizes` on spaces and pages),
[Using the canvas](../../docs/canvas.md) (size switcher, size menu, addresses,
Configure pages), [Pages and scenarios](../../docs/pages-and-scenarios.md),
[TypeScript previews](../../docs/workbench-previews.md), [Design-system
export](../../docs/design-system-export.md), the iOS Simulator and window guides,
which recommend a mobile size for streamed lenses, the [workbench
README](../../ui/legacy/README.md), the
website overview's size switcher and address text, and the changelog.
