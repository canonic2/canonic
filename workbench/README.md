# Canonic Workbench

A VS Code extension that renders a project's screens on one canvas at device
widths: HTML pages, TypeScript component previews, Storybook stories, and
running apps. You can draw on a screen and copy the marks, with a screenshot,
as a prompt for a coding agent.

A project is configured by a `workbench.yaml` at its root. The extension runs
a local server for the canvas; nothing is added to your pages or source.

Documentation: <https://canonic.sh/workbench/docs/>

## Setup

1. Install the extension.
2. Add `workbench.yaml` to the project root:

   ```yaml
   name: Acme

   sections:
     - name: Pages
       items:
         - label: Sign in
           src: pages/sign-in.html
           states:
             - id: default
               label: Default
             - id: error
               label: Wrong password
   ```

3. Open the folder. **Workbench** appears in the activity bar.

A project that only uses TypeScript previews needs just `name: Acme`; the
previews supply the screen list. See
[Getting started](https://canonic.sh/workbench/docs/getting-started/).

## Screen sources

- **TypeScript previews**: `*.workbench.ts` and `*.workbench.tsx` files in the
  project, outside hidden folders, `node_modules`, and build output. Built-in adapters for HTML, React, Vue, Astro, and
  React Native Web, or your own. Each preview has named states, input
  controls, an action log, and docs. For types, install
  `@canonic2/workbench` as a dev dependency. See
  [TypeScript previews](https://canonic.sh/workbench/docs/workbench-previews/).
- **HTML pages** listed in `workbench.yaml`. A page's states are selected
  through `html[data-wb-state]`. See
  [Pages and states](https://canonic.sh/workbench/docs/pages-and-states/).
- **Implementations**, shown as lenses next to a screen's design: Storybook
  stories, URLs on a dev server or staging, the iOS Simulator, or a macOS app
  window. See [Lenses](https://canonic.sh/workbench/docs/lenses/).

## Features

- Viewports: fit, desktop, mobile, or a custom size, with zoom, pan, and a
  recenter button to bring the frame back into view.
- Markup: drawing, arrows, shapes, text, and comments. The camera button saves
  a JPEG.
- Handoff: saves the screenshot to `.canonic/.handoffs/` (ignored), clears the
  marks, and copies a text description of each mark to the clipboard.
- Actions are off by default: clicking a link or submitting a form on a
  screen is logged instead of followed.
- Several projects in one window, such as a product and its design system.
- Design-system export: a ZIP with each screen's source files, reference
  screenshots, and a standalone viewer for the TypeScript previews. See
  [Design-system export](https://canonic.sh/workbench/docs/design-system-export/).
- Errors from the canvas, capture, server, and handoffs go to the
  **Workbench** output log, not into the project.

## Commands

| Command | Does |
| --- | --- |
| `Workbench: Open Canvas` | Opens the canvas in an editor tab |
| `Workbench: Open Canvas in Browser` | Opens the canvas in your browser |
| `Workbench: Copy Canvas URL` | Copies the canvas address |
| `Workbench: Refresh Screens` | Re-reads the config and finds TypeScript previews again |
| `Workbench: Switch Project…` | Switches to another project |
| `Workbench: Add Project…` | Adds a project from elsewhere on disk |
| `Workbench: Show Log` | Shows the Workbench log |

## Requirements

- VS Code 1.75 or later.
- Screenshots and handoffs use a bundled Electron helper, which runs on macOS
  13 or later, Windows, and Linux with a display.
- The frameworks your previews use (React, Vue, Astro, React Native Web) come
  from your project.

See [The VS Code extension](https://canonic.sh/workbench/docs/extension/) for
the server, its ports, the screenshot helper, and workspace trust.

## License

MIT. `workbench/modern-screenshot.js` is vendored from
[modern-screenshot](https://github.com/qq15725/modern-screenshot) (MIT, ©
qq15725).

The bundled Electron runtime includes its MIT license and Chromium's third-party
notices in `electron-runtime/runtime.tar.gz`, preserved in the extracted cache.

Source and contributor notes: <https://github.com/canonic2/canonic>.
