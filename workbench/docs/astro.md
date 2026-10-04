# Astro previews

The `astro` adapter renders an Astro component or page on the server, with your
preview's inputs as `Astro.props`, and shows the resulting HTML, styles, and
scripts on the canvas. Use it to review components and pages in named states
next to their designs, without running your site.

The adapter renders one component on its own, not your application: no
`astro.config.*`, integrations, middleware, or routing. To review the running
site, use a [URL lens](lenses.md) instead.

For what every preview shares, such as the definition keys, controls, hooks, and
the command-line checker, see [TypeScript Workbench previews](workbench-previews.md).

## Requirements

- Astro 4.9 or later, installed where the component can resolve it: in the
  `node_modules` of its package or any folder above it. Workbench uses that
  installation's compiler and its Container API, and never installs Astro.
- A `workbench.yaml` at the project root, and a
  [trusted workspace](extension.md#workspace-trust).

## Add a component preview

This example previews a pricing card in three states.

```text
acme-site/
├── package.json
├── workbench.yaml
└── src/
    ├── assets/check.svg
    └── components/
        ├── Badge.astro
        ├── PlanCard.astro
        ├── PlanCard.workbench.ts
        └── plan-card.css
```

`src/components/PlanCard.astro`:

```astro
---
import Badge from './Badge.astro';
import check from '../assets/check.svg?url';
import './plan-card.css';

interface Props {
  name: string;
  price: string;
  featured?: boolean;
}

const { name, price, featured = false } = Astro.props;
---

<article class:list={['plan', { featured }]}>
  {featured && <Badge>Most popular</Badge>}
  <h2>{name}</h2>
  <p class="price">{price}</p>
  <ul>
    <slot>
      <li><img src={check} alt="" /> Unlimited projects</li>
    </slot>
  </ul>
  <button type="button">Choose {name}</button>
</article>

<style>
  .plan { border: 1px solid #d0d5dd; border-radius: 12px; padding: 24px; }
  .featured { border-color: #4f46e5; }
</style>
```

`src/components/Badge.astro`:

```astro
<span class="badge"><slot /></span>

<style>
  .badge { background: #eef2ff; color: #4338ca; border-radius: 999px; padding: 2px 8px; }
</style>
```

`src/components/plan-card.css`:

```css
.plan h2 {
  font: 600 20px/1.2 system-ui, sans-serif;
}
```

`src/components/PlanCard.workbench.ts`:

```ts
import { definePreview } from '@canonic/workbench';

export default definePreview({
  id: 'components/plan-card',
  title: 'Components/Plan Card',
  adapter: 'astro',
  source: { entry: './PlanCard.astro' },
  inputs: { name: 'Team', price: '$12 per month', featured: false },
  controls: {
    name: { type: 'text' },
    price: { type: 'text' },
    featured: { type: 'boolean' },
  },
  states: {
    default: {},
    featured: { inputs: { featured: true } },
    enterprise: { inputs: { name: 'Enterprise', price: 'Contact sales' } },
  },
});
```

Run **Workbench: Refresh Screens**. **Plan Card** appears in a **Components**
section with **Default**, **Featured**, and **Enterprise** states. To check it
without the canvas, run the
[checker](workbench-previews.md#command-line-tools) on the project:

```text
$ node <extension>/preview/cli.cjs check acme-site
Built components/plan-card
```

## Preview a page

A page is a component like any other. Pass the data it would load as inputs:

```astro
---
// src/pages/pricing.astro
import Base from '../layouts/Base.astro';
import PlanCard from '../components/PlanCard.astro';

const { plans = [] } = Astro.props;
---

<Base title="Pricing">
  <main>
    <h1>Pricing</h1>
    {plans.map((plan) => <PlanCard {...plan} />)}
  </main>
</Base>
```

```ts
// src/pages/pricing.workbench.ts
import { definePreview } from '@canonic/workbench';

export default definePreview({
  id: 'pages/pricing',
  title: 'Pages/Pricing',
  adapter: 'astro',
  source: { entry: './pricing.astro' },
  viewports: ['desktop', 'mobile'],
  inputs: {
    plans: [
      { name: 'Team', price: '$12 per month' },
      { name: 'Business', price: '$24 per month', featured: true },
    ],
  },
  controls: { plans: { type: 'json' } },
  states: {
    default: {},
    empty: { inputs: { plans: [] } },
  },
});
```

When a layout renders `<html>`, `<head>`, and `<body>`, their attributes, such
as `lang` and a `class` on `<body>`, are kept. The page's `<title>`, `<meta>`,
and `<base>` are dropped; the frame's title is the preview's `title`.

A page in `src/pages/` is not routed. `Astro.params` is `{}`, and
`getStaticPaths` isn't called, so a dynamic route such as `[slug].astro` should
read what it needs from props in the preview.

When the frontmatter loads data with `fetch`, answer it with
[`requests`](preview-data.md#request-mocks); the mocks apply while the
page renders in Node as well as to scripts in the frame:

```ts
  requests: { 'GET /api/plans': { body: plans } },
  states: {
    default: {},
    empty: { requests: { 'GET /api/plans': { body: [] } } },
  },
```

Relative URLs resolve against `http://localhost/` there. The page waits for
every fetch before it renders, so `pending` and long `delay` values have no
loading state to show: a render that waits more than 10 seconds fails.

Since the page isn't routed, its links to your other routes have nowhere to go.
Map each route that has a preview of its own, and the navigation in a shared
layout opens it with **Actions** on:

```ts
  links: {
    '/': 'pages/home',
    '/pricing/': 'pages/pricing',
  },
```

Other links, including downloads and other sites, are recorded under
**Actions**. See [Links and navigation](preview-data.md#links-and-navigation).

## What runs where

- **In Workbench's preview worker (Node):** the frontmatter of every component,
  and whatever it imports, including Node modules such as `node:fs`. Workbench
  renders each state there when it builds the preview. Frontmatter is never
  sent to the browser.
- **In the canvas:** the rendered HTML, its styles, its `<script>` tags, and
  your preview's `setup`, `play`, and `ready` hooks.

Because the frontmatter runs once per state at build time, a frontmatter error
in any state fails the whole preview, not just that state. Frontmatter that
calls an external service needs that service to be reachable.

## Props, slots, and states

- **Props:** the preview's `inputs`, merged with the state's `inputs`, become
  `Astro.props`. They must be plain data. `fixtures` and `globals` don't reach
  the frontmatter; they are only available to hooks in the browser.
- **Slots:** nested components fill each other's slots as usual. The preview
  itself renders with no slot content, so a component's own `<slot>` shows its
  fallback. To preview it with content, write a small example component and use
  it as a state's `source`:

  ```astro
  ---
  // src/components/PlanCard.example.astro
  import PlanCard from './PlanCard.astro';
  ---

  <PlanCard name="Team" price="$12 per month">
    <li>Priority support</li>
  </PlanCard>
  ```

  ```ts
  states: {
    default: {},
    'custom-features': { source: { entry: './PlanCard.example.astro' } },
  },
  ```

- **Exports:** `source.export` selects a named export of the entry, so an
  `index.ts` that re-exports components works with
  `source: { entry: './index.ts', export: 'PlanCard' }`.

### Edit inputs

Editing a field in **Preview controls** sends the new inputs to the preview
worker, which renders that state again and replaces the frame's content. Each
edit is a full server render, so frontmatter runs again. **Reset state** returns
to the authored render. Edits never change your files.

## Styles, scripts, and assets

| In the component | In the preview |
| --- | --- |
| `<style>` and `<style is:global>` | Compiled and loaded, with Astro's scoping |
| `import './file.css'` in frontmatter | Loaded, with its `url()` assets |
| `<script>` | Bundled with its imports and run in the frame |
| `<script is:inline>` | Run in the frame |
| `import url from './logo.svg?url'` | A URL to the copied file |
| `import text from './icon.svg?raw'` | The file's text |
| `import logo from './logo.png'` | A URL string, not Astro's image object |
| `<img src="/logo.svg">` | Found in the project root, then in the nearest package's `public/` folder |

Scripts run again on every state change, input edit, and reset, and can read
the current context from `window.workbench`. For example, to log a click in
**Actions**:

```astro
<script>
  for (const button of document.querySelectorAll('.plan button')) {
    button.addEventListener('click', () => window.workbench?.action('choose', button.textContent));
  }
</script>
```

On your site, `window.workbench` is undefined, so the optional call does nothing.

Imported images are URL strings in a preview, so `logo.src` is undefined there.
Import images with `?url` when a component needs to work in both, as the
example does. Frontmatter can import images (`.svg`, `.png`, `.jpg`, `.jpeg`,
`.gif`, `.webp`, `.avif`) and fonts (`.woff`, `.woff2`, `.ttf`); other file
types need a `?url` or `?raw` import.

The definition's `styles`, `environment`, and hooks work as they do for the
[HTML adapter](html.md): an `environment` can export
`mount(canvas, context)`, called after the rendered HTML is in place.

## Environment values

| Value | In a preview |
| --- | --- |
| `import.meta.env.DEV` | `true` on the canvas, `false` in `check`, `build`, and exports |
| `import.meta.env.PROD` | The opposite of `DEV` |
| `import.meta.env.BASE_URL` | `/` |
| `import.meta.env.SITE` | `https://example.com` |
| Other `import.meta.env` values, such as `PUBLIC_API_URL` or `MODE` | Not set. Reading one fails with `Cannot read properties of undefined` |
| `Astro.url` | `https://example.com/` |
| `Astro.site` | `undefined` |
| `Astro.params`, `Astro.locals` | `{}` |

`.env` files aren't read. Set the values you need with `define` in
`workbench.config.ts`:

```ts
import { defineConfig } from '@canonic/workbench';

export default defineConfig({
  define: {
    'import.meta.env.SITE': JSON.stringify('https://example.com/docs/'),
    'import.meta.env.PUBLIC_API_URL': JSON.stringify('https://api.example.com'),
  },
});
```

## Imports, aliases, and plugins

- **`tsconfig.json` paths,** such as `@/*` mapped to `src/*`, resolve in
  frontmatter and in scripts.
- **`aliases`** in `workbench.config.ts` also apply to frontmatter. Each alias
  matches one exact import specifier. See
  [Custom adapters](custom-adapters.md#aliases).
- **Global `plugins`** in `workbench.config.ts` run in the frontmatter build as
  well as in the browser. Use one to stand in for a module that needs Astro's
  application pipeline. This plugin answers `astro:content` with a local
  fixture:

  ```ts
  // workbench.config.ts
  import path from 'node:path';
  import { defineConfig } from '@canonic/workbench';

  const content = {
    name: 'acme-content',
    setup(build) {
      build.onResolve({ filter: /^astro:content$/ }, () => ({
        path: path.join(__dirname, 'preview/content.ts'),
      }));
    },
  };

  export default defineConfig({ plugins: [content] });
  ```

  ```ts
  // preview/content.ts
  export async function getCollection(name: string) {
    return [{ id: 'launch', data: { title: 'Acme launch' } }];
  }
  ```

  A component that calls `getCollection('blog')` then renders the fixture.
  `aliases` can't do this: `astro:` imports are rejected before aliases apply.

Registering an adapter named `astro` in `workbench.config.ts` replaces this
adapter entirely, including server rendering. See
[Replace a built-in adapter](custom-adapters.md#replace-a-built-in-adapter).

## Not supported

| Feature | Error | What to do |
| --- | --- | --- |
| `astro:content`, `astro:assets`, and other `astro:` modules | `astro:assets needs the Astro application pipeline; use a URL lens or a compiler plugin.` | Stand in for the module with a global plugin, as above, or use a URL lens |
| `client:*` directives and `server:defer` | `Astro client/server islands need application integrations; use a URL lens for <file>` | Use a URL lens |
| React, Vue, Svelte, or other framework components inside Astro | ``Unable to render `Widget`. No valid renderer was found for this file extension.`` | Preview the framework component with its own adapter, or use a URL lens |
| `<style lang="scss">` and other preprocessors | `Astro style preprocessors need a compiler plugin: <file>` | Use plain CSS, or a URL lens. A plugin that returns CSS doesn't fix this |
| Importing `.scss`, `.md`, or `.mdx` files | `No loader is configured for ".scss" files: <file>` | Use plain CSS, or pass the content as an input |
| `import.meta.glob` | `(intermediate value).glob is not a function` | Import the files directly |
| `astro.config.*`, integrations, middleware, and `.env` files | None: they are not loaded | Use `define`, `aliases`, and `plugins` in `workbench.config.ts` |

## Portable exports

`build` and **Download design-system ZIP** render every authored state when
they export the preview, so the [portable viewer](workbench-previews.md#portable-exports)
shows them with no Astro installation or server. The controls are left out,
since editing inputs needs a live render, and the preview's **Documentation**
gains the note `Astro input edits require the live Workbench server. This export
contains the authored states.`

## Errors and fixes

| Error | Fix |
| --- | --- |
| `The Astro adapter needs astro installed in the project.` | Install `astro` in the package that contains the component, or in a folder above it. |
| `Astro previews require Astro 4.9 or newer (the Container API).` | Upgrade Astro to 4.9 or later. |
| `Cannot load the project Astro compiler: …` | Reinstall your dependencies so Astro's compiler package is present. |
| `The selected Astro source export does not exist: <name>` | Fix `source.export`, or remove it to use the default export. |
| `Missing HTML asset: <url> in <file>` | A rendered `src`, `poster`, `srcset`, or `<link href>` names a local file that doesn't exist. Attributes such as `data-src` are checked too, so `data-src="undefined"` fails this way. |
| `Cannot read properties of undefined (reading '<name>')` | If `<name>` is an `import.meta.env` value, set it with `define`. Otherwise a prop is missing: add it to `inputs`. |

For the other problems listed under `Workbench previews:`, see
[A TypeScript preview is missing or broken](troubleshooting.md#a-typescript-preview-is-missing-or-broken).
