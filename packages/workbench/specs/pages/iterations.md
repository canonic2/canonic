# Artboards and iterations

Status: agreed 2026-10-24. The model, render, address, review output and export
are implemented; canvas presentation is not ([gaps](#implementation-gaps)).
Iteration UI starts after the whole browser UI is ported to Lattice
([browser source ownership](../system/modules.md#browser-source-ownership)).

This topic owns the two levels below a scenario: where artboards are declared,
what an iteration is, how both change a render, and how they are named in
addresses, review output and agent context. The
[glossary](../glossary.md#the-page-hierarchy) defines the terms,
[scenarios](scenarios.md) owns the level above, and
[multiple artboards](../canvas/multiple-artboards.md) owns artboard instances on
the canvas: their isolation, geometry and review composition.

## Purpose

People design inside the codebase and need to try ideas: another table layout,
a different empty state, a bolder warning. A new page or scenario for each idea
would mix "what situation is the page in" with "which design are we trying" and
fill the sidebar with experiments. An iteration is another take on one
artboard, kept beside it until someone picks it, so ideas can be compared
without new pages, scenarios or artboards.

## The levels

```text
Page › Scenario › Artboard › Iteration
```

- A **scenario** has either **declared artboards** or one **implicit
  artboard**:
  - A declared artboard is an entry in the scenario's `artboards`: an ID,
    label, fixed width and height, inputs and iterations. A scenario's declared
    artboards are shown together, each at its own size; the size switcher does
    not resize them.
  - A scenario that declares no artboards has one implicit artboard, which
    shows the page at the size picked in the size switcher.
- An **artboard** has zero or more **iterations**. With none selected, it
  shows its design as authored.
- An iteration belongs to exactly one artboard of one scenario: each
  `(scenario, artboard)` pair declares its own. Different scenarios can declare
  different artboards.
- Only the implicit artboard makes "one scenario, one artboard" the same as a
  page that declares neither. A single declared artboard is different: it has a
  fixed size, gives the preview `context.artboard`, is addressed as
  `+<artboard>/<iteration>`, and exports once at its own size.
- Size and lens are not levels; they are how an artboard is viewed
  ([lenses](#lenses)).

## Declaring artboards and iterations

In a TypeScript preview definition:

```ts
definePreview({
  id: 'feedback',
  adapter: 'react',
  source: { entry: './feedback.tsx' },
  scenarios: {
    default: {
      artboards: {
        info: { label: 'Info', width: 480, height: 320, inputs: { tone: 'info' } },
        warning: {
          label: 'Warning', width: 480, height: 320, inputs: { tone: 'warning' },
          iterations: {
            bold: { label: 'Bold', source: { entry: './feedback.bold.tsx' } },
          },
        },
      },
    },
    empty: {
      requests: { 'GET /api/items': { body: [] } },
      // No artboards: these are the iterations of the scenario's implicit artboard.
      iterations: {
        illustrated: { label: 'Illustrated', source: { entry: './feedback.empty-illustrated.tsx' } },
      },
    },
  },
});
```

- `scenarios.<id>.artboards` maps 1–32 artboard IDs to
  `{ label?, width, height, inputs?, iterations? }`. Width and height are
  integers from 1 to 8192 CSS pixels, independent of the space's sizes. Any
  other artboard field is a problem.
- `iterations` on a declared artboard maps iteration IDs to iteration objects.
  `iterations` directly on a scenario declares the iterations of its implicit
  artboard. A scenario with `artboards` cannot also have scenario-level
  `iterations`; the SDK types make the two exclusive.
- Artboard and iteration IDs follow the shared ID rule
  ([scenarios](scenarios.md#declaring-scenarios)): a lowercase letter, then
  lowercase letters and digits in single-hyphen parts (`compact-table`). They
  are unique within their parent; the same ID under another parent names
  something else. Labels default to the ID in sentence case
  (`compact-table` → *Compact table*).
- A top-level `artboards` or `iterations` key on a definition is a problem.
- An iteration's `inputs`, `fixtures` and `globals` must be maps, `setup`,
  `play` and `ready` functions, its `source.entry` a file inside the project,
  and its `requests` valid request keys; it cannot declare `scenarios`,
  `artboards` or `iterations`.
- Problems name the file and the location, for example
  `previews/feedback.workbench.ts: scenario default › artboard warning: iteration id “Bold” must be kebab-case.`
  **Requirement:** one invalid artboard or iteration does not hide its
  scenario or its siblings ([gap](#implementation-gaps)); other previews still
  load.

### What an iteration changes

An iteration has a scenario's shape without nesting: it can set `label`,
`source`, `inputs`, `fixtures`, `globals`, `requests`, `setup`, `play` and
`ready`.

| Value | Merged in order |
| --- | --- |
| `source` | Iteration, else scenario, else preview: the last one set wins |
| `inputs` | Preview, scenario, declared artboard, iteration, then **Preview controls** overrides, key by key |
| `fixtures`, `globals` | Preview, scenario, iteration, key by key (an artboard has none) |
| `requests` | Matched iteration first, then scenario, then preview ([request mocks](previews.md#data-request-mocks-and-the-project-environment)) |
| `setup`, `play`, `ready` | Run in order: environment, preview, scenario, iteration |

The preview context carries the position: `context.scenario`,
`context.artboard` (a declared artboard's ID; absent for the implicit one) and
`context.iteration` (absent when none is selected). A TypeScript preview also
sets `data-wb-scenario` and, when an iteration is selected, `data-wb-iteration`
on `<html>`. Reported actions carry the iteration.

An iteration with its own `source` is compiled into the preview's entry under
its own key, `<scenario>+<iteration>` or `<scenario>+<artboard>/<iteration>`;
the runtime looks up that key, then a declared artboard's
`<scenario>+<artboard>/`, then the scenario's, then `default`. An Astro
preview pre-renders a declared artboard under `<scenario>+<artboard>/` when it
changes the server render (it has inputs, or request mocks exist, which read
`context.artboard`), and an iteration only when it changes the server render:
its own source, inputs or requests, or fixtures and globals that request mocks
read. An iteration without its own render uses its artboard's.

### Authored HTML pages

Authored HTML pages declare scenarios in `workbench.yaml` and have one implicit
artboard per scenario; they cannot declare artboards. A scenario entry can list
iterations:

```yaml
- label: Sign in
  src: pages/sign-in.html
  scenarios:
    - id: default
      label: Default
    - id: error
      label: Wrong password
      iterations:
        - id: inline-hint
          label: Inline hint
```

- Iteration IDs follow the artboard and iteration ID rule above and are unique
  within the scenario. A label is optional and defaults to the ID in sentence
  case (`inline-hint` → *Inline hint*). An invalid or repeated entry is dropped
  on its own; the page and its other iterations stay.
- The page reads the iteration the way it reads its scenario
  ([pages and scenarios](../../docs/pages-and-scenarios.md)): it loads with
  `?iteration=<id>`, the compatibility script sets `data-wb-iteration` on
  `<html>`, and `data-wb-iteration-only` and `data-wb-iteration-not` keep or
  remove elements. With no iteration selected the attribute is absent and
  `-only` elements are removed.

### Other page kinds

Storybook stories, Markdown pages, docs examples and catalog pages have no
iterations.

## Lenses

Iterations belong to the design lens: they change what the page's own source
renders. Implementation lenses (`url`, `storybook`, `ios-simulator`, `window`)
show the shipped implementation and ignore the selected iteration; a docs lens
renders its examples without iterations. Switching lenses keeps the selected
iterations, so returning to the design lens shows them again.

## Address

The canvas address ([core](../core.md#selection-and-navigation)) names each
shown artboard's selected iteration with a `+` part after the scenario:

```text
#<src>[:<scenario>][+<iteration> | +<artboard>/<iteration> …][!<example>][@<size>][~<lens>]
```

| Address | Shows |
| --- | --- |
| `#jobs.workbench.ts:empty+illustrated@laptop` | The *Empty* scenario's implicit artboard with its *Illustrated* iteration |
| `#feedback.workbench.ts+warning/bold@laptop` | The first scenario's declared artboards, *Warning* with its *Bold* iteration |
| `#feedback.workbench.ts+warning/bold+info/calm@laptop` | Two declared artboards, each with an iteration |

- `+<iteration>` names the implicit artboard's iteration and
  `+<artboard>/<iteration>` a declared artboard's. An artboard appears at most
  once; the workbench writes the parts in the scenario's artboard order. An
  artboard with no part shows its authored design.
- `:`, `+`, `!` and `~` are reserved in page paths, so the parts split
  unambiguously. A `src` containing one is a configuration problem, and a
  discovered definition whose path contains one is a problem naming the file
  and is not listed.
- **Unknown IDs.** On the canvas, a part naming an unknown artboard or
  iteration is dropped: the artboard shows its authored design and the address
  is rewritten to what is shown. Loaded directly, a preview URL with an unknown
  `?artboard=` or `?iteration=` renders an error naming it. An authored HTML
  page ignores an iteration ID that breaks the ID rule.
- **Navigation.** Picking another page or scenario, or following a link,
  starts from the authored design; switching lens keeps the iterations. Under a
  Storybook lens they are kept as given and checked again on the way back to
  the design lens. Links between previews (`links`, `context.navigate`) never
  name iterations.
- **Portable viewer.** The viewer reads `?iteration=` for the implicit
  artboard of the selected scenario and drops it when the scenario changes.

## Identity, review and agents

- An artboard's view is its page, scenario, declared artboard (if any),
  iteration, size and lens. Two views that differ only by iteration are
  different views. A declared artboard's instance is labelled with the page
  and artboard labels (*Feedback / Warning*).
- **Copy reference** adds `- Iteration: <Label> — \`<id>\`` after its Scenario
  line, and `- Iteration source: \`<path>\`` when the iteration has its own
  source. The **handoff** prompt adds `- Iteration: <Label> (<id>)` after the
  Scenario line and lists the source among the code pointers. Agent context
  carries `iteration` on the single view and `artboard` and `iteration` on
  each artboard of a canvas report. Nothing about iterations appears when none
  is selected, or under an implementation lens.
- Annotations belong to the view they were drawn on. Another iteration clears
  them, as another scenario does; a lens or size change keeps them. Screenshot
  names include the iteration.
- The [design-system export](../review/export.md) captures each iteration as
  its own reference beside its artboard's authored design, and "all declared
  variants" includes them. Labels read *Scenario · Artboard · Iteration* for a
  declared artboard and *Scenario · Iteration* otherwise. Under an
  implementation lens the export captures no iterations.
- **Choosing an iteration** happens through a handoff: it names the iteration
  and its source so a person or agent can make it the artboard's design.
  Workbench never rewrites definitions or sources to promote an iteration.

## Decisions

- **2026-10-24, names.** The alternative condition of a page is a *scenario*;
  a take on an artboard's design is an *iteration*. "Variant" was rejected
  because Figma uses it for property combinations, "option" because `options`
  is pervasive in code, "draft" and "version" because they imply unfinished
  work or releases.
- **2026-10-24, hierarchy.** Page › Scenario › Artboard › Iteration, with an
  implicit scenario and artboard. Iterations per page and then per scenario
  were rejected because not every scenario, and not every artboard, has the
  same iterations. Artboards belong to a scenario because different scenarios
  need different artboards; Workbench has no users to migrate from the earlier
  page-level form.
- **2026-10-24, promotion by handoff.** Rewriting sources is project-specific
  and risky; the handoff names the iteration instead.

## Acceptance criteria

1. A definition with scenario-level `artboards` and per-artboard `iterations`
   lists its scenarios in the sidebar and no iterations there.
2. A render with an iteration selected uses the iteration's `source` and the
   merge order above; with none selected the artboard renders as authored.
3. `context.scenario`, `context.artboard` and `context.iteration` match the
   requested view; a direct URL with an unknown artboard or iteration renders
   a named error.
4. Two scenarios of one page declare different artboards; switching scenario
   shows each scenario's own set.
5. Top-level `artboards` or `iterations`, scenario-level `iterations` beside
   `artboards`, unknown artboard fields, invalid iteration fields and invalid
   IDs are problems naming their location; other previews still load.
6. An authored HTML page with a scenario iteration receives
   `data-wb-iteration`, and `-only`/`-not` elements follow it.
7. Implementation lenses ignore the iteration; returning to the design lens
   restores it.
8. Copy reference, handoff and agent context name a selected iteration and its
   source; annotations stay with the iteration they were drawn on.
9. A space export with all declared variants has one reference per iteration,
   in addition to each artboard's authored design.
10. Addresses with `+` parts round-trip through links and restoration; unknown
    parts normalize to the authored design; reserved marks in a `src` or a
    discovered definition's path are problems.
11. The browser, Node and native `workbench.yaml` readers agree on scenario
    iterations, and the Node and native page indexes report the same problem
    for a definition path with a reserved mark, even when the definition
    also fails to evaluate.

## Implementation gaps

- **No canvas presentation.** An iteration is selected only through the
  address; nothing on the canvas shows or picks one
  ([open question 1](#open-questions)).
- **An invalid artboard or iteration fails its whole definition.** Definition
  validation is all-or-nothing, so the scenario and its siblings are hidden
  too. Isolating it needs per-definition problems in the page index.
- **The portable viewer shows no declared artboards.** A scenario with
  declared artboards renders there as one view without `?artboard=`, so no
  artboard inputs apply, and the viewer cannot select a declared artboard's
  iteration.
- **Copy reference doesn't name the declared artboard** a selection shows;
  only the canvas report's JSON carries it.
- **A warm preview document reloads on another iteration.** The in-place
  scenario command does not change the iteration, so the frame reloads.
- **Public guides** (configuration, pages and scenarios, previews) and the
  shared Workbench skill and rule do not describe iterations yet.

## Open questions

1. **Canvas presentation.** How a person sees and picks an artboard's
   iterations, compares an iteration beside its artboard, and sees that an
   artboard has iterations.
2. **Docs lenses.** Whether a docs page's examples can follow the selected
   iteration of the page they document.
