# Lenses

A lens is one way of rendering a page. Projects choose the views they need
to compare, such as an authored design, a component preview, a Storybook
story, a local app, or the live website. Each lens has a customizable display
label independent of how it renders.

This is the shared lens contract. Rendering contracts live in
[implementations](../pages/implementations.md), [TypeScript previews](../pages/previews.md),
[Storybook](../pages/storybook.md), and [docs pages](../pages/docs-pages.md). Selection and
addresses follow [core.md](../core.md#selection-and-navigation).

## Requirements

- Every lens supports a project-configured display label. This includes the
  page's own authored lens, whether its source is HTML or a `.workbench.ts(x)`
  preview, and every implementation kind: `url`, `workbench`, `storybook`,
  `ios-simulator`, `window`, and `docs`.
- Kind determines rendering behavior; it does not restrict the label. A URL
  lens pointing at the live website can be called **Live**, **Prod**, or another
  project-chosen name. Storybook and Workbench preview lenses have the same
  freedom, including calling a Workbench preview **Design**.
- Labels are optional. Without an override, the authored lens defaults to
  **Design** for HTML and **Preview** for a TypeScript preview.
  Implementations use their configured `label`, falling back to their key.
- Renaming the authored lens changes that existing lens's label. It does not
  require creating an implementation or add a second button for the same view.
- A label is display text, separate from lens identity. Changing it preserves
  the selected view, mappings, saved lens preference, and copied addresses.
  A page's default lens is omitted from the address: its first docs lens, or
  the authored lens of a page without docs. The authored lens of a page with
  docs is addressed `~design`, so `design` is a reserved implementation name.
- Lens names shown by the interface and in review outputs use the resolved
  display label consistently. Rendering, startup, actions, states, and capture
  behavior continue to depend on kind and configuration.
- An authored page has its own lens plus the implementations it maps to,
  its docs lenses among them. Catalog pages and Markdown pages have no
  authored design lens. The switcher appears only when the page has at least
  two lenses; customizing a label does not change that count.
- Docs are a lens: a page with Markdown is read through one docs lens per
  renderer of its examples, or the built-in **Docs** lens when it maps none.
  The switcher lists docs lenses first, then the authored lens, then the
  other implementations, and a page with docs opens on its first docs lens.
  The canvas mode follows the lens; see [docs](../pages/docs-pages.md).
- The chosen lens carries across pages. A page without it shows its default
  lens: its design, its catalog implementation, or a Markdown page's own docs
  lens. [lenses.js](../../contracts/src/docs/lenses.js) decides it.

## Acceptance criteria

- A project can name two URL implementations **Dev** and **Live**, mapping
  them to a local app and the production website, and see those labels in the
  switcher.
- A Storybook implementation and a `workbench` implementation each honor a
  custom label without changing which story or preview they load.
- A page whose own source is `.workbench.ts(x)` can label that lens **Design**
  through project configuration. With a **Live** mapping, the switcher shows
  exactly **Design** and **Live**, with no extra **Workbench** button.
- Renaming a lens leaves existing links and the current selection pointing
  at the same view. Omitting labels retains the defaults above.

## Configuration and verification

Implementations use `label`. Authored pages use `lensLabel` in their YAML page
entry. Discovery supplies `previews.lensLabel` for TypeScript previews; an
explicit page's `lensLabel` takes precedence when that preview is placed in
YAML. Both keys accept nonempty strings and trim surrounding whitespace.
Invalid values produce named problems and leave the fallback available.
Markdown pages reject page `lensLabel`, since they have no authored lens.

The shared [manifest reader](../../contracts/manifest.js) validates labels and
resolves authored defaults. The server supplies the discovery label, and
collection merging retains page overrides without duplicating the preview.
The switcher, source menu, copied references, and authored handoffs use the
resolved label. Addresses, saved preferences, and screenshot filenames retain
their stable identities rather than incorporating renamed display labels.

Behavioral coverage is in [manifest tests](../../contracts/manifest.test.js),
[configuration tests](../../server/node/config.test.js), [preview integration tests](../../server/node/preview.test.js),
[switcher tests](../../ui/legacy/preview.test.js),
[reference tests](../../ui/legacy/reference.test.js), and
[handoff module tests](../../server/src/handoff.rs).
