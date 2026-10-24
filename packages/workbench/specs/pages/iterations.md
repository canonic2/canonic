# Artboards and iterations

Status: agreed 2026-10-24, not implemented. This topic owns where artboards are
declared, what an iteration is, how it changes a render, and how it is named in
addresses, review output and agent context. How the canvas presents and picks
iterations is open ([open questions](#open-questions)). The
[glossary](../glossary.md#the-page-hierarchy) defines the terms;
[multiple artboards](../canvas/multiple-artboards.md) owns instance isolation,
geometry and review composition.

## Purpose

People design inside the codebase and need to try ideas: another table layout,
a different empty state, a bolder warning. Today the only way is a new page or
a new scenario, which mixes "what situation is the page in" with "which design
are we trying" and fills the sidebar with experiments. An iteration is another
take on one artboard, kept beside it until someone picks it, so ideas can be
compared without new pages, scenarios or artboards.

## The hierarchy

```text
Space › Collection › (Group) › Page › Scenario › Artboard › Iteration
```

- A **page** has one or more **scenarios**. A page that declares none has one
  implicit default scenario.
- A **scenario** has one or more **artboards**. A scenario that declares none
  has one implicit artboard: the page at the selected size. A page with one
  scenario and one artboard is the same as a page that declares neither.
- An **artboard** has zero or more **iterations**. With no iteration selected,
  the artboard shows its design as authored.
- An iteration belongs to exactly one artboard of one scenario. Scenarios do
  not share iterations, and artboards do not share them either: each
  `(scenario, artboard)` pair declares its own.
- Size and lens are not levels. They are how the selected artboard is viewed
  (see [lenses](#lenses)).

There are no page-level artboards. Artboards are declared inside a scenario,
so different scenarios can show different artboards.

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
      // No artboards: these are the iterations of the scenario's one artboard.
      iterations: {
        illustrated: { label: 'Illustrated', source: { entry: './feedback.empty-illustrated.tsx' } },
      },
    },
  },
});
```

- `scenarios.<id>.artboards` is a map of kebab-case artboard IDs to
  `{ label?, width, height, inputs?, iterations? }`. Labels default to title
  case. Width and height follow the existing artboard dimension rules
  ([sizes](../canvas/sizes.md#multiple-artboards)).
- `iterations` on an artboard is a map of kebab-case iteration IDs to
  iteration objects. Labels default to title case.
- `iterations` directly on a scenario declares the iterations of its implicit
  artboard. A scenario that declares `artboards` cannot also declare
  scenario-level `iterations`; that is a problem naming the scenario.
- IDs are unique within their parent: artboard IDs within a scenario,
  iteration IDs within an artboard. The same ID may appear under different
  parents and names different things.
- A top-level `artboards` or `iterations` key on a definition is a problem;
  artboards and iterations are always declared inside a scenario.
- Problems name their location, for example
  `components/feedback › default › warning: iteration id “Bold” must be kebab-case`.
  One invalid iteration does not hide the scenario, its other artboards or
  other previews.

### What an iteration changes

An iteration has the same shape as a scenario: it can set `source`, `inputs`,
`fixtures`, `globals`, `requests`, `setup`, `play` and `ready`. It cannot
declare scenarios, artboards or iterations of its own.

A render merges, in order: the preview, its scenario, the artboard, the
iteration, then control overrides from **Preview controls**. `source`
replaces the one before it. `inputs`, `fixtures` and `globals` merge key by
key. An iteration's `requests` are matched before its scenario's, which are
matched before the preview's ([request mocks](previews.md#data-request-mocks-and-the-project-environment)).
`setup`, `play` and `ready` of the iteration run after the scenario's.

The preview context carries the whole position: `context.scenario`,
`context.artboard` (the declared artboard's ID, or absent for the implicit
one) and `context.iteration` (absent when none is selected). A preview's
address loads a render with `?scenario=`, `?artboard=` and `?iteration=`;
unknown IDs render an error naming them, as an unknown scenario does.

### Authored HTML pages

Authored HTML pages declare scenarios in `workbench.yaml` and have one implicit
artboard per scenario. A scenario entry can list iterations:

```yaml
- label: Sign in
  src: pages/sign-in.html
  scenarios:
    - id: default
    - id: error
      iterations:
        - id: inline-hint
          label: Inline hint
```

The page reads the selected iteration the way it reads its scenario
([pages and scenarios](../../docs/pages-and-scenarios.md)): the page loads with
`?iteration=<id>`, the compatibility script sets `data-wb-iteration` on
`<html>`, and `data-wb-iteration-only` and `data-wb-iteration-not` keep or
remove elements. With no iteration selected the attribute is absent.
Declaring artboards for authored pages is not part of this contract.

### Other page kinds

Storybook stories, Markdown pages, docs examples and catalog pages have no
iterations. Under a Storybook lens the scenario switcher lists stories, and
iterations of the design lens do not apply.

## Lenses

Iterations belong to the design lens: they change what the page's own source
renders. Implementation lenses (`url`, `storybook`, `ios-simulator`, `window`)
show the shipped implementation and ignore the selected iteration. A docs lens
renders examples without iterations. Switching lenses keeps the selected
iteration so returning to the design lens shows it again.

## Address

The canvas address ([core](../core.md#selection-and-navigation)) names each
shown artboard's selected iteration with `+`, after the scenario and before
the size:

```text
#<src>[:<scenario>][+<iteration> | +<artboard>/<iteration> …]@<size>[~<lens>]
```

| Address | Shows |
| --- | --- |
| `#jobs.workbench.ts:empty+illustrated@laptop` | The Empty scenario's implicit artboard with its *Illustrated* iteration |
| `#feedback.workbench.ts+warning/bold@laptop` | The default scenario's artboards, *Warning* with its *Bold* iteration |
| `#feedback.workbench.ts+warning/bold+info/calm@laptop` | Two declared artboards, each with an iteration |

- `+<iteration>` is used only for a scenario's implicit artboard;
  `+<artboard>/<iteration>` only for declared artboards. The other form is an
  unknown iteration.
- An artboard with no `+` part shows its authored design. An artboard appears
  at most once; the workbench writes parts in the scenario's artboard order.
- `+` is reserved in page paths and addresses, as `:`, `!`, `~` and `@` are.
  A `src` containing `+` is a configuration problem.
- An unknown artboard or iteration falls back to the authored design and the
  address is normalized to what is shown, as an unknown scenario is.
- Links between previews (`links`, `context.navigate`) do not name
  iterations: following a link opens the target's authored design.

## Identity, review and agents

- An artboard's view is page, scenario, artboard, iteration, size and lens.
  Two artboards that differ only by iteration are different views.
- **Copy reference**, handoffs, captures and agent context name the
  iteration when one is selected, by ID and label, and the iteration's source
  file among the code pointers. Without one, they say nothing about
  iterations.
- Annotations belong to the view they were drawn on, including its iteration.
  Switching iteration follows the same rule as switching scenario.
- The [design-system export](../review/export.md) captures every declared
  iteration as its own reference, beside its artboard's authored design, and
  "all declared variants" includes them. Each reference names its scenario,
  artboard and iteration.
- **Choosing an iteration** happens through a handoff: the handoff names the
  iteration and its source so a person or agent can make it the artboard's
  design. Workbench never rewrites definitions or sources to promote an
  iteration.

## Decisions

- **2026-10-24, names.** The alternative condition of a page is a *scenario*;
  a take on an artboard's design is an *iteration*. "Variant" was rejected
  because Figma uses it for property combinations, "option" because `options`
  is pervasive in code, "draft" and "version" because they imply unfinished
  work or releases.
- **2026-10-24, hierarchy.** Page › Scenario › Artboard › Iteration, with
  implicit scenario and artboard. Iterations were first considered per page
  and then per scenario; both were rejected because not every scenario, and
  not every artboard, has the same iterations.
- **2026-10-24, artboards move under scenarios.** Page-level `artboards` in
  `definePreview` was the wrong level and is removed without a fallback;
  Workbench has no users to migrate.
- **2026-10-24, promotion by handoff.** Rewriting sources is project-specific
  and risky; the handoff names the iteration instead.

## Acceptance criteria

1. A definition with scenario-level `artboards` and per-artboard `iterations`
   lists its scenarios in the sidebar and no iterations there.
2. A render with an iteration selected uses the iteration's `source` and
   merged inputs, fixtures, globals and requests in the order above; with none
   selected the artboard renders exactly as before.
3. `context.scenario`, `context.artboard` and `context.iteration` match the
   requested view; unknown artboard or iteration IDs render a named error.
4. Two scenarios of one page declare different artboards; switching scenario
   shows each scenario's own set.
5. Top-level `artboards` or `iterations`, scenario-level `iterations` beside
   `artboards`, duplicate or non-kebab-case IDs are problems naming their
   location; other previews still load.
6. An authored HTML page with a scenario iteration receives
   `data-wb-iteration`, and `-only`/`-not` elements follow it.
7. Implementation lenses ignore the iteration; returning to the design lens
   restores it.
8. Copy reference, handoff and agent context name a selected iteration and its
   source; annotations stay with the iteration they were drawn on.
9. A space export with all declared variants has one reference per iteration,
   in addition to each artboard's authored design.
10. Addresses with `+` parts round-trip through links, restoration and the
    portable viewer; unknown parts normalize to the authored design; a `src`
    containing `+` is a problem.
11. The Rust and Node definition readers agree on every rule above.

## Implementation gaps

Nothing in this topic is implemented. In particular:

- `definePreview` declares `artboards` at the page level
  ([api.d.ts](../../sdk/api.d.ts)), and definition validation, the page index,
  the canvas and capture read them there
  ([artboard system](../canvas/artboard-system.md)).
- The Acme demo's and website's Feedback pages declare page-level artboards and
  must move them under a scenario.
- No reader, address, context field, compatibility attribute, handoff or
  agent-context text knows iterations.

## Open questions

Deferred until the technical implementation is done:

1. **Canvas presentation.** How a person sees and picks an artboard's
   iterations, compares an iteration beside its artboard, and sees that an
   artboard has iterations.
2. **Docs lenses.** Whether a docs page's examples can follow the selected
   iteration of the page they document.
