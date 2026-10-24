# Scenarios

This topic owns scenarios on every kind of page: how they are declared, named
and addressed, which one a page shows by default, and how unknown ones are
handled. The [glossary](../glossary.md#the-page-hierarchy) places them in the
hierarchy; [artboards and iterations](iterations.md) own the levels below a
scenario; each page format's spec owns what a scenario can change there.

## What a scenario is

A scenario is one of a page's alternative conditions, such as *Empty*,
*Loading*, *With filters* or *Wrong password*. Scenarios are alternatives:
the canvas shows one at a time, and the page list lists them under their page.

A page has one or more scenarios. A page or definition that declares none has
one implicit scenario with the ID `default`.

### The first scenario

A page's **first scenario** is its **default scenario**: what the page shows
when the address names no scenario. Its ID is left out of the address, and
the canvas reports it as no scenario. What the page itself sees depends on its
kind:

| Page kind | What the first scenario is called inside the page |
| --- | --- |
| Authored HTML | `default`, whatever its ID: the page loads without `?scenario=` and gets `data-wb-scenario="default"` |
| Docs example | `default`, whatever its ID (`context.scenario`) |
| TypeScript preview | Its own ID (`context.scenario` and `data-wb-scenario`); `default` only when the definition declares no scenarios |
| Implementation path map | Either `default` or the first scenario's own ID |

## Declaring scenarios

| Where | Shape |
| --- | --- |
| `workbench.yaml` page | A list of `{ id, label?, iterations? }` |
| `definePreview` | A map of ID to a scenario object ([previews](previews.md#definition-contract)) |
| `defineDocs` | A map of ID to `{ label? }` ([docs](docs-pages.md#scenarios)) |
| Storybook lens | The title's stories; the part of a story ID after `--` is its scenario, and its label is the story's name ([Storybook](storybook.md#story-identity)) |

- **IDs.** Scenario, artboard and iteration IDs share one rule: a lowercase
  letter, then lowercase letters and digits in single-hyphen parts
  (`with-filters`, `step-2`). IDs are unique within their page.
- **Labels** are optional. Without one, a label is the ID in sentence case
  (`with-filters` → *With filters*).
- In `workbench.yaml`, an entry without an ID, with an invalid ID, or with an
  ID the page already declared is dropped with a problem naming its location;
  the first entry with an ID is kept. A page keeps its scenario list only when
  two or more entries are valid, or when any entry lists
  [iterations](iterations.md#authored-html-pages). A lone scenario still
  counts for its page's implementation path maps.
- In a definition, an invalid scenario fails the whole definition with a
  problem naming the file; other definitions still load.
- A `definePreview` scenario may set `label`, `source`, `inputs`, `fixtures`,
  `globals`, `requests`, `setup`, `play` and `ready`, and either `artboards`
  or `iterations` ([artboards and iterations](iterations.md#declaring-artboards-and-iterations)).
  It merges over the preview's own values; the merge order is in
  [previews](previews.md#definition-contract).

## In the page list

- A page lists its scenarios under it only when it has two or more. They
  expand while the page is showing and when search matches them; search
  matches scenario labels. The row of the scenario showing is marked current,
  including the first scenario, which the canvas reports as no scenario.
- Picking another scenario of a TypeScript preview re-renders it in the warm
  document without reloading, unless the iteration changes too
  ([previews](previews.md#open-scenarios-controls-and-actions)); other pages
  load again with the new scenario.
- A catalog page with scenarios opens its first named scenario.

## Addresses and unknown scenarios

- The address names a scenario after `:`: `#pages/sign-in.html:error@mobile`.
  The first scenario is left out ([core](../core.md#selection-and-navigation)).
- On the canvas, an unknown scenario in the address shows the first scenario
  and the address is rewritten to it.
- Loaded directly, a TypeScript preview with an unknown `?scenario=` renders an
  error naming it. An authored HTML page given an ID that breaks the ID rule
  shows its default.
- Links between previews (`links`, `context.navigate`) may name a scenario
  ([previews](previews.md#links-and-navigation)).

## Implementations

An implementation's mapping can be a path per scenario. The key `default` and
the first scenario's own ID both name the first scenario; a key that names no
declared scenario is a problem ([implementations](implementations.md)). An
authored page whose `src` is a TypeScript preview takes its scenarios from the
definition, so its path maps are checked against the definition's scenarios
once the preview is merged in ([page tree](page-tree.md#merging)).
