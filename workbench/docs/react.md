# React previews

The `react` adapter renders one of your React components on the Workbench
canvas, with the preview's inputs as its props. Each named state is a set of
props, and you can edit them in **Preview controls** while you review.

Use it for components and pages that render in the browser. To review your
whole application as it runs, use a [URL lens](lenses.md). For Storybook
stories, see [Storybook](storybook.md).

This guide covers what is specific to React. The definition keys, states,
controls, lifecycle hooks, command-line tools, and portable exports are the
same for every adapter and are described in
[TypeScript Workbench previews](workbench-previews.md).

## Requirements

- `react` and `react-dom` 18 or later, installed in your project. Workbench
  compiles your TypeScript and JSX itself but uses your project's React. It
  finds `react` and `react-dom` from the definition's folder, the way Node
  does, and never installs or replaces them.
- `workbench.yaml` at the project root. `name: Acme` is enough.
- A [trusted workspace](extension.md#workspace-trust), since previews run
  project code.

## Write a first preview

```text
acme-web/
├── workbench.yaml
├── package.json            react and react-dom
└── src/components/
    ├── Button.tsx
    ├── Button.css
    └── Button.workbench.ts
```

`src/components/Button.tsx`:

```tsx
import type { ReactNode } from 'react';
import './Button.css';

export interface ButtonProps {
  label: string;
  tone?: 'primary' | 'secondary';
  disabled?: boolean;
  icon?: ReactNode;
  onClick?: () => void;
}

export function Button({ label, tone = 'primary', disabled = false, icon, onClick }: ButtonProps) {
  return (
    <button className={`acme-button acme-button--${tone}`} disabled={disabled} onClick={onClick}>
      {icon}
      {label}
    </button>
  );
}
```

`src/components/Button.css`:

```css
.acme-button {
  font: 600 16px/1 system-ui, sans-serif;
  padding: 12px 20px;
  border: 0;
  border-radius: 8px;
}
.acme-button--primary { background: #2563eb; color: white; }
.acme-button--secondary { background: #e5e7eb; color: #111827; }
.acme-button:disabled { opacity: 0.5; }
```

`src/components/Button.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/button',
  title: 'Components/Button',
  adapter: 'react',
  source: { entry: './Button.tsx', export: 'Button' },
  inputs: { label: 'Continue', tone: 'primary', disabled: false },
  controls: {
    label: { type: 'text' },
    tone: { type: 'select', options: ['primary', 'secondary'] },
    disabled: { type: 'boolean' },
  },
  states: {
    primary: {},
    secondary: { inputs: { tone: 'secondary' } },
    disabled: { inputs: { disabled: true } },
  },
});
```

Run **Workbench: Refresh Pages**. **Button** appears in a **Components**
collection of the sidebar, with the states **Primary**, **Secondary**, and
**Disabled**. To build it without the canvas, run the
[checker](workbench-previews.md#command-line-tools) on the project; it prints
`Built components/button`.

`source.export` names the component's export and defaults to `default`. Each
state renders in a fresh React root, and the previous root is unmounted first.

Name the component in `source` rather than importing it into the definition.
Workbench runs the definition in Node to list your previews, so a definition
that imports a component which imports an image fails with
`No loader is configured for ".png" files`. Importing types with
`import type` is fine.

## Props, children, and callbacks

The merged `inputs` of the preview, the state, and any edits in
**Preview controls** are passed as the component's props. Inputs must be plain
data that can be copied, such as strings, numbers, booleans, arrays, and
objects.

- **Text children:** set a `children` input, such as
  `inputs: { children: 'Everything your team needs.' }`. Give it a `text`
  control to edit it.
- **Elements, such as icons or slots:** an element can't be an input. Write a
  small preview-only module that supplies it, and render that module in a
  state with the state's `source`:

  ```tsx
  // src/components/Button.preview.tsx
  import { Button, type ButtonProps } from './Button';

  function StarIcon() {
    return <span aria-hidden="true">★ </span>;
  }

  export function WithIcon(props: ButtonProps) {
    return <Button {...props} icon={<StarIcon />} />;
  }
  ```

  ```ts
  states: {
    primary: {},
    'with-icon': { source: { entry: './Button.preview.tsx', export: 'WithIcon' } },
  },
  ```

  The module's name doesn't end in `.workbench.ts`, so it isn't listed as a
  preview of its own.
- **Callbacks:** a function can't be an input either. Supply callbacks from an
  [environment](#add-providers-and-context) and report each call with
  `context.action`, so it appears under **Actions** in **Preview controls**.

## Add providers and context

Name an `environment` module in the definition to wrap the component in the
providers it needs: a theme, a router, translations, or a data client. The
module's `wrap(element, context)` receives the component's element and returns
the tree to render. It runs on every render, so read the current state from
`context`.

`src/preview/environment.tsx`:

```tsx
import { cloneElement, type ReactElement } from 'react';
import type { PreviewContext } from '@canonic2/workbench';
import { ThemeProvider, type Theme } from '@/theme';

export function setup(context: PreviewContext) {
  document.documentElement.lang = String(context.globals.locale ?? 'en');
}

export function wrap(element: ReactElement<Record<string, unknown>>, context: PreviewContext) {
  const withActions = cloneElement(element, {
    onSelect: (id: string) => context.action('select', id),
  });
  return <ThemeProvider theme={(context.globals.theme as Theme) ?? 'light'}>{withActions}</ThemeProvider>;
}
```

`src/components/Card.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/card',
  title: 'Components/Card',
  adapter: 'react',
  source: { entry: './Card.tsx' },
  environment: '../preview/environment.tsx',
  styles: ['../styles/global.css'],
  inputs: { title: 'Acme Pro', children: 'Everything your team needs.' },
  globals: { theme: 'light', locale: 'en' },
  controls: { title: { type: 'text' }, children: { type: 'text', label: 'Body' } },
  states: {
    light: {},
    dark: { globals: { theme: 'dark' } },
  },
});
```

- `@/theme` stands for your own theme module, resolved through
  [`tsconfig.json` `paths`](#imports-jsx-and-environment-variables).
- `cloneElement` adds props to the component, here an `onSelect` callback that
  logs `select` with its argument. Use the same pattern for `onClick`,
  `onChange`, or any other callback prop.
- `globals` carry settings for the environment, such as the theme and locale,
  and each state can override them.
- The environment can also export `setup` and `ready`, which run with the
  [lifecycle hooks](workbench-previews.md#lifecycle-hooks) of the preview and
  its states.
- Several definitions can share one environment module. For providers every
  preview needs, name it once as the
  [project-wide environment](preview-data.md#environments)
  instead.
- A data client such as Apollo, React Query, or SWR can stay real: wrap the
  component in it as usual and answer its requests with
  [`requests`](preview-data.md#request-mocks), so each state shows its
  own data. To seed the client's cache instead, do it in `wrap` or `setup` from
  `context.fixtures`.

Workbench doesn't add `React.StrictMode`. Return it from `wrap` if you want
it.

## Styles, fonts, and images

| What you write | What Workbench does |
| --- | --- |
| `import './Button.css'` in a component | Bundles the CSS and loads it with the preview |
| `import styles from './Card.module.css'` | Treats it as a CSS module, with locally scoped class names |
| `styles: ['../styles/global.css']` in the definition | Loads global CSS, such as resets, tokens, and `@font-face` rules, that no component imports |
| `url(...)` in CSS | Copies the image or font and rewrites the URL |
| `import logo from './logo.svg'` | Copies the file; `logo` is its URL, for an `img` `src` |
| `import copy from './copy.json'` | Imports the parsed JSON |

Files you can import or reference from CSS are `.svg`, `.png`, `.jpg`,
`.jpeg`, `.gif`, `.webp`, `.avif`, `.woff`, `.woff2`, `.ttf`, `.mp4`, and
`.mp3`, plus CSS, JSON, and script files. A preview waits for its fonts and
visible images to load before it counts as ready.

The `<html>` element carries `data-wb-state`, so a global stylesheet can
change with the state, for example
`:root[data-wb-state='dark'] body { background: #111827; }`.

These need more than the built-in compiler:

- **Sass, Less, and Stylus** fail with
  `No loader is configured for ".scss" files`.
- **PostCSS and Tailwind** don't run. `@tailwind` and `@apply` are left
  unprocessed, so utility classes have no styles. Build the CSS with your own
  tooling and list the output file in `styles`.
- **SVG as a React component**, such as SVGR's
  `import { ReactComponent as Logo } from './logo.svg'`, fails with
  `No matching export in "logo.svg" for import "ReactComponent"`. An SVG import
  is a URL.
- **Other file types**, such as `.otf` fonts or `.webm` video, fail with
  `No loader is configured`.

For any of these, add a [compiler plugin](custom-adapters.md#plugins)
in `workbench.config.ts`.

## Imports, JSX, and environment variables

- **JSX** compiles with React's automatic runtime, so components don't need
  `import React`. A `jsx` or `jsxImportSource` setting in the nearest
  `tsconfig.json` takes precedence. With `"jsx": "react"`, each file that uses
  JSX needs `import React from 'react'`, or the preview fails with
  `React is not defined`.
- **`paths` in `tsconfig.json`**, such as `"@/*": ["./src/*"]`, resolve as
  they do in your editor.
- **`aliases` in `workbench.config.ts`** replace an exact import specifier with
  a package or a project file. Use them to swap a module for a mock. An alias
  for `@acme/analytics` doesn't apply to `@acme/analytics/track`; add that
  specifier as well, or use `tsconfig.json` `paths` for a prefix.
- **`process.env.NODE_ENV`** is `development` on the canvas and `production`
  in exports. Any other `process.env` or `import.meta.env` value is undefined
  in the browser and fails with `process is not defined` or
  `Cannot read properties of undefined`. Set it with `define`.

`workbench.config.ts` at the project root:

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  aliases: { '@acme/analytics': './src/preview/analytics-mock.ts' },
  define: {
    'process.env.API_URL': JSON.stringify('https://api.example.com'),
    'import.meta.env': JSON.stringify({ VITE_API_URL: 'https://api.example.com' }),
  },
});
```

Each `define` value is code, so wrap strings in `JSON.stringify`. Relative
alias paths start from the project root. Workbench doesn't read Vite, webpack,
or Next.js configuration; repeat what your previews need here.

## Use one copy of React in a monorepo

The adapter loads React from the definition's folder, and each component
loads React from its own folder. When those resolve to two installations,
hooks fail with an error such as
`Cannot read properties of null (reading 'useState')`. List the packages in
`dedupe` so every import of them resolves from the definition's folder:

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  dedupe: ['react', 'react-dom'],
});
```

Add any other package that must have a single copy, such as a context-based
design system.

## Document components with a docs page

A page's [docs](docs-pages.md) show a component's examples in Markdown, each
in its own panel with its code. Render the examples with React through a
`docs` lens whose `adapter` is `react`:

```yaml
implementations:
  web:
    kind: docs
    label: Web
    adapter: react
    styles:
      - src/styles/global.css

collections:
  - name: Components
    items:
      - label: Button
        src: docs/button.md
        implementations:
          web: src/components/button-examples/
```

Each example is a React component:

- **A folder**, written with a trailing `/`: each file directly inside it is
  one example, its default export.
  `src/components/button-examples/secondary.tsx` is the example `secondary`, and **Show code** shows the whole file.
- **A file**: each named export is one example. `export function WithIcon()`
  in `src/components/Button.examples.tsx` is the example `with-icon`, and
  **Show code** shows that export's statement.

`docs/button.md` places each example by its ID, in a fenced block whose info
string is `example` and the ID, such as `example secondary`; see
[Write the Markdown](docs-pages.md#write-the-markdown).

`src/components/button-examples/secondary.tsx`:

```tsx
import { Button } from '../Button';

export default function Secondary() {
  return <Button label="Cancel" tone="secondary" />;
}
```

Examples receive no props and have no controls: write the props in the
example. The project's environment for `react`, and the lens's `environment`,
wrap each example with `wrap`, so the providers your previews use apply to the
examples too; see
[Examples on docs pages](preview-data.md#examples-on-docs-pages). To render
the same Markdown with React Native Web, add a second lens with
`adapter: react-native-web`. To read the docs beside a component's design or
preview, give that page `docs: docs/button.md` in place of a Markdown `src`.
A `*.workbench.ts` file can declare a Markdown page and its lenses with
`defineDocs` instead; see
[Docs](docs-pages.md#declare-a-markdown-page-in-a-definition).

## What isn't supported

- The adapter renders a component in the browser. It doesn't run your
  application's server, server components, server actions, or framework data
  loaders. Supply data through inputs, providers in an environment,
  [request mocks](preview-data.md#request-mocks), or mocked modules.
- Inputs can't hold functions or elements. Use a
  [preview-only module or an environment](#props-children-and-callbacks).
- Saving a file reloads the preview; component state isn't preserved.

## Errors and fixes

| Error | Cause and fix |
| --- | --- |
| `Could not resolve "react"`, `"react-dom/client"`, or `"react/jsx-runtime"` | React isn't installed where the definition and component can find it. Install `react` and `react-dom` in the project. |
| `React is not defined` | `tsconfig.json` sets `"jsx": "react"`. Import React in the file, or use `"jsx": "react-jsx"`. |
| `Cannot read properties of null (reading 'useState')`, or React's invalid hook call error | Two copies of React. Add them to [`dedupe`](#use-one-copy-of-react-in-a-monorepo). |
| `The selected source export does not exist for <id> — <state>` | `source.export` doesn't match an export of the entry. A named export needs `export: 'Button'`. |
| `DataCloneError: … could not be cloned` | An input holds a function or another value that can't be copied. See [Props, children, and callbacks](#props-children-and-callbacks). |
| `process is not defined`, or `Cannot read properties of undefined (reading 'VITE_…')` | An environment variable isn't defined. Add it to [`define`](#imports-jsx-and-environment-variables). |
| `No loader is configured for ".<ext>" files` | An unsupported file type, or a definition that imports a component. See [Styles, fonts, and images](#styles-fonts-and-images) and [Write a first preview](#write-a-first-preview). |
| `Could not resolve "<package>/<path>"` with an alias set | Aliases match whole specifiers only. Alias that specifier too. |

For previews that don't appear in the list, see
[A TypeScript preview is missing or broken](troubleshooting.md#a-typescript-preview-is-missing-or-broken).
