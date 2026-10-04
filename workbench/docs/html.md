# HTML previews

An HTML preview renders an HTML file, or a function that draws into the
canvas, as a [TypeScript preview](workbench-previews.md) with
`adapter: 'html'`. Workbench compiles the file's scripts, stylesheets, and
assets, and gives the page states, inputs, **Preview controls**, and an action
log. No framework is involved.

This guide covers what is specific to HTML. For the definition keys, controls,
lifecycle hooks, and the command-line checker, see
[TypeScript Workbench previews](workbench-previews.md).

## HTML previews or design pages

Workbench shows HTML in two ways. A [design page](pages-and-states.md#design-pages)
is an HTML file listed in `workbench.yaml` and served as it is. An HTML preview
is an HTML file named by a `.workbench.ts` definition and rebuilt by Workbench.

| | Design page | HTML preview |
| --- | --- | --- |
| Declared in | `workbench.yaml`, with its states | A `.workbench.ts` definition next to the file |
| How it's served | The file itself, from the project | Compiled into a new document |
| Scripts | Run as written | Bundled, so they can be TypeScript and import local modules, packages, CSS, and JSON |
| States | `data-wb-*` attributes and CSS; the first state is `default` in the page | The same, plus scripts that read the state and inputs; the state ID is used as it is |
| Inputs, controls, actions | None | `inputs`, **Preview controls**, and **Actions** |
| Relative links | Work as on a web server | Resolve against the compiled preview, not the file (see [Links](#links)) |
| Portable viewer | Not included | Included in `build` output and the export's `browser/` folder |

Use a design page for a mockup or static export that you review as it is,
especially one with links between pages. Use an HTML preview when the page
needs data from inputs, TypeScript or bundled imports, controls, or a place in
the [portable viewer](workbench-previews.md#portable-exports).

## Requirements

Only the general requirements in
[TypeScript Workbench previews](workbench-previews.md): a `workbench.yaml` and a
trusted workspace. Packages that your scripts import must be installed in the
project; Workbench bundles them but never installs them.

## A minimal preview

```text
acme/
├── workbench.yaml
└── site/
    ├── images/
    │   ├── hero.svg
    │   ├── logo.svg
    │   └── logo@2x.svg
    └── pricing/
        ├── pricing.css
        ├── pricing.html
        ├── pricing.ts
        └── pricing.workbench.ts
```

`workbench.yaml` only needs a name:

```yaml
name: Acme
```

`site/pricing/pricing.html`:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Pricing — Acme</title>
  <link rel="stylesheet" href="./pricing.css">
</head>
<body class="marketing">
  <header class="hero">
    <img src="../images/logo.svg" srcset="../images/logo.svg 1x, ../images/logo@2x.svg 2x" alt="Acme">
    <h1>Plans for every team</h1>
  </header>

  <article class="plan">
    <h2 class="plan-name">Team</h2>
    <p class="plan-price">$12.00 / month</p>
    <p class="plan-note">Billed yearly</p>
    <button class="plan-cta" type="button">Choose plan</button>
    <a href="./compare.html">Compare plans</a>
  </article>

  <script type="module" src="./pricing.ts"></script>
</body>
</html>
```

`site/pricing/pricing.css`:

```css
body { font: 16px/1.5 system-ui, sans-serif; }
.hero { padding: 32px; background: url(../images/hero.svg) no-repeat right center / 120px; }
.plan { margin: 0 32px; padding: 24px; max-width: 320px; border: 1px solid #dde3ee; border-radius: 12px; }
.plan-cta { padding: 12px 20px; border: 0; border-radius: 8px; background: #1f5eff; color: white; }

.plan-note { display: none; }
html[data-wb-state="annual"] .plan-note { display: block; }
html[data-wb-state="annual"] .plan { border-color: #1f5eff; }
```

`site/pricing/pricing.ts`:

```ts
import type { PreviewContext } from '@canonic2/workbench';

// Set while Workbench renders the page, and undefined anywhere else.
const preview = (window as Window & { workbench?: PreviewContext }).workbench;

if (preview) {
  const plan = String(preview.inputs.plan);
  const price = Number(preview.inputs.price);
  document.querySelector('.plan-name')!.textContent = plan;
  document.querySelector('.plan-price')!.textContent = '$' + price.toFixed(2) + ' / month';
  document.querySelector('.plan-cta')!.addEventListener('click', () => {
    preview.action('choose-plan', plan);
  });
}
```

`site/pricing/pricing.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'pages/pricing',
  title: 'Pages/Pricing',
  adapter: 'html',
  source: { entry: './pricing.html' },
  viewports: ['desktop', 'mobile'],
  inputs: { plan: 'Team', price: 12 },
  controls: {
    plan: { type: 'select', options: ['Starter', 'Team', 'Enterprise'] },
    price: { type: 'number', min: 0, step: 1 },
  },
  states: {
    monthly: {},
    annual: { inputs: { price: 10 } },
  },
});
```

Run **Workbench: Refresh Pages**. **Pricing** appears in a **Pages** collection
with the states **Monthly** and **Annual**. **Annual** shows $10.00 and the
"Billed yearly" note, choosing a plan in **Preview controls** renders the page
again with that plan, and clicking **Choose plan** records `choose-plan` under
**Actions**. The [command-line checker](workbench-previews.md#command-line-tools)
prints `Built pages/pricing`.

## How Workbench rebuilds the page

Workbench doesn't serve your HTML file. It compiles it and puts its content into
a document of its own:

- `<title>`, `<meta>`, and `<base>` are dropped. The page's title is the
  preview's `title`.
- The attributes of your `<html>` and `<body>` are copied onto the preview's
  `<html>` and `<body>`, so `lang`, classes, and inline styles still apply.
- The rest of `<head>` and the content of `<body>` go, in that order, into a
  `<main id="workbench-preview">` element. Selectors that depend on what is
  directly inside `body`, such as `body > header`, don't match. Rules for
  `html`, `body`, and your own classes do.
- The HTML can be a fragment, with no `<html>`, `<head>`, or `<body>`.

### Stylesheets

`<link rel="stylesheet">` files and `<style>` blocks are compiled with their
`@import` rules, and each relative `url()` is resolved from the file it appears
in and copied with the preview. A `url()` that starts with `/` fails the build
with `Could not resolve`; write it relative to the stylesheet. External
stylesheets keep their URLs.

A `url()` may reference fonts (`.woff`, `.woff2`, `.ttf`), images (`.svg`,
`.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.avif`), or media (`.mp4`, `.mp3`).
Another type fails the build, such as `No loader is configured for ".otf" files`.

### Scripts

Every `<script>` is compiled and bundled with what it imports: local modules,
installed packages, CSS (added to the page), JSON (as data), and the file types
above (as URLs).

- A `<script src>` file can be TypeScript. An inline `<script>` is JavaScript
  only; move TypeScript into a file.
- Each script is wrapped so that its top-level declarations stay private. An
  inline handler such as `onclick="hello()"` can't see a `function hello()`
  declared in a script, and fails with `ReferenceError: hello is not defined`.
  Add listeners from the script, or assign the function to `window`.
- `<script type="application/json">` and `application/ld+json` blocks are kept
  as they are, so a script can read fixture data from them.
- Stylesheets load first. Then the scripts run one at a time, in document order,
  after all of the markup is in place. `DOMContentLoaded` has already fired, so
  run your code directly instead of waiting for it or for `load`.

### Images and other files

Relative paths in `src`, `poster`, and `srcset`, and in `href` on `<link>`
elements, are resolved from the HTML file and copied with the preview. A path
that starts with `/` is resolved from the project root. A URL with a scheme,
such as `https:` or `data:`, is kept as it is. A local file that doesn't exist
fails the build with `Missing HTML asset: <path> in <file>`.

These aren't rewritten, so a relative path in them doesn't load:

- an attribute value without quotes, such as `src=logo.svg`,
- a `url()` inside a `style="..."` attribute; move it into a stylesheet,
- `href` on `<a>` and other elements that aren't `<link>`.

### Links

A relative link resolves against the compiled preview's folder, not your HTML
file's folder, so it doesn't reach the page next to your file. A link that
starts with `/` resolves from the project root. Links don't add pages to
Workbench: write a preview for each page you want, and map the addresses your
page links to in the definition's `links`. With the top bar's **Actions**
switch on, a mapped
link opens its preview, and any other link, such as **Compare plans** without a
`pages/compare` preview, is recorded under **Actions** as `navigate`. See
[Links and navigation](preview-data.md#links-and-navigation).

## Inputs, states, and actions

While a state renders, `window.workbench` holds the preview's context: `id`,
`state`, `inputs`, `fixtures`, `globals`, `signal`, `action(name, ...values)`,
`navigate(to)`, and `error(error)`. Your HTML doesn't receive inputs any other
way; a script reads them and updates the page, as `pricing.ts` does. Outside
Workbench, `window.workbench` is undefined.

A page whose scripts load their data with `fetch` or XMLHttpRequest needs no
changes: answer those requests per state with
[`requests`](preview-data.md#request-mocks).

Every state change, input edit, and **Reset state** renders the page again from
the start: Workbench removes the content, restores the `<html>` and `<body>`
attributes, inserts the content again, and runs every script again, modules
included. The window itself is kept, so globals your scripts set and listeners
they add to `window` or `document` survive into the next render. Tie those
listeners to the render's signal:

```js
document.addEventListener('keydown', onKey, { signal: window.workbench?.signal });
```

States:

- `<html>` carries `data-wb-state` with the state's ID. The first state uses its
  own ID, such as `monthly`, not `default` as on design pages. CSS keyed off it,
  as in `pricing.css`, works on the canvas and in exports.
- `data-wb-state-only`, `data-wb-state-not`, and `data-wb-set-<id>`, described
  in [States](pages-and-states.md#states), are applied on the canvas before your
  scripts run. They aren't applied in the
  [portable viewer](workbench-previews.md#portable-exports), so use CSS or a
  script when the preview must look the same there.

Call `window.workbench.action(name, ...values)` to add an entry under
**Actions**.

## Render from a function

`source.entry` can also be a TypeScript or JavaScript module that exports a
function. Workbench calls it with the canvas element and the context, and it
may return a cleanup function. Use this for a component that isn't a page.

`site/components/banner.ts`:

```ts
import type { PreviewContext } from '@canonic2/workbench';
import './banner.css';

export function mountBanner(canvas: HTMLElement, context: PreviewContext) {
  const banner = document.createElement('div');
  banner.className = 'banner ' + String(context.inputs.tone);
  banner.textContent = String(context.inputs.message);

  const close = document.createElement('button');
  close.type = 'button';
  close.textContent = 'Dismiss';
  close.addEventListener('click', () => context.action('dismiss'), { signal: context.signal });
  banner.append(close);
  canvas.append(banner);

  // Anything outside the canvas needs its own cleanup.
  const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') context.action('dismiss'); };
  document.addEventListener('keydown', onKey);
  return () => document.removeEventListener('keydown', onKey);
}
```

`site/components/banner.css`:

```css
.banner { display: flex; gap: 12px; align-items: center; padding: 12px 16px; border-radius: 8px; font: 15px system-ui, sans-serif; }
.banner.info { background: rgb(232, 237, 247); }
.banner.warning { background: rgb(255, 240, 200); }
```

`site/components/banner.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/banner',
  title: 'Components/Banner',
  adapter: 'html',
  source: { entry: './banner.ts', export: 'mountBanner' },
  viewports: ['fit'],
  inputs: { message: 'Your trial ends in 3 days.', tone: 'info' },
  controls: {
    message: { type: 'text' },
    tone: { type: 'select', options: ['info', 'warning'] },
  },
  states: {
    info: {},
    warning: { inputs: { tone: 'warning', message: 'Payment failed.' } },
  },
});
```

Workbench empties the canvas before each render, so you only clean up what you
added elsewhere. If the named export isn't a function, the preview shows
`An HTML JavaScript entry must export a mount function.`

## Environment mount

An `environment` module can export `mount(canvas, context)`. For an HTML file,
Workbench calls it after the content is in place and the page's scripts have
run, and runs the cleanup it returns before the next render. It isn't called
for a [function source](#render-from-a-function). The environment can also
export `setup` and `ready`; see
[Lifecycle hooks](workbench-previews.md#lifecycle-hooks).

`site/pricing/pricing-environment.ts` makes **Choose plan**, a button rather
than a link, open a checkout preview:

```ts
import type { PreviewContext } from '@canonic2/workbench';

// Runs after the HTML is in place and its scripts have run.
export function mount(canvas: HTMLElement, context: PreviewContext) {
  canvas.querySelector('.plan-cta')?.addEventListener('click', () => {
    context.navigate('pages/checkout');
  }, { signal: context.signal });
}
```

Add it to the definition, with a path relative to the definition:

```ts
  source: { entry: './pricing.html' },
  environment: './pricing-environment.ts',
```

With a `pages/checkout` preview in the project and **Actions** on, clicking
**Choose plan** opens it. `pricing.ts` still records `choose-plan` first.

## Document components with a docs page

A [docs page](docs-pages.md) can show HTML components as live examples. Give
it an `examples` lens with `adapter: html`. Each example is a function
`(canvas, context)` that draws into `canvas`, like a
[function source](#render-from-a-function), and may return a cleanup function.
HTML files aren't examples.

```yaml
implementations:
  web:
    kind: examples
    label: Web
    adapter: html
    styles:
      - site/components/banner.css

collections:
  - name: Components
    items:
      - label: Banner
        src: docs/banner.md
        implementations:
          web: site/components/banner.examples.ts
```

`site/components/banner.examples.ts` holds two examples, `info` and `warning`:

```ts
import type { PreviewContext } from '@canonic2/workbench';

function banner(canvas: HTMLElement, tone: string, message: string) {
  const element = document.createElement('div');
  element.className = 'banner ' + tone;
  element.textContent = message;
  canvas.append(element);
  return element;
}

export function info(canvas: HTMLElement) {
  banner(canvas, 'info', 'Your trial ends in 3 days.');
}

export function warning(canvas: HTMLElement, context: PreviewContext) {
  const element = banner(canvas, 'warning', 'Payment failed.');
  const close = document.createElement('button');
  close.type = 'button';
  close.textContent = 'Dismiss';
  close.addEventListener('click', () => context.action('dismiss'), { signal: context.signal });
  element.append(close);
}
```

`docs/banner.md` places each one with a fenced block, such as
```` ```example warning ````; see
[Docs pages](docs-pages.md#write-the-markdown). A folder works too, with one
`.ts` or `.js` file per example and the function as its default export.

Examples get no inputs or controls, so set what each one shows in its code.
The lens's `styles` load with the examples, and an `environment`'s `setup` and
`ready` run once for the page. Its `mount` isn't called, as for a function
source.

## Errors and fixes

The [command-line checker](workbench-previews.md#command-line-tools) reports
build errors. Errors that happen while the page renders, such as a mistyped
`export` name or a failing script, appear only in the preview's frame. An
uncaught error in a script or an event handler replaces the page with the error;
fix it and save, or use **Reset state**.

| Error | Cause and fix |
| --- | --- |
| `Missing HTML asset: <path> in <file>` | A `src`, `srcset`, `poster`, or `<link href>` names a local file that doesn't exist, or one outside the project. Fix the path. |
| `Expected ";" but found ":"` and other syntax errors in an inline script | The inline script contains TypeScript. Move it to a `.ts` file and load it with `<script src>`. |
| `Could not resolve "/…"` | A stylesheet's `url()` starts with `/`. Make it relative to the stylesheet. |
| `No loader is configured for "<extension>" files` | A stylesheet or script references a file type Workbench doesn't copy. See [Stylesheets](#stylesheets). |
| `ReferenceError: <name> is not defined` from an `onclick` or other inline handler | Script declarations are private. Add the listener from the script, or assign the function to `window`. |
| `The selected source export does not exist for <id> — <state>` | `source.export` names an export the module doesn't have. |
| `An HTML JavaScript entry must export a mount function.` | The module's selected export isn't a function. |
| `Could not load HTML resource: <url>` or `HTML resource load timed out: <url>` | A stylesheet or script failed to load, or took more than 5 seconds. Check the URL, especially an external one. |
| An image or background is missing, with no error | The path is in an unquoted attribute or a `style` attribute. See [Images and other files](#images-and-other-files). |
| Code in a `DOMContentLoaded` listener never runs | The event has already fired. Run the code directly. |

For problems that apply to every preview, such as a preview missing from the
list, see
[A TypeScript preview is missing or broken](troubleshooting.md#a-typescript-preview-is-missing-or-broken).
