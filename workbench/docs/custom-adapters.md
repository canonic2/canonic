# Custom adapters and compiler settings

Workbench previews ship with adapters for HTML, React, Vue, Astro, and React
Native Web. To preview anything else, such as web components, Svelte, Solid, or
an in-house rendering library, register an adapter in `workbench.config.ts`. The
same file holds the compiler settings every preview shares: esbuild plugins,
import aliases, deduplicated packages, file extensions, and build-time
constants.

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
  [trusted workspace](extension.md#workspace-trust). Plugins and runtimes run
  project code.
- Whatever your adapter renders with, installed in your project. Workbench
  bundles your runtime module with the project's own packages and installs
  nothing.
- Plugins written for [esbuild's plugin API](https://esbuild.github.io/plugins/).
  Vite, Rollup, and Babel plugins need a wrapper that calls them from esbuild's
  `onLoad` or `onResolve`.

## Example: web components

This example previews a custom element whose shadow DOM styles are imported with
a Vite-style `?inline` suffix. It registers a `web-components` adapter, with a
plugin that loads `?inline` imports as text. It needs no packages.

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

`workbench.config.ts` registers the adapter and its plugin:

```ts
import fs from 'node:fs';
import { defineConfig } from '@canonic2/workbench';

// Imports ending in `?inline` load the file's text, for shadow DOM styles.
const inlineText = {
  name: 'acme-inline-text',
  setup(build) {
    build.onResolve({ filter: /\?inline$/ }, async args => {
      const result = await build.resolve(args.path.slice(0, -'?inline'.length), {
        resolveDir: args.resolveDir,
        kind: args.kind,
      });
      return { path: result.path, errors: result.errors, pluginData: { inline: true } };
    });
    build.onLoad({ filter: /.*/ }, args => {
      if (!args.pluginData?.inline) return;
      return { contents: fs.readFileSync(args.path, 'utf8'), loader: 'text' };
    });
  },
};

export default defineConfig({
  adapters: {
    'web-components': {
      runtime: './preview/web-components.ts',
      plugins: [inlineText],
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
  states: {
    default: {},
    disabled: { inputs: { disabled: true } },
  },
});
```

Run **Workbench: Refresh Screens**. **Button** appears under **Components**
with **Default** and **Disabled** states. Editing **label** in
**Preview controls** mounts the element again with the new value, and pressing
the button logs `acme-press` in **Actions**. The
[checker](workbench-previews.md#command-line-tools) reports:

```text
$ node <extension>/preview/cli.cjs check acme-ui
Built components/acme-button
```

esbuild also reads `import styles from './acme-button.css' with { type: 'text' }`
without a plugin. The plugin is for code that already uses `?inline`.

## The runtime module

`runtime` is a browser module, relative to the project root, that exports
`mount(canvas, source, context, environment)`. Workbench bundles it with each
preview that uses the adapter, so it can import your framework like any other
module.

| Argument | Value |
| --- | --- |
| `canvas` | The empty element to render into. |
| `source` | The export of the state's `source.entry` named by `source.export` (`default` when omitted), after your plugins compile it. |
| `context` | The [preview context](workbench-previews.md#lifecycle-hooks): `id`, `state`, `inputs`, `fixtures`, `globals`, `signal`, `action`, `navigate`, and `error`. |
| `environment` | Everything the definition's `environment` module exports, or `{}` when it has none. Define your own hooks here, such as `wrap` above. With a [project-wide environment](preview-data.md#environments) too, the two are combined: `setup`, `mount`, `ready`, `configure`, and `wrap` run both, the project's outside, and other exports come from the definition's module first. |

- `mount` may return a cleanup function, or a promise for one. Workbench waits
  for the promise before it runs `play` and `ready` hooks and marks the preview
  ready, so resolve it once the component has rendered.
- Workbench calls `mount` again for every state change, input edit, and
  **Reset state**, after aborting `context.signal` and running the previous
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

## Configuration file

Workbench reads `workbench.config.ts` at the project root. To use another file
inside the project, set [`previews.config`](configuration.md#previews) in
`workbench.yaml`. Saving the file, or a file it imports, rebuilds the open
preview.

Workbench runs the file in Node. Relative imports are bundled with it; packages
are loaded from your project's `node_modules`. `defineConfig` from
`@canonic2/workbench` only adds types.

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  adapters: { /* name: { runtime, plugins } */ },
  environment: './src/workbench/environment.tsx',
  plugins: [],
  aliases: {},
  dedupe: [],
  resolveExtensions: [],
  define: {},
});
```

| Key | Type | Description |
| --- | --- | --- |
| `adapters` | map of name to `{ runtime, plugins? }` | Adapters that definitions name in `adapter`. |
| `environment` | path, or map of adapter name to path | An [environment](preview-data.md#environments) around every preview, or around each adapter's previews, relative to the project root. |
| `plugins` | list of esbuild plugins | Plugins for every preview. |
| `aliases` | map of import specifier to path or package | Redirects exact imports. |
| `dedupe` | list of package names | Packages resolved from the definition's folder. |
| `resolveExtensions` | list of extensions | The extensions tried for imports without one. Replaces the default list. |
| `define` | map of expression to code string | Constants replaced at build time. |

### adapters

Each key is the name a definition gives as `adapter`. Definitions only accept
kebab-case names, such as `web-components`. Each value has:

- `runtime`: the [runtime module](#the-runtime-module), relative to the project
  root.
- `plugins`: esbuild plugins for previews that use this adapter only.
  `defineAdapter` from `@canonic2/workbench` types one adapter on its own.

### plugins

Global `plugins` apply to every preview. Adapter `plugins` apply to previews
that use the adapter. In a build, global plugins run first, then the adapter's,
then Workbench's own resolvers and loaders, so your plugins decide first how a
file resolves and loads.

Plugins run when Workbench bundles a preview for the browser, and global plugins
also run when it renders [Astro frontmatter](astro.md#imports-aliases-and-plugins)
in Node. They don't run when Workbench reads a definition or this file, so a
`.workbench.ts` file can't import a file that only a plugin can load.

- **Keep real file paths.** Workbench reloads a preview and copies source files
  into exports based on the files a build reads. A plugin that returns its own
  `namespace` or a `suffix` for a file hides it: edits to the file don't reload
  the preview, and exports leave it out. Return the file's path and carry
  anything else in `pluginData`, as the example does.
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
`.astro` files then need a plugin that compiles them for the browser, or the
build fails with `No loader is configured for ".astro" files`. Replacing
`react-native-web` keeps its `react-native` alias and `.web.*` extensions.

## Errors and fixes

| Error | Where | Fix |
| --- | --- | --- |
| `Unknown adapter “<name>”. Register it in workbench.config.ts.` | The frame and `check` | Add the name to `adapters`, or fix the definition's `adapter`. |
| `adapter must be a registered kebab-case name.` | Configuration problems | Use lowercase letters, digits, and hyphens in the adapter name. |
| `Could not resolve "<path>"` on a runtime path | The frame and `check` | Fix `runtime`. It is relative to the project root. |
| `adapter.mount is not a function` | The frame | Export a function named `mount` from the runtime. |
| `The selected source export does not exist for <id> — <state>` | The frame only; `check` passes | Fix `source.export`, or export that name from the entry. |
| `No loader is configured for ".<ext>" files` | The frame and `check` | Add a plugin that loads the file type. |
| `Cannot import "<file>" into a JavaScript file without an output path configured` | The frame and `check` | A definition imports a file only a plugin can load, or an Astro frontmatter plugin returned CSS. Move the import into the source, or return JavaScript. |
| `Workbench previews: <message>` | Configuration problems | `workbench.config.ts` threw while loading. No previews load until it's fixed; `check` prints the same message. |
| `previews.config: must be a project-relative config file.` | Configuration problems | Point `previews.config` at a file inside the project. Previews stay off until it's fixed. |

For configuration problems and the worker's log, see
[A TypeScript preview is missing or broken](troubleshooting.md#a-typescript-preview-is-missing-or-broken).
