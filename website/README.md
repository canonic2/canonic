# Canonic website

A single static page that documents the extension's UI: the workbench layout,
each toolbar button, page states, markup and handoff, lenses, and how to
install a platform build. Plain HTML and CSS with no build step or runtime
dependencies.

| File | Job |
| --- | --- |
| `index.html` | the page |
| `style.css` | light and dark themes, taken from the workbench chrome's grays and Canonic blue |
| `canonic.svg` | the mark, filled blue so it reads on both themes |
| `images/` | screenshots of the workbench showing the Acme example |
| `screenshots/` | the Acme fixture and the script that regenerates `images/` |

## Previewing

Open `index.html` in a browser, or serve the folder with any static server.

## Updating screenshots

When the workbench UI changes, regenerate the images from the repository root:

```sh
node packages/website/screenshots/capture.cjs
```

The script starts `packages/workbench/server.js` on `screenshots/fixture`, drives
headless Chrome through `packages/workbench/capture.js`, and overwrites the screenshots
in `images/`. Set `CHROME_PATH` to choose a browser.

- The overview is a 1440 × 860 window. The script also measures the workbench's
  rail, screen list, toolbar, and canvas there, and rewrites the numbered
  outlines between the `regions:start` and `regions:end` markers in
  `index.html`. Don't edit that block by hand.
- The toolbar section's strip is three crops (`images/toolbar-*.png`) from the
  same window rendered at 4×, one per group: Actions, markup tools, and frame
  and screen controls. The script writes them side by side between the
  `toolbar-strip` markers, with an invisible hotspot button over every
  control. A small script at the end of `index.html` captions the hotspot you
  point at, tap, or tab to, using the matching entry in the "All controls"
  list, so each description is written once.
- Before writing the strip, the script checks that each "All controls" list's
  `data-label` attributes match the toolbar's control labels in order, and
  stops with both lists if they don't. When a control is added, removed, or
  renamed, update the list, then rerun.
- The States and Markup shots are 1120 × 860, the narrowest window that still
  shows the markup tools; narrower, the toolbar folds them into its More menu.

Keep the fixture on Acme and example.com placeholders.
