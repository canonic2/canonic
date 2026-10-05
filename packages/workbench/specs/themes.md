# Workbench themes

## Purpose and status

Workbench's interface has a complete default theme and can inherit its host's
theme. In VS Code it follows the active theme, including user color overrides.
In a standalone browser it remains usable without any VS Code variables or API.
The same [Web Components](web-components.md) serve both environments.

**Status (2026-10-04): shared defaults and first consumer implemented.**
`src/theme/defaults.css` supplies the space switcher's and size controls' tokens,
including the selected segment, error text and primary button roles, and direct
webview host mappings. The sidebar maps host roles for this control; the canvas continues
to use its existing palette. The canvas theme bridge and full-shell migration
remain implementation gaps.

## Tokens and default behavior

Components consume semantic `--wb-*` CSS custom properties. A shared shell theme
layer owns the defaults and host mappings; components do not each maintain a
palette or directly depend on `--vscode-*` variables. CSS custom properties
inherit through component hosts into Shadow DOM.

Keep theme assets under `src/theme/`, separate from components and host transport.
Define defaults for every token used by migrated components. Start from the
existing Workbench palette for standalone use; this task does not introduce a
theme picker, OS preference behavior, or persisted theme settings.

The token contract covers surfaces (panel, canvas, floating controls, inputs and
menus), foregrounds, borders, hover and selection, action buttons, focus, disabled
and error states, and UI typography. Tokens describe roles rather than a particular
color. Preserve existing `--wb-*` names where their meaning fits; add roles as
components require them rather than inventing unused tokens.

For example, the shared theme layer can resolve host variables with defaults:

```css
:root {
  --wb-panel: var(--vscode-editor-background, #1e1e1e);
  --wb-fg: var(--vscode-foreground, #ffffff);
  --wb-focus: var(--vscode-focusBorder, #00a1ff);
}
```

This illustrates a canvas host mapping, not an exhaustive palette. A sidebar maps
its surface to `sideBar.background` and `sideBar.foreground`. Other roles use the
corresponding input, menu, button, list, and border tokens. Map by meaning and
provide a usable fallback when an optional host value is absent. Validate supported
host values at the adapter boundary; CSS `var()` fallbacks alone do not repair a
present but invalid value.

The resolution order is an explicit Workbench token override by the embedding
host, then the mapped host theme value, then the Workbench default. Components
must not redefine shared tokens on `:host` in ways that mask inherited overrides.
Another host may supply `--wb-*` values without emulating VS Code.

## VS Code integration

VS Code injects theme CSS variables and theme metadata into the webview document.
Use resolved values from that document rather than reading theme JSON or guessing
a palette from the theme name. This includes custom themes and user overrides.
Select UI fonts from host UI variables, not editor code font settings.

The sidebar is rendered directly in a webview and can use those values there.
The canvas is a separate, cross-origin iframe inside a webview:

```text
VS Code webview (theme variables)
  → Workbench iframe (shell tokens)
    → preview iframe (project styling)
```

CSS values do not inherit across iframe documents. The embedding webview owns a
theme adapter that reads the relevant resolved variables and sends a complete,
versioned theme snapshot to Workbench through the host message bridge. The
snapshot includes theme kind and the allowlisted color/font values needed by the
shell. Exact message names and schema are established with the implementation.

- Send the latest snapshot after the Workbench receiver is ready, including after
  iframe reloads and space switches; do not lose the initial theme to load timing.
- Observe webview theme metadata/variable changes, including changes to overrides
  while the theme kind stays the same. Coalesce updates and send the latest state.
  An extension-side theme-kind notification alone is insufficient.
- Check message source and the expected host origin using the embedding contract.
  Accept only the supported schema and allowlisted theme values. Do not accept
  arbitrary stylesheets, selectors, or CSS property names from messages.
- Apply each snapshot as a replacement. Clear values absent from the new snapshot
  so previous theme colors cannot leak into the next theme. Invalid snapshots do
  not discard the last valid theme; missing individual values use defaults.
- Keep theme state scoped to the receiving document. Do not store host themes in
  `workbench.yaml`, shared server state, or project files. Multiple canvases must
  not affect each other's appearance.
- Release observers and message subscriptions with their owning host/document.

Theme changes update styles in place. They do not reload the canvas, reconnect
previews, reset application state, close menus, or move focus. Before the initial
snapshot, Workbench renders its defaults rather than waiting indefinitely.

## Accessibility and preview isolation

Support light, dark, high contrast dark, and high contrast light host themes.
Carry theme kind into the shell for behavior that cannot be expressed with color
tokens alone, and set `color-scheme` consistently for native controls. Use semantic
focus, selection, and contrast border values; derived transparency shades alone
must not erase focus or control boundaries. Honor browser forced-colors behavior.

The theme applies to Workbench chrome and controls. Do not inject it into authored
pages, implementation iframes, native streams, or exported project examples.
Workbench-owned controls around those previews follow the shell theme. Preserve
artboard backing and project capture/export appearance independently of shell
colors. Branding marks and authored drawing colors are not automatically theme
accent colors; interactive control states use the host's semantic roles.

## Acceptance criteria

- Standalone Workbench renders every migrated control using defaults with all
  `--vscode-*` variables absent. Another host can override shell tokens.
- Embedded canvas and sidebar follow a custom VS Code theme and user color
  overrides. Check light/dark and both high contrast variants in real VS Code.
- Changing theme or a color override while a menu/input is active updates colors
  without losing focus, text, selection, canvas state, or preview state.
- Reloading or switching spaces receives the latest snapshot. A missing optional
  value falls back; a formerly supplied value does not survive its omission.
- Unrelated windows/messages cannot change the theme. Pure mapping and snapshot
  replacement tests cover missing/invalid values and host boundary validation.
- Real browser checks verify inheritance into nested Shadow DOM, native controls,
  visible focus/selection and forced colors. Packaged assets work under webview CSP.
- Preview styling and project screenshots remain independent of shell theme.

## Evidence and implementation gaps

Verified from repository sources on 2026-10-04:

- [sidebar.css](../workbench/sidebar.css) already maps panel/text variables but
  uses fixed blue accents and derived shades.
- [workbench.css](../workbench/workbench.css) defines the canvas shell's fixed
  palette and forces `color-scheme: dark`.
- [panel.js](../panel.js) embeds the server canvas as a separate-origin iframe;
  its current message bridge does not forward theme values.

Platform references:

- [VS Code webview theming](https://code.visualstudio.com/api/extension-guides/webview#theming-webview-content)
  defines injected variables, theme categories, and font variables.
- [VS Code theme colors](https://code.visualstudio.com/api/references/theme-color)
  defines semantic color roles and customization.
- [MDN CSS custom properties](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_cascading_variables/Using_CSS_custom_properties)
  and [Shadow DOM](https://developer.mozilla.org/en-US/docs/Web/API/Web_components/Using_shadow_DOM)
  describe inherited values and style encapsulation.
