# The page tree

This topic owns the upper levels of the page hierarchy
([glossary](../glossary.md#the-page-hierarchy)): spaces, collections, groups
and pages, how `workbench.yaml` and imported pages make them, and how the
sidebar lists them. [Scenarios](scenarios.md) own the level below a page, and
[artboards and iterations](iterations.md) the levels below a scenario. Space
switching in the editor belongs to [VS Code](../integrations/vscode-extension.md#spaces),
space sizes to [sizes](../canvas/sizes.md), and problem reporting to
[core](../core.md#problem-reporting).

**Status:** spaces, local overrides, authored and imported pages, merging,
ordering, icons and the page list are implemented. The server does not check
an authored preview page's per-scenario paths when the page index doesn't run;
see [implementation gaps](#implementation-gaps).

## The levels

```text
Space › Collection › (Group) › Page
```

- A **space** is collections of pages served together: one `workbench.yaml`,
  or one entry of its `spaces`, with its own root and Workbench server.
- A **collection** is an ordered list of pages and groups, shown in the
  collection list.
- A **group** is a named, ordered list of pages inside a collection. Groups
  don't nest.
- A **page** is one entry the page list shows under a collection or group:
  an authored HTML page, a TypeScript preview, a Markdown page, or a catalog
  page. Its identity is its `src`.

## Spaces

`workbench.yaml` declares one space, or several under `spaces`:

- **One space.** The file's own keys describe it. With no `name`, it is named
  after its root folder everywhere: the space switcher, the canvas tab and the
  breadcrumb.
- **Several spaces.** `spaces` maps kebab-case IDs to entries; a non-map entry
  or another ID is dropped with a problem. Each entry may set `name`, `color`,
  `icon`, `root` and `sizes`, which belong to that space only and are never
  inherited; every other key falls back to the file's top level, and a key the
  space sets replaces the top-level one, so top-level `collections` are
  shared by spaces that declare none, except `implementations`, which merge
  with the space's own by name (`selectSpace` in
  [manifest.js](../../contracts/manifest.js), which the server's reader
  matches). A top-level `sizes` beside `spaces` is
  a problem. A space's name defaults to its ID in sentence case
  (`design-system` → *Design system*). An unknown space asked for at startup
  falls back to the first, with a problem.
- `root` is a folder path relative to the file's folder, not a URL; a value
  that isn't a folder path is a problem. Without one, the space is served from
  the file's folder, so several spaces may share one root.
- `color` is one of nine named colors (`blue`, `green`, `orange`, `purple`,
  `pink`, `teal`, `red`, `yellow`, `gray`) or a hex value. Without one, the
  space takes one of six colors chosen from a hash of its ID, except behind an
  image icon, which gets none.
- `icon` is a Lucide icon name or a project-relative `.svg`, `.png`, `.jpg`,
  `.webp` or `.gif` file of at most 256 KB. A missing, oversized or escaping
  image falls back to the name's initial, and a missing one is a problem.
  `color` and `icon` are validated by `spaceMark` in
  [manifest.js](../../contracts/manifest.js) for both readers; an invalid value
  is a problem and left out.

### Local overrides

`workbench.local.yaml` is optional and merges over the committed file
(`merge` in [manifest.js](../../contracts/manifest.js)):

- A top-level value it sets replaces the committed one, except as below.
- `spaces` merge by key, each space as the whole file merges.
- `implementations` and `sizes` merge by key, one level deep: a local entry's
  fields replace the committed entry's, so a nested `start` block supplied
  locally replaces the whole committed `start` block.
- A local `collections` or `previews` value replaces the committed one.

## Collections, groups and pages in `workbench.yaml`

- A **page** entry needs `label` and `src`. `src` is relative to the space
  root, without `..`, and cannot contain a mark the
  [address](../core.md#selection-and-navigation) reserves in page paths. A
  `src` ending in `.md` is a Markdown page.
- A **group** entry (`group:`) needs a name and at least one page. A group
  inside a group is dropped.
- A **collection** needs a `name` and either items or an `icon`. A collection
  with only an icon is a placeholder that fixes the order and icon of a
  collection imported pages fill.
- An entry that breaks these rules is dropped with a problem naming its
  location by name, such as `Pages › Auth › Sign in: …`. Every reader reports
  the same problems: the browser reader the canvas and sidebars use, and the
  server's readers behind `/_workbench/config`. Every sidebar shows them in its
  problems list ([core](../core.md#problem-reporting)).
- **Duplicates.** A second collection with a name already used, or a second
  page with a `src` already in the tree, is dropped with a problem; the first
  is kept.
- `collections` can be left out when previews or a catalog supply the pages.
  When no collection has a page and nothing can be imported, the space has
  *nothing to show*, reported as a problem.

## Imported pages

Pages also come from discovered definitions and catalogs, and join the
authored tree.

- **Previews and docs definitions** are placed by their `title`, split on `/`
  with empty parts dropped:

  | Title | Collection | Group | Page label |
  | --- | --- | --- | --- |
  | `Button` | *Previews* (docs: *Docs*) | — | Button |
  | `Components/Button` | Components | — | Button |
  | `Components/Forms/Text input` | Components | Forms | Text input |
  | `Components/Forms/Inputs/Text` | Components | Forms / Inputs | Text |

  With no title, the label is the definition's ID. The middle parts make one
  group, so groups never nest.
- **Catalog pages** form one collection named by the catalog
  implementation's `label`, with its `catalogIcon` (`smartphone` by default):
  one page per booted device matching `device`, or every booted device
  ([implementations](implementations.md)).
- A discovered definition whose path contains a mark the
  [address](../core.md#selection-and-navigation) reserves is a problem naming
  the file and is not listed, since no address could name it.

### Merging

Each surface reads the authored tree from `workbench.yaml` and merges the
server's imported collections into it with the shared `mergeCollections` rule
([manifest.js](../../contracts/manifest.js)):

- An imported collection joins the first authored collection with the same
  name; an imported group joins a top-level group with the same name in that
  collection. Otherwise they are appended.
- An imported preview whose `src` an authored page already lists is not added
  again. The authored page keeps its label, lenses and code pointers and takes
  the definition's scenarios; it takes the definition's lens label and icon
  only when it has none. The file must still match discovery. Its per-scenario implementation paths are checked
  against those scenarios after the merge. A discovered docs page is skipped
  when an authored page lists its `src` or Markdown.
- Groups with exactly the same name merge across authored pages, previews and
  catalogs within one collection: the first keeps its position and metadata,
  and later pages append in source order. Matching names in different
  collections stay separate, as do names that differ in case.
- Groups and collections left empty by the merge are removed, including an
  icon-only placeholder collection no import fills or whose catalog failed.
  Placeholders survive both config readers and **Configure pages** saves.
- Merged collections take their icon from the [collection icon](#icons)
  ladder.

### Order

- Authored collections, groups and pages keep their order in `workbench.yaml`.
- Imported collections that join no authored one are appended: docs pages,
  then previews, then catalogs.
- Discovered definitions arrive in definition-file path order (depth first,
  locale-compared), not title order. Within a collection, imported pages and
  groups come after authored items, and an imported group sits where its first
  page arrived.

## The page list

- The collection list shows every collection with its page count (pages in
  groups included, scenarios not). It is hidden when the space has one
  collection.
- Picking a collection shows its pages and opens its first page, unless the
  page showing is already in it.
- A group's header opens its first page; its caret folds it. Groups start
  expanded.
- A page with two or more scenarios lists them under it, expanded while it is
  showing ([scenarios](scenarios.md#in-the-page-list)). Iterations are not
  listed.
- Every page shows an icon ([icons](#icons)); groups show `folder` and
  scenarios `circle-dot`.
- Search matches page labels, `src` and scenario labels across collections,
  and keeps a group whole when its name matches; collection names are not
  searched. Results are grouped by collection only when several match.
- Folds set by hand survive refreshes; navigation from elsewhere reveals the
  page's ancestors.

### Icons

Page and collection icons are Lucide icon names. Validation checks kebab-case
syntax, not membership in Lucide. An invalid `icon` on a definition fails that
definition alone, and other definitions still load; an invalid
`previews.icon`, `previews.icons` or catalog icon value is reported as a
problem and ignored.

Each page's icon is the first that applies:

| Page | Icon |
| --- | --- |
| Written in `workbench.yaml` | Its own `icon`, else its collection's icon |
| Written in `workbench.yaml` with a preview definition as `src` | Its own `icon`, else the preview's icon below |
| TypeScript preview | The `definePreview` `icon`; else the `previews.icons` value of the longest title prefix that matches whole `/` segments, an exact title included; else `previews.icon`; else `component` |
| `defineDocs` page | The definition's `icon`, else `book-open` |
| Simulator catalog page | Its catalog's icon: the implementation's `catalogIcon`, else `smartphone` |

A definition's icon never sets its collection's icon. Each collection's icon
resolves against the collection name, highest first:

1. an icon written on the authored collection;
2. an icon configured for the import: the `previews.icons` value of the
   longest prefix matching the collection name (collections of previews and
   docs pages), or a catalog's `catalogIcon`;
3. `previews.icon` (collections of previews and docs pages);
4. the authored default, `file-text`, for an authored collection written
   without an icon;
5. the import's built-in default: `component` for previews, `book-open` for
   docs pages, `smartphone` for a Simulator catalog.

When imports offer different icons at the same level, the alphabetically first
icon name wins, so import order never changes the result. An import that joins
a collection another import created first still applies a higher icon.
`mergeCollectionIcon` in [manifest.js](../../contracts/manifest.js) owns
prefix matching and collection icon merging; the server's catalog import
([import.rs](../../server/src/catalog/import.rs)) matches it.

## Verification

`server/node/config-parity.test.js` runs the browser, Node and native readers
over one corpus and compares their trees and problems, including duplicates,
dropped entries, lone scenarios with path maps and *nothing to show*.
`contracts/manifest.test.js` covers merging, preview-`src` path maps and the
icon ladder; `ui/src/views/page-list/page-list.test.cjs` covers the page list.

## Implementation gaps

- **Preview-`src` path maps without an index.** When the page index doesn't
  run (an untrusted workspace or an index failure), the server never checks
  an authored preview page's per-scenario paths, so their problems aren't
  reported; the browser still applies the check to its own tree.
