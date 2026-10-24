# Pages and scenarios

A design page is an HTML file in your repository. This guide covers how
those pages are served, how one page shows several scenarios, how links behave,
and how to choose the artboard sizes a page supports. To render components
from your codebase instead, see [TypeScript previews](workbench-previews.md).
To document components in Markdown with live examples, see
[Docs](docs-pages.md).

## Design pages

Any HTML file in the project can be a page: a hand-written page, a design
system's component preview, or a static export. List it in `workbench.yaml`:

```yaml
collections:
  - name: Pages
    items:
      - label: Dashboard
        src: design/dashboard.html
```

Pages need nothing added for Workbench. No script tag, import, or build step.

### How pages are served

The extension runs a small server on `127.0.0.1` that serves your project at
`/` and the workbench at `/_workbench/`, on one origin. That has a few
consequences for how you write pages:

- **Paths behave as on a normal web server rooted at the project.** A relative
  path (`../styles/app.css`) resolves from the page. A root-relative path
  (`/styles/app.css`) resolves from the project root.
- **One script is added to every HTML page the server serves, and to
  TypeScript previews.** `/_workbench/preview-compat.js` goes first in `<head>`
  and handles [scenarios](#scenarios), [actions](#links-and-actions), and forwarding
  editor shortcuts to VS Code. You don't add it yourself.
- **A page opened straight from disk gets no script.** It shows as
  authored, and its links behave like ordinary links.

Because the canvas and your page share an origin, the workbench can read the
live page. That is what lets it name the element under each annotation and take
a screenshot of exactly what you see.

### What works in a page

Pages are real pages in a Chromium-based browser: scripts, web components,
shadow DOM, web fonts, canvas, video, dialogs, and forms all work. The
screenshot path copies the live document, so closed shadow roots don't appear
in screenshots, and a page containing an `iframe`, `object`, or `embed`, or a
canvas or video loaded from another origin without CORS, can't be captured.
See [Screenshot limits](annotations-and-handoff.md#what-screenshots-can-and-cant-include).

## Scenarios

A scenario is a variation of the same page: the empty form and the form that came
back with an error, a list with rows and the same list with none. The page
list shows a page's scenarios under it, the way Storybook lists stories under a
component.

A scenario is declared twice, and the ids must match:

1. In `workbench.yaml`, which adds the scenario to the page list.
2. In the page, which decides what the scenario looks like.

```yaml
- label: Sign in
  src: pages/sign-in.html
  scenarios:
    - id: default
      label: Default
    - id: error
      label: Wrong password
    - id: sending
      label: Signing in
```

When you pick a scenario, the page loads with `?scenario=<id>`. The first scenario is
the page as authored and loads with no parameter. Workbench's script reads the
parameter and sets `data-wb-scenario` on `<html>`: `default` for the first scenario,
or the id. An id that isn't kebab-case is treated as `default`.

> The first scenario always appears in the page as `default`, whatever its id.
> Give it the id `default` so the YAML and the page agree. If it has another
> id, such as `primary`, refer to it as `default` in the page.

A page can answer in three ways. Use the smallest one that works.

### 1. CSS keyed off the root

```css
.alert { display: none; }
html[data-wb-scenario="error"] .alert { display: block; }
html[data-wb-scenario="error"] .field input { border-color: var(--red); }
```

This suits differences that are purely visual.

### 2. Markup that exists only in some scenarios

```html
<p class="alert" data-wb-scenario-only="error">That password isn't right.</p>
<p class="hint" data-wb-scenario-not="error sending">Use your work email.</p>
```

- `data-wb-scenario-only="a b"` keeps the element in scenarios `a` and `b`, and
  removes it in every other scenario.
- `data-wb-scenario-not="a b"` removes the element in scenarios `a` and `b`.

Ids are space-separated. Elements are **removed**, not hidden, so they don't
take up space, affect layout, or appear in a screenshot. To keep an element
in the default scenario, include `default` in the list.

### 3. Attributes set in one scenario

```html
<input type="email" data-wb-set-error="value=dana@example.com; aria-invalid=true">
<button data-wb-set-sending="disabled; aria-busy=true">Sign in</button>
```

`data-wb-set-<id>` sets attributes on that element in scenario `<id>`. Separate
pairs with semicolons. Write `name=value`, or a bare `name` for a boolean
attribute. Only the active scenario's attribute is read; the others are left in
the markup as a record of what the element does elsewhere.

### When scenarios are applied

The script sets the root attribute immediately, then applies `only`, `not`,
and `set` at `DOMContentLoaded`. Custom elements loaded with `defer` are already
upgraded by then, so they receive the attributes normally. Elements that your
scripts add later aren't processed, so key late content off the root attribute
with CSS, or read `document.documentElement.dataset.wbScenario` in your script:

```js
if (document.documentElement.dataset.wbScenario === 'empty') renderEmptyList();
```

### Tips

- Keep ids short and descriptive. They appear in links and screenshot names:
  `sign-in-error.jpg`.
- A page needs at least two scenarios to show any in the page list. One scenario
  is the same as none.
- When a scenario needs data the page doesn't have, render it from a small inline
  fixture keyed by `dataset.wbScenario`, rather than making a second page.
- Scenario ids also map to implementation paths, so the same scenario can be shown
  on your dev server. See [Map scenarios to paths](lenses.md#3-map-scenarios-to-paths).
- In a [docs lens](docs-pages.md#scenarios), the page's scenario applies to every
  example in its docs. A Markdown page's scenarios aren't listed in the page list.

## Links and actions

The top bar's **Actions** switch decides whether a page's links and forms work.

- **Off** (the default): links don't navigate and forms don't submit. Hover,
  focus, pressed states, disclosure widgets, pickers, and anything else that
  only changes the page in place still work. You can click around a page
  you're reviewing without leaving it.
- **On**: the page behaves normally, so you can walk through a real flow.

With actions on, a link to another page listed in `workbench.yaml` switches
the workbench to that page, and the sidebar follows. A `?scenario=<id>` on the
link picks that scenario, as in `<a href="sign-in.html?scenario=error">`. A form
whose `action` names a listed page does the same when it is submitted, so a
flow can move from one page to the next.

A link or form to an `.html` file in the project that isn't listed as a page
does nothing. List the page in `workbench.yaml` to make it reachable. Links to other
sites, to project paths that don't end in `.html`, `mailto:` links, and
downloads behave as usual.

A link to a spot on the same page, such as `href="#pricing"`, scrolls to it
whether actions are on or off.

[Workbench previews](preview-data.md#links-and-navigation) are mocks
rather than pages: with actions on, their links open the previews their
definition maps them to, and any other link or form is recorded under
**Actions** instead of followed.

Either way, browser form validation is off, so a `required` field doesn't stop
a flow.

The switch applies to pages Workbench serves. Implementations shown through a
[lens](lenses.md) are other sites, and their links always work.

Workbench sets `data-wb-actions="on"` or `"off"` on `<html>` if your styles
need to know.

## Sizes

`sizes` lists the artboard sizes a page is designed for. The size
switcher's other sizes are disabled while it is showing, and
[exports](design-system-export.md) capture one reference image per size.

```yaml
- label: Reset password
  src: pages/reset.html
  sizes:
    - mobile
    - resizable
```

| Size | Artboard | Use it for |
| --- | --- | --- |
| `laptop` | 1512 × 982, a 14-inch MacBook Pro | Laptop and desktop layouts |
| `mobile` | 393 × 852, an iPhone 15 Pro | Phone layouts |
| `resizable` | An artboard you resize by dragging its edges or corners | Layouts that should work at any width. Exports both laptop and mobile references. |
| `fit` | Fills the canvas | Components and pages without a fixed device size. Exports at 1440 × 900. |

Omit `sizes` to allow all four. The page always lays out at the artboard's
real size; when the artboard is larger than the canvas, the canvas zooms out
rather than squeezing the page. See [Artboard sizes and zoom](canvas.md#artboard-sizes).
In a [docs lens](docs-pages.md#on-the-canvas), the docs fill the canvas and
the size switcher is disabled; the design lens keeps the page's size. A
Markdown page has no artboard and takes no `sizes`.

## Component previews

For a design system, a common pattern is one preview page per component, with
its variants as scenarios:

```yaml
- name: Components
  icon: component
  items:
    - label: Button
      src: components/button.html
      sizes:
        - fit
      scenarios:
        - id: default
          label: Primary
        - id: secondary
          label: Secondary
        - id: disabled
          label: Disabled
```

```html
<!-- components/button.html -->
<button class="btn"
        data-wb-set-secondary="class=btn btn-secondary"
        data-wb-set-disabled="disabled">Continue</button>
```

To render the real components from your codebase instead of HTML copies, write
[TypeScript previews](workbench-previews.md). To document each component on
one page, with its examples, their code, and props tables, give the page
[docs](docs-pages.md). If your components already have Storybook
stories, [map your pages to them](storybook.md#map-design-pages-to-stories).
