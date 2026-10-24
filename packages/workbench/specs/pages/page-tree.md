# The page tree

This topic owns the upper levels of the page hierarchy
([glossary](../glossary.md#the-page-hierarchy)): spaces, collections, groups
and pages, how `workbench.yaml` and imported pages make them, and how the
sidebar lists them. [Scenarios](scenarios.md) own the level below a page, and
[artboards and iterations](iterations.md) the levels below a scenario. Space
switching in the editor belongs to [VS Code](../integrations/vscode-extension.md#spaces),
space sizes to [sizes](../canvas/sizes.md), and problem reporting to
[core](../core.md#problem-reporting).

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
  `icon`, `root` and `sizes`, which belong to that space only; every other
  key falls back to the file's top level, so top-level `collections` are
  shared by spaces that declare none. A top-level `sizes` beside `spaces` is
  a problem. A space's name defaults to its ID in sentence case
  (`design-system` → *Design system*). An unknown space asked for at startup
  falls back to the first, with a problem.
- `root` is a folder path relative to the file, not a URL.
- `color` is one of nine named colors or a hex value. Without one, the space
  takes one of six colors chosen from a hash of its ID.
- `icon` is a Lucide icon name or an image file of at most 256 KB.

`workbench.local.yaml` merges over the committed file: `spaces` merge by key,
each space as the whole file merges; `implementations` and space `sizes` merge
by key one level deep; a local `collections`, `previews` or nested `start`
block replaces the committed one ([core](../core.md#configuration-and-serving)).

## Collections, groups and pages in `workbench.yaml`

- A **page** entry needs `label` and `src`. `src` is relative to the space
  root, without `..`, and cannot contain the address marks `:`, `+`, `!` or
  `~`. A `src` ending in `.md` is a Markdown page.
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
- A discovered definition whose path contains `:`, `+`, `!` or `~` is a
  problem naming the file and is not listed, since no address could name it.

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
  only when it has none. Its per-scenario implementation paths are checked
  against those scenarios after the merge. A discovered docs page is skipped
  when an authored page lists its `src` or Markdown.
- Groups and collections left empty by the merge are removed.
- **Collection icons** follow one ladder for every import, highest first:
  1. an icon written on the authored collection;
  2. an icon configured for the import: `previews.icons` or `previews.icon`
     for collections of previews and docs pages, or a catalog's `catalogIcon`;
  3. the authored default, `file-text`;
  4. the import's built-in default (`book-open` for docs, `smartphone` for a
     catalog, the preview default for previews).

  An import that joins a collection another import created first still
  applies a higher icon. [Preview placement](previews.md#sidebar-placement)
  details the preview mappings.

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
- A page without its own `icon` shows its collection's icon; groups show
  `folder` and scenarios `circle-dot`.
- Search matches page labels, `src` and scenario labels across collections,
  and keeps a group whole when its name matches; collection names are not
  searched. Results are grouped by collection only when several match.
- Folds set by hand survive refreshes; navigation from elsewhere reveals the
  page's ancestors.

## Verification

`server/node/config-parity.test.js` runs the browser, Node and native readers
over one corpus and compares their trees and problems, including duplicates,
dropped entries, lone scenarios with path maps and *nothing to show*.
`contracts/manifest.test.js` covers merging, preview-`src` path maps and the
icon ladder; `ui/legacy/page-list.test.js` covers the page list.

## Implementation gaps

- **Preview-`src` path maps without an index.** When the page index doesn't
  run (an untrusted workspace or an index failure), the server never checks
  an authored preview page's per-scenario paths, so their problems aren't
  reported; the browser still applies the check to its own tree.
