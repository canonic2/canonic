# Custom adapters and compiler settings

Workbench previews ship with adapters for HTML, React, Vue, Astro, and React
Native Web. To preview anything else, such as web components, Svelte, Solid, or
an in-house rendering library, register an adapter in `workbench.config.ts`. The
same file holds the compiler settings every preview shares: compiler plugins and
loader rules, import aliases, deduplicated packages, file extensions, and
build-time constants.

Use this guide when:

- your components aren't rendered by a built-in adapter,
- a file type needs a compiler step Workbench doesn't have, such as `.svelte`
  files or `?inline` imports,
- imports need redirecting to mocks or to one copy of a package,
- you want to replace how a built-in adapter mounts components.

For the definition keys, controls, hooks, and the command-line checker, see
[TypeScript Workbench previews](workbench-previews.md).

## Requirements

- A `workbench.yaml` at the project root, and a
  [trusted workspace](extension.md#workspace-trust). Plugins, loaders and
  runtimes run project code.
- Whatever your adapter renders with, installed in your project. Workbench
  bundles your runtime module with the project's own packages and installs
  nothing.
- Plugins and loaders for [Rspack](https://rspack.rs/), which compiles
  previews. Most webpack loaders work as Rspack rules. Vite, Rollup, and Babel
  plugins need an equivalent loader or plugin.

## Example: web components

This example previews a custom element whose shadow DOM styles are imported with
a Vite-style `?inline` suffix. It registers a `web-components` adapter, with a
rule that loads `?inline` imports as text. It needs no packages.

```text
acme-ui/
├── workbench.yaml
├── workbench.config.ts
├── preview/
│   └── web-components.ts
└── src/
    ├── acme-button.css
    ├── acme-button.ts
    ├── acme-button.workbench.ts
    ├── acme-theme.ts
    └── inline.d.ts
```

`workbench.config.ts` registers the adapter and its rule:

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  adapters: {
    'web-components': {
      runtime: './preview/web-components.ts',
      // Imports ending in `?inline` load the file's text, for shadow DOM styles.
      rules: [{ resourceQuery: /inline/, type: 'asset/source' }],
    },
  },
});
```

`preview/web-components.ts` is the runtime. It creates the element, sets the
inputs as properties, and logs the element's events in **Actions**:

```ts
import type { PreviewContext } from '@canonic2/workbench';

type ElementClass = CustomElementConstructor & { events?: string[] };

export async function mount(
  canvas: HTMLElement,
  source: ElementClass,
  context: PreviewContext,
  environment: { wrap?: (element: HTMLElement, context: PreviewContext) => HTMLElement },
) {
  const tag = customElements.getName(source);
  if (!tag) throw new Error('Define the element with customElements.define before exporting it.');

  const element = document.createElement(tag);
  Object.assign(element, context.inputs);
  for (const name of source.events ?? []) {
    element.addEventListener(name, event => context.action(name, (event as CustomEvent).detail), {
      signal: context.signal,
    });
  }
  canvas.append(environment.wrap ? environment.wrap(element, context) : element);

  // Lit and similar libraries render asynchronously.
  await (element as { updateComplete?: Promise<unknown> }).updateComplete;
  return () => canvas.replaceChildren();
}
```

`src/acme-button.ts` is the component:

```ts
import styles from './acme-button.css?inline';

export class AcmeButton extends HTMLElement {
  static events = ['acme-press'];

  label = 'Continue';
  disabled = false;

  connectedCallback() {
    const root = this.shadowRoot ?? this.attachShadow({ mode: 'open' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(styles);
    root.adoptedStyleSheets = [sheet];

    const button = document.createElement('button');
    button.textContent = this.label;
    button.disabled = this.disabled;
    button.addEventListener('click', () => {
      this.dispatchEvent(new CustomEvent('acme-press', { detail: this.label }));
    });
    root.replaceChildren(button);
  }
}

customElements.define('acme-button', AcmeButton);
```

`src/acme-button.css`:

```css
button {
  background: #4f46e5;
  border: 0;
  border-radius: 8px;
  color: white;
  font: 600 14px/1 system-ui, sans-serif;
  padding: 12px 16px;
}

button:disabled {
  opacity: 0.5;
}
```

`src/inline.d.ts` types the `?inline` imports for your editor:

```ts
declare module '*?inline' {
  const text: string;
  export default text;
}
```

`src/acme-theme.ts` is the preview's environment. The runtime above calls its
`wrap`:

```ts
import type { PreviewContext } from '@canonic2/workbench';

export function wrap(element: HTMLElement, context: PreviewContext) {
  const theme = document.createElement('div');
  theme.dataset.theme = String(context.globals.theme ?? 'light');
  theme.append(element);
  return theme;
}
```

`src/acme-button.workbench.ts` uses the adapter by name:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/acme-button',
  title: 'Components/Button',
  adapter: 'web-components',
  source: { entry: './acme-button.ts', export: 'AcmeButton' },
  environment: './acme-theme.ts',
  globals: { theme: 'light' },
  inputs: { label: 'Continue', disabled: false },
  controls: {
    label: { type: 'text' },
    disabled: { type: 'boolean' },
  },
  scenarios: {
    default: {},
    disabled: { inputs: { disabled: true } },
  },
});
```

Run **Workbench: Refresh Pages**. **Button** appears under **Components**
with **Default** and **Disabled** scenarios. Editing **label** in
**Preview controls** mounts the element again with the new value, and pressing
the button logs `acme-press` in **Actions**. The
[checker](workbench-previews.md#command-line-tools) reports:

```text
$ <extension>/server/bin/<platform>-<arch>/workbench-server check acme-ui
Built components/acme-button
```

## The runtime module

`runtime` is a browser module, relative to the project root, that exports
`mount(canvas, source, context, environment)`. Workbench bundles it with each
preview that uses the adapter, so it can import your framework like any other
module.

| Argument | Value |
| --- | --- |
| `canvas` | The empty element to render into. |
| `source` | The export of the scenario's `source.entry` named by `source.export` (`default` when omitted), after your rules and plugins compile it. |
| `context` | The [preview context](workbench-previews.md#lifecycle-hooks): `id`, `scenario`, `inputs`, `fixtures`, `globals`, `signal`, `action`, `navigate`, and `error`. |
| `environment` | Everything the definition's `environment` module exports, or `{}` when it has none. Define your own hooks here, such as `wrap` above. With a [project-wide environment](preview-data.md#environments) too, the two are combined: `setup`, `mount`, `ready`, `configure`, and `wrap` run both, the project's outside, and other exports come from the definition's module first. |

- `mount` may return a cleanup function, or a promise for one. Workbench waits
  for the promise before it runs `play` and `ready` hooks and marks the preview
  ready, so resolve it once the component has rendered.
- Workbench calls `mount` again for every scenario change, input edit, and
  **Reset scenario**, after aborting `context.signal` and running the previous
  cleanup. Remove anything you added outside `canvas` in the cleanup.
- An error thrown by `mount`, or a rejected promise, shows in the frame. Pass
  errors that happen later, such as in an event handler, to
  `context.error(error)`.
- How inputs reach the component is up to the runtime: props, properties,
  attributes, or a store.
- The runtime needs nothing for [request mocks](preview-data.md#request-mocks)
  or [links](preview-data.md#links-and-navigation): Workbench answers the
  page's requests and handles its links whatever renders it.

Previews with custom adapters export to the
[portable viewer](workbench-previews.md#portable-exports) like any other, with
their controls.

## Docs page examples

A registered adapter also renders the examples in a page's
[docs](docs-pages.md). Name it as the `adapter` of a `docs` lens:

```yaml
implementations:
  web:
    kind: docs
    label: Web components
    adapter: web-components
```

Workbench bundles the lens's example source with the runtime and the adapter's
`rules` and `plugins`, and calls `mount(canvas, source, context, environment)` once per
example:

- `canvas` is the example's panel, and `source` is the example: a file's
  default export, for a folder source, or one named export, for a file source.
- `context.id` is the example's ID. `inputs`, `fixtures`, and `globals` are
  empty, since examples have no inputs or controls.
- `environment` combines the project's environment for the adapter and the
  lens's `environment`, as for a preview. Its `setup` runs once before the
  examples mount, and `ready` once after.
- In a folder source, only files ending in `.ts`, `.tsx`, `.js`, `.jsx`, or
  `.mjs` are examples. For components in other file types, such as `.svelte`,
  write one script file per example that exports the component, or export
  them by name from one file.

A built-in adapter replaced in `adapters` is replaced for docs examples too.

## Configuration file

Workbench reads `workbench.config.ts` at the project root. To use another file
inside the project, set [`previews.config`](configuration.md#previews) in
`workbench.yaml`. Saving the file, or a file it imports, restarts the preview
compilers and reloads the open previews.

Workbench runs the file in Node. Relative imports are bundled with it; packages
are loaded from your project's `node_modules`. `defineConfig` from
`@canonic2/workbench` only adds types.

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  adapters: { /* name: { runtime, rules, plugins } */ },
  environment: './src/workbench/environment.tsx',
  rules: [],
  plugins: [],
  aliases: {},
  dedupe: [],
  resolveExtensions: [],
  define: {},
});
```

| Key | Type | Description |
| --- | --- | --- |
| `adapters` | map of name to `{ runtime, rules?, plugins? }` | Adapters that definitions name in `adapter`. |
| `environment` | path, or map of adapter name to path | An [environment](preview-data.md#environments) around every preview, or around each adapter's previews, relative to the project root. |
| `rules` | list of Rspack module rules | Loader rules for every preview. |
| `plugins` | list of compiler plugins | [Plugins](#rules-and-plugins) for every preview. |
| `aliases` | map of import specifier to path or package | Redirects exact imports. |
| `dedupe` | list of package names | Packages resolved from the definition's folder. |
| `resolveExtensions` | list of extensions | The extensions tried for imports without one. Replaces the default list. |
| `define` | map of expression to code string | Constants replaced at build time. |

### adapters

Each key is the name a definition gives as `adapter`. Definitions only accept
kebab-case names, such as `web-components`. Each value has:

- `runtime`: the [runtime module](#the-runtime-module), relative to the project
  root.
- `rules` and `plugins`: rules and plugins for previews that use this
  adapter only. `defineAdapter` from `@canonic2/workbench` types one adapter on its own.

### rules and plugins

Workbench compiles previews with [Rspack](https://rspack.rs/)'s compiler.
`rules` are [module rules](https://rspack.rs/config/module#modulerules): they
pick a loader or a module type for matching files. `plugins` are the compiler
capabilities below, made with helpers from `@canonic2/workbench`. Global ones
apply to every preview, and an adapter's to the previews that use it.
Workbench's own rules and plugins come first.

| Helper | What it does |
| --- | --- |
| `virtualModule({ id, source })` | Serves a module made from `source` when code imports `id`, such as `virtual:build-info` |
| `resolveImport({ request, to, importer? })` | Sends an exact import to another file: from every file, or only from files `importer` matches |
| `importGlob({ include? })` | Expands `import.meta.glob(...)` at build time, as Vite does |
| `svgSprite({ dirs, symbolId?, register?, names? })` | Builds an SVG sprite from a folder of icons, as `vite-plugin-svg-icons` does |

```ts
import { defineConfig, virtualModule, resolveImport, importGlob, svgSprite } from '@canonic2/workbench';

export default defineConfig({
  plugins: [
    virtualModule({ id: 'virtual:build-info', source: "export const buildInfo = 'Acme build 42';" }),
    // The first match wins: native screens get the native entry, other files the web one.
    resolveImport({ request: '@acme/ui', importer: 'apps/mobile/**', to: './packages/ui/src/native.ts' }),
    resolveImport({ request: '@acme/ui', to: './packages/ui/src/index.ts' }),
    importGlob(),
    svgSprite({ dirs: ['packages/ui/src/icons'], symbolId: 'icon-[name]' }),
  ],
});
```

- **`resolveImport`**: `request` matches one whole import. `to` is a file
  relative to the project root, or a package. `importer` is a glob, or a list
  of globs, over the importing file's path from the project root, or a regular
  expression over its absolute path.
- **`importGlob`**: each call's patterns must be string literals starting with
  `./`, `../` or `/` (the project root). Keys are written as the pattern is,
  in name order. `eager`, `import`, `query` and `exhaustive` work as in Vite;
  a `!` pattern leaves files out. `include` limits the transform to some files,
  as a rule's `include` does: a path from the project root, a regular
  expression, or a list of them. Calls in comments and strings are left as
  written.
- **`svgSprite`**: every `.svg` in `dirs`, relative to the project root,
  becomes a `<symbol>`. `symbolId` names each one: `[dir]` is the icon's
  folder inside `dirs` (with `-` for `/`) and `[name]` its file name, and
  the default is `icon-[dir]-[name]`. Importing `virtual:svg-icons-register`
  inserts the sprite into the page; `virtual:svg-icons-names` exports the IDs.
  `register` and `names` rename those imports. Each symbol keeps the icon's
  view box, or takes one from its width and height; IDs inside an icon are
  prefixed with its symbol ID, and so are the references to them.
- Adding, editing or deleting a file a plugin reads, such as an icon or a file
  a glob matches, rebuilds the previews that use it.

A loader is a file that exports a function from source text to module text.
Name it in a rule by path, relative to the project root:

```ts
rules: [{ test: /\.tsx?$/, exclude: /node_modules/, enforce: 'pre', loader: './workbench/greeting-loader.cjs' }],
```

Loaders run in Node, one module at a time, with the usual loader context:
`this.getOptions()`, `this.resourcePath`, `this.async()`, `this.addDependency()`
and the like. `this.emitError()` fails the module's build with its message,
as a thrown error does; `this.emitWarning()` reports a warning and the build
continues. `this.loadModule` and `this.importModule` are not available.
Rspack's built-in loaders, such as `builtin:lightningcss-loader`, run in the
compiler itself.

Rules and plugins apply when Workbench builds previews and docs examples, live
and for the [design-system export](design-system-export.md). They don't apply
when Workbench reads a definition or this file, or in
[Astro frontmatter](astro.md#imports-aliases-and-plugins), so a `.workbench.ts`
file can't import a file that only a loader can read.

An object with `apply(compiler)` (an Rspack plugin) or `setup(build)` (an
esbuild plugin) doesn't run: it is reported as a configuration problem and
left out. Use a helper above, or a loader rule.

- **Build absolute paths from `__dirname`,** the folder of the configuration
  file. Don't rely on the current working directory.

### aliases

```ts
aliases: {
  '@acme/tokens': './src/tokens.ts',
  '@acme/tokens/colors': './src/tokens/colors.ts',
  'acme-analytics': './preview/analytics-mock.ts',
  '#runtime': 'acme-runtime',
},
```

- A key matches one exact package-style import: `@acme/tokens` doesn't match
  `@acme/tokens/colors`, so list each subpath you import. Keys that start with
  `.` or `/` never match.
- A value that starts with `.`, or an absolute path, is a file relative to the
  project root. Any other value is a package, resolved from the definition's
  folder.
- `tsconfig.json` `paths` also apply, without an alias.
- Aliases also apply to [Astro frontmatter](astro.md).

### dedupe

```ts
dedupe: ['react', 'react-dom'],
```

Imports of these packages resolve from the folder of the preview definition, as
if the definition imported them, instead of from the file that imports them. In
a monorepo where a shared package has its own copy of a framework, this gives
every module in the preview the same copy.

### resolveExtensions

```ts
resolveExtensions: ['.web.ts', '.ts', '.js'],
```

The extensions tried, in order, for an import without one. The list replaces
the default, `.tsx`, `.ts`, `.jsx`, `.js`, `.mjs`, `.cjs`, `.json`, `.vue`,
`.html`, so include every extension your previews import without one. The
`react-native-web` adapter puts `.web.tsx`, `.web.ts`, `.web.jsx`, and
`.web.js` first when you don't set this. It doesn't apply to Astro frontmatter.

### define

```ts
define: {
  'process.env.API_URL': JSON.stringify('https://api.example.com'),
  __ACME_FLAG__: 'true',
},
```

Each key is an identifier or property chain, and each value is the code that
replaces it, so wrap strings in `JSON.stringify`. Workbench sets
`process.env.NODE_ENV` to `"development"` on the canvas and `"production"` in
`check`, `build`, and exports; your `define` can override it. Defines also
apply to Astro frontmatter, where they set
[`import.meta.env` values](astro.md#environment-values).

## Replace a built-in adapter

Registering an adapter named `html`, `react`, `vue`, `astro`, or
`react-native-web` replaces the built-in one for every preview in the project:

```ts
export default defineConfig({
  adapters: {
    react: { runtime: './preview/react-runtime.tsx' },
  },
});
```

Your runtime receives the selected export and does all the mounting, including
any providers. A replaced `astro` adapter doesn't render on the server:
`.astro` files then need a rule that compiles them for the browser, or the
build fails with `Module parse failed`. Replacing
`react-native-web` keeps its `react-native` alias and `.web.*` extensions.

## Errors and fixes

| Error | Where | Fix |
| --- | --- | --- |
| `Unknown adapter “<name>”. Register it in workbench.config.ts.` | The frame and `check` | Add the name to `adapters`, or fix the definition's `adapter`. |
| `adapter must be a registered kebab-case name.` | Configuration problems | Use lowercase letters, digits, and hyphens in the adapter name. |
| `Module not found: Can't resolve '<path>'` on a runtime path | The frame and `check` | Fix `runtime`. It is relative to the project root. |
| `adapter.mount is not a function` | The frame | Export a function named `mount` from the runtime. |
| `The selected source export does not exist for <id> — <scenario>` | The frame only; `check` passes | Fix `source.export`, or export that name from the entry. |
| `Module parse failed` on a file type Workbench doesn't compile | The frame and `check` | Add a rule with a loader for the file type. |
| `workbench.config.ts: “<name>” is an esbuild plugin.` or `is a JavaScript Rspack plugin.` | Configuration problems | Use a plugin from `@canonic2/workbench`, or a loader rule. |
| `Workbench previews: <message>` | Configuration problems | `workbench.config.ts` threw while loading. No previews load until it's fixed; `check` prints the same message. |
| `previews.config: must be a project-relative config file.` | Configuration problems | Point `previews.config` at a file inside the project. Previews stay off until it's fixed. |

For configuration problems and the log, see
[A TypeScript preview is missing or broken](troubleshooting.md#a-typescript-preview-is-missing-or-broken).
