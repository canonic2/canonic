# Pages and states

A Workbench screen is an HTML file in your repository. This guide covers how
those pages are served, how one page shows several states, how links behave,
and how to choose the frame sizes a screen supports.

## Design pages

Any HTML file in the project can be a screen: a hand-written page, a design
system's component preview, or a static export. List it in `workbench.yaml`:

```yaml
sections:
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
- **Two scripts are injected into every page the server serves.** `actions.js`
  goes first in `<head>`, and `states.js` follows it. They implement
  [states](#states) and [actions](#links-and-actions). They don't change
  anything a page's own scripts can see, apart from attributes on `<html>`.
- **A page opened straight from disk gets neither script.** It shows as
  authored, and its links behave like ordinary links.

Because the canvas and your page share an origin, the workbench can read the
live page. That is what lets it name the element under each mark and take a
screenshot of exactly what you see.

### What works in a page

Pages are real pages in a Chromium-based browser: scripts, web components,
shadow DOM, web fonts, canvas, video, dialogs, and forms all work. The
screenshot path copies the live document, so a few things don't appear in
screenshots even though they appear on the canvas:

- content inside nested iframes,
- closed shadow roots,
- canvas or video pixels loaded from another origin without CORS.

See [Screenshot limits](markup-and-handoff.md#what-screenshots-can-and-cant-include).

## States

A state is a variation of the same page: the empty form and the form that came
back with an error, a list with rows and the same list with none. The workbench
lists a screen's states under it, the way Storybook lists stories under a
component.

A state is declared twice, and the ids must match:

1. In `workbench.yaml`, which adds the row to the sidebar.
2. In the page, which decides what the state looks like.

```yaml
- label: Sign in
  src: pages/sign-in.html
  states:
    - id: default
      label: Default
    - id: error
      label: Wrong password
    - id: sending
      label: Signing in
```

When you pick a state, the page loads with `?state=<id>`. The first state is
the page as authored and loads with no parameter. `states.js` reads the
parameter and sets `data-wb-state` on `<html>`: `default` for the first state,
or the id. An id that isn't kebab-case is treated as `default`.

> The first state always appears in the page as `default`, whatever its id.
> Give it the id `default` so the YAML and the page agree. If it has another
> id, such as `primary`, refer to it as `default` in the page.

A page can answer in three ways. Use the smallest one that works.

### 1. CSS keyed off the root

```css
.alert { display: none; }
html[data-wb-state="error"] .alert { display: block; }
html[data-wb-state="error"] .field input { border-color: var(--red); }
```

This suits differences that are purely visual.

### 2. Markup that exists only in some states

```html
<p class="alert" data-wb-state-only="error">That password isn't right.</p>
<p class="hint" data-wb-state-not="error sending">Use your work email.</p>
```

- `data-wb-state-only="a b"` keeps the element in states `a` and `b`, and
  removes it in every other state.
- `data-wb-state-not="a b"` removes the element in states `a` and `b`.

Ids are space-separated. Elements are **removed**, not hidden, so they don't
take up space, affect layout, or appear in a screenshot. To keep an element
in the default state, include `default` in the list.

### 3. Attributes set in one state

```html
<input type="email" data-wb-set-error="value=dana@example.com; aria-invalid=true">
<button data-wb-set-sending="disabled; aria-busy=true">Sign in</button>
```

`data-wb-set-<id>` sets attributes on that element in state `<id>`. Separate
pairs with semicolons. Write `name=value`, or a bare `name` for a boolean
attribute. Only the active state's attribute is read; the others are left in
the markup as a record of what the element does elsewhere.

### When states are applied

`states.js` sets the root attribute immediately, then applies `only`, `not`,
and `set` at `DOMContentLoaded`. Custom elements loaded with `defer` are already
upgraded by then, so they receive the attributes normally. Elements that your
scripts add later aren't processed, so key late content off the root attribute
with CSS, or read `document.documentElement.dataset.wbState` in your script:

```js
if (document.documentElement.dataset.wbState === 'empty') renderEmptyState();
```

### Tips

- Keep ids short and descriptive. They appear in links and screenshot names:
  `sign-in-error.jpg`.
- A screen needs at least two states to show any rows. One state is the same
  as none.
- When a state needs data the page doesn't have, render it from a small inline
  fixture keyed by `dataset.wbState`, rather than making a second page.
- State ids also map to implementation paths, so the same state can be shown
  on your dev server. See [Map states to paths](lenses.md#3-map-states-to-paths).

## Links and actions

The toolbar's **Actions** switch decides whether a page's links and forms work.

- **Off** (the default): links don't navigate and forms don't submit. Hover,
  focus, pressed states, disclosure widgets, pickers, and anything else that
  only changes the page in place still work. You can click around a screen
  you're reviewing without leaving it.
- **On**: the page behaves normally, so you can walk through a real flow.

With actions on, a link to another page in the project that is listed in
`workbench.yaml` switches the workbench to that screen, and the sidebar follows.
Links elsewhere, `mailto:` links, downloads, and same-page anchors behave as
usual.

The switch applies to pages Workbench serves. Implementations shown through a
[lens](lenses.md) are other sites, and their links always work.

`actions.js` sets `data-wb-actions="on"` or `"off"` on `<html>` if your styles
need to know.

## Viewports

`viewports` lists the frame sizes a screen is designed for. Width buttons for
other sizes are disabled while it is showing, and
[exports](design-system-export.md) capture one reference image per viewport.

```yaml
- label: Reset password
  src: pages/reset.html
  viewports:
    - mobile
    - responsive
```

| Viewport | Frame | Use it for |
| --- | --- | --- |
| `desktop` | 1512 × 982, a 14-inch MacBook Pro | Desktop layouts |
| `mobile` | 393 × 852, an iPhone 15 Pro | Phone layouts |
| `responsive` | A frame you resize by dragging its edges or corners | Layouts that should work at any width. Exports both desktop and mobile references. |
| `fit` | Fills the canvas | Components and pages without a fixed device size. Exports at 1440 × 900. |

Omit `viewports` to allow all four. The page always lays out at the frame's
real size; when the frame is larger than the canvas, the canvas zooms out
rather than squeezing the page. See [Frame widths and zoom](canvas.md#frame-widths).

## Component previews

For a design system, a common pattern is one preview page per component, with
its variants as states:

```yaml
- name: Components
  icon: component
  items:
    - label: Button
      src: preview/button.html
      viewports:
        - fit
      states:
        - id: default
          label: Primary
        - id: secondary
          label: Secondary
        - id: disabled
          label: Disabled
```

```html
<!-- preview/button.html -->
<button class="btn"
        data-wb-set-secondary="class=btn btn-secondary"
        data-wb-set-disabled="disabled">Continue</button>
```

If your components already have Storybook stories, consider
[importing them](storybook.md#import-the-whole-catalog) instead of writing
previews.
