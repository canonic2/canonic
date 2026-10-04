# Vue previews

A Vue preview renders a Vue 3 component from your project on the canvas, with
the preview's inputs as its props. It is a
[TypeScript preview](workbench-previews.md) with `adapter: 'vue'`: you write a
`.workbench.ts` definition next to the component, and each state appears in the
sidebar like any other screen. Workbench compiles your `.vue` files itself, so
you don't need Vite or a build step.

This guide covers what is specific to Vue. For the definition keys, controls,
lifecycle hooks, and the command-line checker, see
[TypeScript Workbench previews](workbench-previews.md).

## Requirements

- **Vue 3 in your project.** Workbench imports `vue` from your project, resolved
  from the definition file's folder the way Node resolves packages. It never
  installs or replaces it. Without it, the build fails with
  `Could not resolve "vue"`.
- **Nothing else from the Vue toolchain.** Workbench compiles `.vue` files with
  its own bundled `@vue/compiler-sfc` 3.5. It doesn't check your Vue version,
  and it is tested with Vue 3.5. It doesn't read `vite.config.*` or
  `vue.config.*`.
- The general requirements in
  [TypeScript Workbench previews](workbench-previews.md): a `workbench.yaml` and a
  trusted workspace.

## A minimal preview

```text
acme/
├── workbench.yaml
├── package.json            (with vue installed)
└── src/components/
    ├── AcmeButton.vue
    └── AcmeButton.workbench.ts
```

`workbench.yaml` only needs a name:

```yaml
name: Acme
```

`src/components/AcmeButton.vue`:

```vue
<script setup lang="ts">
withDefaults(defineProps<{
  label: string;
  tone?: 'primary' | 'secondary';
  disabled?: boolean;
}>(), { tone: 'primary', disabled: false });

const emit = defineEmits<{ press: [label: string] }>();
</script>

<template>
  <button class="button" :class="tone" :disabled="disabled" @click="emit('press', label)">
    {{ label }}
  </button>
</template>

<style scoped>
.button { font: 600 15px/1 system-ui, sans-serif; padding: 12px 20px; border: 0; border-radius: 8px; }
.primary { background: #1f5eff; color: white; }
.secondary { background: #e8edf7; color: #1b2433; }
.button:disabled { opacity: 0.5; }
</style>
```

`src/components/AcmeButton.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/button',
  title: 'Components/Button',
  adapter: 'vue',
  source: { entry: './AcmeButton.vue' },
  viewports: ['fit'],
  inputs: { label: 'Continue', tone: 'primary', disabled: false },
  controls: {
    label: { type: 'text' },
    tone: { type: 'select', options: ['primary', 'secondary'] },
    disabled: { type: 'boolean' },
  },
  states: {
    default: {},
    secondary: { inputs: { tone: 'secondary' } },
    disabled: { inputs: { disabled: true } },
  },
});
```

Run **Workbench: Refresh Screens**. **Button** appears in a **Components**
section with the states **Default**, **Secondary**, and **Disabled**, and
**Preview controls** lets you edit the label, tone, and disabled flag. To check
the project without the editor, run the
[command-line checker](workbench-previews.md#command-line-tools); it prints
`Built components/button`.

`source.entry` can be a `.vue` file or a TypeScript or JavaScript module. A
`.vue` file has only a default export. In a module, `source.export` names the
component to render, such as a `defineComponent(...)` export, and defaults to
`default`.

## Inputs and props

Workbench renders the component with `h(Component, inputs)`. That means:

- Each input is passed as a prop. A state's `inputs` override the preview's, and
  edits in **Preview controls** override both until you reset or change state.
- An input that the component doesn't declare as a prop falls through to its
  root element as an HTML attribute, as Vue does for any undeclared attribute.
- Inputs must be plain data. A function, such as an `onClick` handler, fails
  the render with `DataCloneError: … could not be cloned`. To handle events, see
  [Events and actions](#events-and-actions).
- Props stay as the state sets them. When the component emits
  `update:modelValue`, the event can be recorded, but the `modelValue` input
  doesn't change. Add a state for the result you want to review.

A state can render a different component by setting its own `source`, such as
`{ entry: './AcmeButtonGroup.vue' }`.

## Events and actions

Workbench doesn't listen to a component's events by itself. To record them
under **Actions** in **Preview controls**, add an `environment` module whose
`wrap` export adds a listener for each event the component declares.

`preview/vue-environment.ts`:

```ts
import { camelize, cloneVNode, toHandlerKey, type VNode } from 'vue';
import type { PreviewContext } from '@canonic2/workbench';

// Adds a listener for every event the component declares in `emits`,
// and records each emit under Actions.
export function wrap(vnode: VNode, context: PreviewContext) {
  const emits = (vnode.type as { emits?: string[] | Record<string, unknown> }).emits ?? [];
  const names = Array.isArray(emits) ? emits : Object.keys(emits);
  const listeners = Object.fromEntries(names.map(name => [
    toHandlerKey(camelize(name)),
    (...values: unknown[]) => context.action(name, ...values),
  ]));
  return cloneVNode(vnode, listeners);
}
```

Point the definition at it. The path is relative to the definition:

```ts
  source: { entry: './AcmeButton.vue' },
  environment: '../../preview/vue-environment.ts',
```

Clicking the button now records `press` with the value `Continue`. The same
module works for every component that declares its events, with
`defineEmits` or the `emits` option, including `update:modelValue`. An event
the component emits without declaring it isn't recorded.

`wrap(vnode, context)` receives the component's virtual node and returns what
to render, so it can also add a layout or providing component around it. One
environment can serve every preview; give each definition the same
`environment` path.

## Slots

The adapter passes props only, never slot content. To fill slots, write a small
preview component that uses yours, and point `source` at it.

`src/components/AcmeCard.vue`:

```vue
<script setup lang="ts">
defineProps<{ title: string }>();
</script>

<template>
  <article class="card">
    <header>
      <h2>{{ title }}</h2>
      <slot name="actions" />
    </header>
    <slot />
  </article>
</template>

<style scoped>
.card { border: 1px solid #dde3ee; border-radius: 12px; padding: 16px; max-width: 360px; }
header { display: flex; justify-content: space-between; align-items: center; }
</style>
```

`src/components/AcmeCard.preview.vue`:

```vue
<script setup lang="ts">
import AcmeButton from './AcmeButton.vue';
import AcmeCard from './AcmeCard.vue';

defineProps<{ title: string; body: string; editable: boolean }>();
const emit = defineEmits<{ edit: [] }>();
</script>

<template>
  <AcmeCard :title="title">
    <template v-if="editable" #actions>
      <AcmeButton label="Edit" tone="secondary" @press="emit('edit')" />
    </template>
    <p>{{ body }}</p>
  </AcmeCard>
</template>
```

`src/components/AcmeCard.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/card',
  title: 'Components/Card',
  adapter: 'vue',
  source: { entry: './AcmeCard.preview.vue' },
  environment: '../../preview/vue-environment.ts',
  inputs: { title: 'Team plan', body: 'Up to 20 seats, billed monthly.', editable: true },
  controls: {
    title: { type: 'text' },
    body: { type: 'text' },
    editable: { type: 'boolean' },
  },
  states: {
    default: {},
    'no-actions': { inputs: { editable: false } },
  },
});
```

The preview component's inputs fill the slots, and the `wrap` from the previous
section records its `edit` event. The file name is up to you: only
`.workbench.ts` and `.workbench.tsx` files are read as definitions.

## App setup: plugins, provide, router, and i18n

Each render creates a new app with Vue's `createApp` and mounts it into the
canvas. Changing state, editing an input, or resetting unmounts that app and
creates another. Before each app mounts, Workbench calls the environment's
`configure(app, context)` export, which may be async. Install plugins and
`provide` values there. For plugins every screen needs, name the module once
as the [project-wide environment](preview-data.md#environments).
A component that loads its own data with `fetch`, axios, or a client built on
them needs no store setup: answer its requests per state with
[`requests`](preview-data.md#request-mocks).

`src/i18n.ts`, a small plugin for this example:

```ts
import type { App } from 'vue';

const messages: Record<string, Record<string, string>> = {
  'en-US': { greeting: 'Welcome back' },
  'fr-FR': { greeting: 'Bon retour' },
};

export function createAcmeI18n(locale: string) {
  return {
    install(app: App) {
      app.config.globalProperties.$t = (key: string) => messages[locale]?.[key] ?? key;
      app.provide('acme:locale', locale);
    },
  };
}
```

Add `configure` to `preview/vue-environment.ts`, next to `wrap`:

```ts
import { camelize, cloneVNode, toHandlerKey, type App, type VNode } from 'vue';
import type { PreviewContext } from '@canonic2/workbench';
import { createAcmeI18n } from '../src/i18n';

// Runs before every render, on a new app.
export function configure(app: App, context: PreviewContext) {
  app.use(createAcmeI18n(String(context.globals.locale ?? 'en-US')));
}
```

Use `globals` to vary the setup by state. `src/components/AcmeGreeting.vue`:

```vue
<script setup lang="ts">
import { inject } from 'vue';

defineProps<{ name: string }>();
const locale = inject<string>('acme:locale');
</script>

<template>
  <p class="greeting" :lang="locale">
    <img src="../assets/logo.svg" alt="" width="24" height="24">
    {{ $t('greeting') }}, {{ name }}
  </p>
</template>
```

`src/components/AcmeGreeting.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/greeting',
  title: 'Components/Greeting',
  adapter: 'vue',
  source: { entry: './AcmeGreeting.vue' },
  environment: '../../preview/vue-environment.ts',
  styles: ['../styles/tokens.css'],
  inputs: { name: 'Dana' },
  globals: { locale: 'en-US' },
  states: {
    english: {},
    french: { globals: { locale: 'fr-FR' } },
  },
});
```

**English** shows "Welcome back, Dana" and **French** shows "Bon retour, Dana".

Install a router, a store, or an i18n library the same way: create it inside
`configure` and pass it to `app.use`, so every render starts from a fresh
instance. A router reads the preview's own address unless you give it memory
history, so for Vue Router use `createMemoryHistory()` and push the route the
state needs before `configure` returns.

If `configure` sets `app.config.errorHandler`, Workbench still reports the
error in the preview's frame, then calls your handler.

## Styles, fonts, and images

- **Component styles.** `<style>` and `<style scoped>` blocks are compiled and
  loaded with the preview, and `v-bind()` in CSS works.
- **Global CSS.** List stylesheets in the definition's `styles`, relative to the
  definition, or import them from a script with `import './tokens.css'`. Both
  are bundled into the preview's stylesheet, with their `@import` rules.
- **Files referenced from CSS and templates.** A relative `url()` in CSS, and a
  relative `src` in a template such as `<img src="../assets/logo.svg">`, is
  resolved from the file it appears in and copied with the preview. Importing a
  file in a script, as in `import logoUrl from '../assets/logo.svg'`, gives its
  URL. An SVG import is a URL, not a component.
- **File types.** Fonts: `.woff`, `.woff2`, `.ttf`. Images: `.svg`, `.png`,
  `.jpg`, `.jpeg`, `.gif`, `.webp`, `.avif`. Media: `.mp4`, `.mp3`. JSON imports
  as data. Any other type in a `url()` or import fails the build, such as
  `No loader is configured for ".otf" files`. Convert the file, or handle it
  with a [compiler plugin](workbench-previews.md#register-other-technologies).

`src/styles/tokens.css`, listed in `styles` in the greeting example:

```css
@font-face {
  font-family: 'Acme Sans';
  src: url('../assets/acme-sans.ttf') format('truetype');
}
:root { --acme-blue: #1f5eff; }
body { font-family: 'Acme Sans', system-ui, sans-serif; }
```

Not supported without a compiler plugin:

- **CSS preprocessors and external style files.** `<style lang="scss">` and
  other `lang` values, and `<style src="...">`, fail the build with
  `Vue style preprocessors need a compiler plugin: <file>`.
- **CSS Modules.** A `<style module>` block's CSS loads, but `$style` is
  undefined, so a template that reads it fails to render with
  `Cannot read properties of undefined`.
- **Tailwind and PostCSS.** Workbench doesn't run PostCSS. `@tailwind` and
  `@apply` are dropped without an error, so utility classes have no styles.
  Build your CSS with your own toolchain and list the output file in `styles`.

## Imports, aliases, and environment variables

Scripts are bundled for the browser, along with the packages they import from
your project.

- **Path aliases from `tsconfig.json`.** `compilerOptions.paths`, such as
  `"@/*": ["src/*"]`, apply to imports in `.vue` and TypeScript files.
- **Other aliases.** Vite's `resolve.alias` isn't read. Map import specifiers in
  `workbench.config.ts` at the project root instead. Each entry matches one
  exact specifier and maps it to a package name or a project path, which is
  how you replace a module the preview can't run with a mock.
- **`import.meta.env`.** It isn't defined, so `import.meta.env.VITE_API_URL`
  fails the render with
  `Cannot read properties of undefined (reading 'VITE_API_URL')`. Define each
  value you use.
- **`process.env.NODE_ENV`** is `development` on the canvas and `production` in
  exports.

```ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  aliases: { 'acme-analytics': './preview/analytics-mock.ts' },
  define: { 'import.meta.env.VITE_API_URL': JSON.stringify('https://api.example.com') },
});
```

In a monorepo, add `dedupe: ['vue']` if the preview could load two copies of
Vue; every `vue` import then resolves from the definition's folder. See
[Register other technologies](workbench-previews.md#register-other-technologies)
for all the configuration keys.

Vite's import suffixes aren't supported: `?raw` gives the file's URL, not its
text.

## What .vue files can use

| Supported | Not supported |
| --- | --- |
| `<script setup>` and `<script>`, with `lang="ts"` or plain JavaScript | `lang="tsx"` or `lang="jsx"` scripts: the build fails on the JSX |
| The Options API and `defineComponent` | Prop or emit types imported from another file, as in `defineProps<CardProps>()` with `CardProps` imported. Declare the type in the `.vue` file. |
| `defineProps`, `withDefaults`, and `defineEmits`, typed with types declared in the same file | `<template lang="pug">` and other template languages: the source is shown as text |
| Template-only components | `<style module>`, `<style lang>`, `<style src>` (see [Styles, fonts, and images](#styles-fonts-and-images)) |
| `<style>`, `<style scoped>`, and `v-bind()` in CSS | Custom blocks such as `<i18n>` or `<docs>`: they are ignored |

## Errors and fixes

The [command-line checker](workbench-previews.md#command-line-tools) compiles
every preview, so it reports build errors. Errors that happen while rendering,
such as a missing `$style` or an undefined `import.meta.env`, appear only in
the preview's frame.

| Error | Cause and fix |
| --- | --- |
| `Could not resolve "vue"` | Vue isn't installed where the definition can resolve it. Install `vue` in the project. |
| `Could not resolve "<package>"` | A package the component imports isn't installed, or only exists in your app's build setup. Install it, or map it in `aliases`. |
| ``No fs option provided to `compileScript` in non-Node environment. File system access is required for resolving imported types.`` | `defineProps` or `defineEmits` uses a type imported from another file. Declare the type in the `.vue` file. |
| `Vue style preprocessors need a compiler plugin: <file>` | A `<style>` block has a `lang` or `src` attribute. Use plain CSS, or add a compiler plugin. |
| `No loader is configured for ".otf" files` | CSS or a script references a file type Workbench doesn't copy. See [Styles, fonts, and images](#styles-fonts-and-images). |
| `Element is missing end tag.` and other template errors | The Vue compiler's message for invalid template syntax in the file. |
| `DataCloneError: … could not be cloned` | `inputs`, `fixtures`, or `globals` hold a function or another value that can't be copied. Keep them to plain data. |
| `Cannot read properties of undefined (reading 'VITE_…')` | The code reads `import.meta.env`. Add the value to `define`. |
| `Cannot read properties of undefined (reading '<class>')` | The template reads `$style` from a `<style module>` block, which isn't supported. |
| Clicks don't appear under **Actions** | Add an environment with a `wrap` export, and declare the events with `defineEmits` or `emits`. See [Events and actions](#events-and-actions). |

For problems that apply to every preview, such as a preview missing from the
list, see
[A TypeScript preview is missing or broken](troubleshooting.md#a-typescript-preview-is-missing-or-broken).
