# Workbench browser UI

The UI is authored and shipped as JavaScript ES modules, Web Components, HTML
and CSS. Serve these files unchanged from a static host. There is no UI build,
transpilation, bundling, generated JavaScript copy or runtime package installation.

`src/components/<name>/` owns an element, its styles and helpers. Importing an
element does not register it. `src/bootstrap/` composes registration and application
controllers; `src/modules/` owns capabilities; `src/theme/` supplies standalone
defaults. Browser-used rules under `../contracts/src/` are also JavaScript modules.
Their declaration files support backend consumers and are never loaded by browsers.

`src/vendor/highlight.js/` contains pinned upstream browser ES modules and their
license, served unchanged. Its manifest records asset hashes checked by the
focused highlighting test. The docs document owns a lazy module worker; its
explicit language imports require no UI build or runtime downloads. Vendored
files are excluded from source formatting and linting.

For example, from a static host exposing the product folder:

```html
<link rel="stylesheet" href="./ui/src/theme/defaults.css">
<wb-actions-switch></wb-actions-switch>
<script type="module">
  import { registerActionsComponents } from './ui/src/bootstrap/register-actions.js';
  registerActionsComponents();
  document.querySelector('wb-actions-switch').addEventListener('wb-actions-toggle', event => {
    console.log(event.detail);
  });
</script>
```

Small hosts import their registration family directly: `register-space.js`,
`register-sizes.js`, `register-actions.js`, or `register-canvas.js` under
`src/bootstrap/`. This keeps the sidebar and component examples from loading
unrelated elements. The full shell uses `register.js`, which exports all four
families. Registration is repeatable and rejects a conflicting element definition.

The complete canvas uses project metadata and operations through its API adapter.
VS Code supplies resource URLs, CSP and host capabilities; it loads the same
module files directly. Portable docs copy their UI modules unchanged.
`legacy/README.md` documents the shipping shell and content renderer.

`preview/` contains project-content adapters. A project's React/Vue/Astro code
still uses that project's preview compilation pipeline; it does not compile the
Workbench interface.

Run `pnpm --dir packages/workbench run check:ui` for JavaScript/Web Component
lint, formatting and unit/browser tests. Component browser fixtures use a static
file host without the Workbench backend or compiler. Backend TypeScript checks
remain a separate package command.
