# Preview data, mocks, and actions

A [Workbench preview](workbench-previews.md) renders your real component or
page, but not your app: there is no server, no router, no API, and nobody
signed in. This guide covers everything a preview supplies in their place, and
how to change it for each state and while you review:

- **[Inputs and controls](#inputs-and-controls)**: the props your component
  receives, and the fields that edit them live.
- **[Fixtures and globals](#fixtures-and-globals)**: data and settings for the
  code around your component.
- **[Environments](#environments)**: providers, plugins, stores, and setup,
  for one preview or for the whole project.
- **[Request mocks](#request-mocks)**: answers to the requests your page makes
  itself.
- **[Actions](#actions)**: a log of what your page tried to do.
- **[Links and navigation](#links-and-navigation)**: where links and forms go,
  and how one preview leads to another.

[Recipes](#recipes) at the end put them together for common kinds of pages.

## Choose how to give a page its data

Supply data the way your page already receives it, so the component renders
unchanged. The options combine freely, and each can change per state.

| The page gets its data from | Give it | With |
| --- | --- | --- |
| Props, or `Astro.props` | Values for those props | [`inputs`](#inputs-and-controls) |
| A context, store, or provider | A provider holding fixture data, or a seeded store | An [environment](#environments), reading [`fixtures`](#fixtures-and-globals) |
| Its own `fetch` or XMLHttpRequest, or a client built on them, such as Apollo Client, axios, React Query, or SWR | Answers to those requests | [`requests`](#request-mocks) |
| Browser storage, such as a saved session | Values written before it mounts | A [`setup` hook](#seed-storage-and-stores) |
| A module that can't run in a preview, such as analytics or a native API | A stand-in module | [`aliases`](custom-adapters.md#aliases) in `workbench.config.ts` |

## What you can set, and where

| Key | Set on the preview | Overridden by a state | Edited while you review | Reaches |
| --- | --- | --- | --- | --- |
| `inputs` | Yes | Yes, key by key | Yes, through `controls` | The component: props, `Astro.props`, or `window.workbench.inputs` |
| `fixtures`, `globals` | Yes | Yes, key by key | No | `context` in environments, hooks, and request handlers |
| `requests` | Yes | Yes, tried first | No | The page's `fetch` and XMLHttpRequest calls |
| `source` | Yes | Yes | No | Which file and export renders |
| `environment` | Yes, and once for the project | No; read `context.state` | No | Providers, plugins, and setup around the component |
| `setup`, `play`, `ready` | Yes | Yes, both run | No | Code before and after the component mounts |
| `links` | Yes | No | No | Where the page's links and forms go |

A state overrides a preview **key by key, one level deep**. If the preview has
`inputs: { user: { name: 'Ada', role: 'admin' }, compact: false }` and a state
has `inputs: { user: { name: 'Grace' } }`, the state's `user` replaces the whole
`user` object, so `role` is gone, while `compact` keeps the preview's value.
Repeat the fields a state still needs, or spread a shared object:

```ts
const ada = { name: 'Ada', role: 'admin' };

export default definePreview({
  // …
  inputs: { user: ada, compact: false },
  states: {
    default: {},
    viewer: { inputs: { user: { ...ada, role: 'viewer' } } },
  },
});
```

## How a render runs

Workbench renders the state again whenever you pick a state, edit a control,
or choose **Reset state**. Each render:

1. Merges `inputs`, `fixtures`, and `globals`: the preview's, then the
   state's, then, for inputs, your control edits. Each render gets a fresh
   copy, so code can't change the authored values.
2. Turns on [request mocks](#request-mocks): the state's, then the preview's.
3. Runs `setup` hooks: the project's environment, the preview's environment,
   the preview, then the state.
4. Mounts the component with the inputs, inside the environments' providers.
5. Runs `play` hooks (the preview's, then the state's) and `ready` hooks (the
   environments', the preview's, then the state's).
6. Waits for fonts and visible images, then marks the preview ready.
   Screenshots and handoffs wait for this.

Before the next render, Workbench aborts `context.signal`, which also cancels
any mocked request still waiting, and runs cleanups in reverse order. See
[Lifecycle hooks](workbench-previews.md#lifecycle-hooks) for timing and errors.

## Inputs and controls

`inputs` are the values your component receives:

| Adapter | Inputs arrive as |
| --- | --- |
| `react`, `react-native-web` | Props |
| `vue` | Props, and attributes for anything the component doesn't declare |
| `astro` | `Astro.props` |
| `html` | `window.workbench.inputs`, read by the page's scripts, or the `context` passed to a [function source](html.md#render-from-a-function) |
| Custom | However the [runtime](custom-adapters.md#the-runtime-module) passes them |

Inputs must be plain data: strings, numbers, booleans, `null`, arrays, and
objects of those. For functions, elements, and slots, see
[Callbacks, children, and slots](#callbacks-children-and-slots).

### Controls

`controls` turns inputs into fields under **Preview controls** in the top bar.
Each key names an input:

```ts
inputs: { label: 'Continue', size: 'md', count: 3, disabled: false, items: ['Ada', 'Grace'] },
controls: {
  label: { type: 'text' },
  size: { type: 'select', options: ['sm', 'md', 'lg'] },
  count: { type: 'number', min: 0, max: 10, step: 1, label: 'Item count' },
  disabled: { type: 'boolean' },
  items: { type: 'json' },
},
```

| `type` | Field | Options |
| --- | --- | --- |
| `text` | Text box | |
| `number` | Number box | `min`, `max`, `step` |
| `boolean` | Checkbox | |
| `select` | Menu | `options`, a nonempty list of strings or numbers |
| `json` | Text area of JSON | |

Every control also takes a `label`, which defaults to the input's name.

- An edit applies when the field changes, and renders the current state again
  with that input replaced. A `json` edit replaces the whole value.
- Edits last until you choose **Reset state**, pick another state, or leave
  the page. They never change the definition, exports, or the screenshots
  an export takes of each state.
- An Astro preview renders each edit on the Workbench server. A
  [portable export](workbench-previews.md#portable-exports) of an Astro
  preview has its authored states but no controls.

### Callbacks, children, and slots

Inputs can't hold functions or elements. To give a component a callback, an
element, or a slot, wrap it in an [environment](#environments) or render it
through a preview-only file:

- **React:** add props in the environment's `wrap` with `cloneElement`, or
  point `source` at a small wrapper component in a preview-only file. See
  [Props, children, and callbacks](react.md#props-children-and-callbacks).
- **Vue:** pass listeners and slots from the environment's `wrap`. See
  [Events and actions](vue.md#events-and-actions) and [Slots](vue.md#slots).
- **Astro:** see [Props, slots, and states](astro.md#props-slots-and-states).
- **HTML:** attach listeners in a page script or an
  [environment `mount`](html.md#environment-mount).

## Fixtures and globals

`fixtures` and `globals` are plain data for the code around your component:
environments, hooks, and request handlers read them from `context`. The
component doesn't receive them unless that code passes them on. By convention,
`fixtures` hold records, such as customers or an order, and `globals` hold
settings, such as a theme or a locale.

They merge like inputs, key by key, and controls can't edit them. A state that
changes only its data can change only `fixtures`:

```ts
export default definePreview({
  id: 'pages/customers',
  adapter: 'react',
  source: { entry: './CustomersPage.tsx' },
  fixtures: { customers: [{ id: 'c-1', name: 'Ada' }, { id: 'c-2', name: 'Grace' }] },
  requests: {
    'GET /api/customers': (request, context) => ({ body: context.fixtures.customers }),
  },
  states: {
    default: {},
    empty: { fixtures: { customers: [] } },
  },
});
```

An Astro component doesn't see `fixtures` or `globals` in `Astro.props`; pass
what it needs as `inputs`. Request handlers that answer its frontmatter do
see them.

## Environments

An environment is a module that runs around the component on every render. Use
it for providers, plugins, stores, and anything else a page expects the app
to have set up. There are two kinds, and a preview can use both:

- **A preview's environment:** `environment` in the definition, relative to
  the definition. Several definitions can name the same module.
- **The project's environment:** `environment` in `workbench.config.ts`,
  relative to the project root. It applies to every preview, like global
  decorators in Storybook.

```ts
// workbench.config.ts
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({ environment: './src/workbench/environment.tsx' });
```

Environment exports are specific to an adapter: a React `wrap` can't wrap a
Vue component. When your previews use more than one adapter, give each its
own project environment, keyed by adapter name. Previews whose adapter isn't
listed get none:

```ts
export default defineConfig({
  environment: {
    react: './src/workbench/react-environment.tsx',
    vue: './src/workbench/vue-environment.ts',
  },
});
```

An environment can export:

| Export | Adapters | Runs |
| --- | --- | --- |
| `setup(context)` | All | Before the component mounts. May return a cleanup function. |
| `wrap(element, context)` | `react`, `react-native-web` | Returns the element wrapped in providers. |
| `wrap(vnode, context)` | `vue` | Returns the vnode wrapped in other components. |
| `configure(app, context)` | `vue` | Before mount, to install plugins and `provide` values. May be async. |
| `mount(canvas, context)` | `html`, `astro` | After the HTML is in place and its scripts have run. May return a cleanup function. |
| `ready(context)` | All | After the component mounts, for anything the preview must wait for. |
| Anything else | Custom adapters | Whatever the [runtime](custom-adapters.md#the-runtime-module) reads. |

When a preview has both, the project's environment goes on the outside: its
`setup`, `configure`, `mount`, and `ready` run first, its cleanups run last,
and its `wrap` receives what the preview's `wrap` returned. For exports a
custom adapter defines itself, the preview's environment wins.

An environment runs on every render, so read the current state from
`context`, never from a value saved when the module loaded. This project
environment gives every React preview a theme and a signed-in user, both
changeable per state:

```tsx
// src/workbench/environment.tsx
import type { ReactElement } from 'react';
import type { PreviewContext } from '@canonic2/workbench';
import { ThemeProvider } from '../theme';
import { SessionContext } from '../session';

const ada = { id: 'u-1', name: 'Ada Lovelace', email: 'ada@example.com' };

export function wrap(element: ReactElement, context: PreviewContext) {
  const user = 'user' in context.fixtures ? context.fixtures.user : ada;
  return (
    <ThemeProvider theme={context.globals.theme === 'dark' ? 'dark' : 'light'}>
      <SessionContext.Provider value={{ user }}>{element}</SessionContext.Provider>
    </ThemeProvider>
  );
}
```

A state then shows a signed-out page with `fixtures: { user: null }`, or the
dark theme with `globals: { theme: 'dark' }`.

### Seed storage and stores

`setup` runs before the component mounts, so it can put things where the
page will look for them. Return a cleanup that puts them back, so the next
state starts clean:

```ts
export function setup(context: PreviewContext) {
  const previous = localStorage.getItem('acme.session');
  localStorage.setItem('acme.session', JSON.stringify({ email: 'ada@example.com' }));
  return () => {
    if (previous === null) localStorage.removeItem('acme.session');
    else localStorage.setItem('acme.session', previous);
  };
}
```

The same pattern seeds a global store: set its state in `setup` from
`context.fixtures`, and restore it in the cleanup. For a Vue store installed
as a plugin, create it in `configure` instead; Workbench creates a new app on
every render.

### Examples on docs pages

The examples on a [docs page](docs-pages.md) render through environments too:
the project's environment for the lens's adapter, with the lens's own
[`environment`](docs-pages.md#lenses) inside it. Exports such as `wrap`,
`configure`, and `mount` run for each example. `setup` runs once for the page
before the first example mounts, and `ready` once after the last.

An example's context has `state`, `signal`, `action`, and `navigate`, but its
`inputs`, `fixtures`, and `globals` are empty, so an environment that reads
them gets its defaults on a docs page. Docs pages have no request mocks or
controls.

## Request mocks

`requests` answers your page's own `fetch` and XMLHttpRequest calls, so a
page that loads its data renders unchanged, and each state shows the data,
emptiness, delay, or failure you choose:

```ts
export default definePreview({
  id: 'pages/customers',
  adapter: 'react',
  source: { entry: './CustomersPage.tsx' },
  requests: {
    'GET /api/customers': { body: [{ id: 'c-1', name: 'Ada' }] },
    'POST /api/customers': (request) => ({ status: 201, body: { id: 'c-2', ...request.body } }),
  },
  states: {
    default: {},
    empty: { requests: { 'GET /api/customers': { body: [] } } },
    loading: { requests: { 'GET /api/customers': { pending: true } } },
    'server-error': { requests: { 'GET /api/customers': { status: 500 } } },
    offline: { requests: { 'GET /api/customers': { failed: true } } },
  },
});
```

Mocks work with every adapter. Workbench installs them before any of your code
loads, so a client that keeps a reference to `fetch` from the start, as many
data clients do, is answered too. They don't depend on the **Actions** switch.

### Keys

Each key says which requests it answers:

| Key | Answers |
| --- | --- |
| `'GET /api/customers'` | `GET` requests to that path, with any query string, on any host |
| `'/api/customers'` | Any method to that path |
| `'/api/customers/*'` | Any path that starts `/api/customers/`; `*` matches any characters, including `/` |
| `'GET /search?q=acme'` | Requests whose query string includes `q=acme`, whatever else it has |
| `'POST /graphql Customers'` | The GraphQL operation named `Customers`; see [GraphQL](#graphql) |
| `'GET https://api.example.com/me'` | That path on that host only |

A path starts with `/`. The method is optional and written in capitals. A
path without a host matches requests to any host, so the same key answers
`/api/customers` on your page's own address and `https://api.example.com/api/customers`.

### Which mock answers

Workbench tries the state's mocks first, then the preview's, each in the order
you wrote them, and the first match answers. A state's key doesn't have to
repeat the preview's to win: a state's `'/api/*'` answers every request under
`/api/`, ahead of all of the preview's mocks. Put specific keys before broad
ones.

Once any mock applies to a preview, a request that matches none answers `404`,
with a JSON body that names it, such as
`{ "error": "No Workbench request mock for GET /api/invoices" }`. A preview
with mocks therefore never reaches a live service by accident. A preview with
no mocks at all leaves every request alone.

Requests to Workbench itself, under `/_workbench/` on the preview's address,
are never mocked.

### Responses

A mock's value is a response:

| Key | Description |
| --- | --- |
| `status` | The HTTP status. Defaults to `200`. |
| `statusText` | The status text. |
| `headers` | Response headers, such as `{ 'x-total-count': '42' }`. |
| `body` | The body. A string is sent as it is; anything else is sent as JSON with `content-type: application/json`. |
| `delay` | Milliseconds to wait before answering. |
| `pending` | `true` never answers, for a loading state. The request is cancelled when the state renders again. |
| `failed` | `true` fails like a network error, as when offline: `fetch` rejects with a `TypeError`, and XMLHttpRequest fires `error`. |
| `passthrough` | `true` sends the request to the network after all. |

### Handlers

A mock's value can also be a function, which receives the request and the
preview's context and returns a response, a promise of one, or a `Response`:

```ts
'GET /api/customers': (request, context) => {
  const search = (request.query.search || '').toLowerCase();
  const customers = context.fixtures.customers.filter(customer => customer.name.toLowerCase().includes(search));
  return { body: customers, headers: { 'x-total-count': String(customers.length) } };
},
```

| Request field | Value |
| --- | --- |
| `method` | The method, in capitals. |
| `url` | The full URL. |
| `path` | The path, without the query string. |
| `query` | The query string as an object. A repeated parameter keeps its last value. |
| `headers` | The request headers, with lowercase names. |
| `body` | The body: an object or array for JSON, an object for a URL-encoded form, otherwise the text, or `undefined` for none. |
| `operationName`, `variables` | For a GraphQL request; otherwise `null`. |

The context is the same one hooks receive: `state`, `inputs`, `fixtures`,
`globals`, `action`, and the rest. Handlers run in the browser, so they can
use anything your definition imports.

A handler that keeps data between calls acts as a small backend, so a list
shows what a form just added. Reset that data in the preview's `setup`, so
each render starts from the authored data:

```ts
const seed = [{ id: 'c-1', name: 'Ada' }];
let customers = [...seed];

export default definePreview({
  // …
  setup: () => { customers = [...seed]; },
  requests: {
    'GET /api/customers': () => ({ body: customers }),
    'POST /api/customers': (request) => {
      const customer = { id: 'c-' + (customers.length + 1), ...request.body };
      customers = [...customers, customer];
      return { status: 201, body: customer };
    },
  },
});
```

### GraphQL

A key with a third word matches a GraphQL operation by name. Workbench reads
the name from the request's `operationName`, or from the `query` text when the
client doesn't send one (`query Customers { … }` names `Customers`), in the
body of a `POST` or the query string of a `GET`. Clients such as Apollo Client
send the operation name with each request.

Answer with the usual GraphQL response shape:

```ts
requests: {
  'POST /graphql Customers': (request, context) => ({
    body: { data: { customers: context.fixtures.customers.slice(0, request.variables?.first ?? 20) } },
  }),
  'POST /graphql RenameCustomer': (request) => ({
    body: { data: { renameCustomer: { id: request.variables.id, name: request.variables.name } } },
  }),
},
states: {
  default: {},
  'load-error': { requests: { 'POST /graphql Customers': { body: { errors: [{ message: 'Internal server error' }] } } } },
},
```

A request that sends several operations at once, as an array, has no single
operation name, so only keys without one match it.

### Share mocks between previews

Definitions are modules, so keep mocks your previews share in a file of their
own and spread them in. Keys are tried in the order they appear, so put a
preview's own keys before the shared ones:

```ts
// src/workbench/mocks.ts
import type { RequestMocks } from '@canonic2/workbench';

export const session: RequestMocks = {
  'GET /api/me': { body: { id: 'u-1', name: 'Ada Lovelace' } },
  'GET /api/notifications': { body: [] },
};

// src/pages/customers.workbench.tsx
requests: {
  'GET /api/customers': { body: customers },
  ...session,
},
```

### Astro frontmatter

For an [Astro](astro.md) preview, `requests` also answers `fetch` in the
component's frontmatter, which runs on the Workbench server rather than in the
browser:

- A relative URL, such as `fetch('/api/plans')`, resolves against
  `http://localhost/`, and a path-only key matches it.
- The handler's context has `id`, `state`, `inputs`, `fixtures`, and
  `globals`.
- The page waits for every request before it renders, so a server render has
  no loading state to show. A render that waits more than 10 seconds, for a
  `pending` or long `delay` mock, fails.
- Unmatched requests answer `404` but aren't listed under **Actions**.

Scripts on the rendered page run in the browser and get the browser behavior.

### What request mocks don't cover

Mocks answer `fetch` and asynchronous XMLHttpRequest. These reach the network
as they would without Workbench: WebSocket, EventSource,
`navigator.sendBeacon`, synchronous XMLHttpRequest, and requests made from web
workers or service workers.

## Actions

**Actions**, in **Preview controls**, lists the last 30 things your page
tried to do, newest last. **Reset state** clears it.

| Entry | Logged when | Values |
| --- | --- | --- |
| Any name you choose | Your code calls `context.action(name, ...values)` | The values you pass |
| `navigate` | A link is clicked with **Actions** on, and `links` doesn't map it | The link's `href` |
| `submit` | A form is submitted with **Actions** on, and `links` doesn't map its `action` | The form's `action`, when it has one, and its fields |
| `request` | The page sends a write: any method but `GET`, `HEAD`, and `OPTIONS`, or a GraphQL mutation | The method, path, and operation, and the body |
| `request` | A request matches no mock in a preview that has some | The method, path, and operation, followed by `no mock` |

Values are shown as text; objects are shown as JSON.

On a [docs page](docs-pages.md), **Actions** lists the examples'
`context.action` calls, and the page's links and forms when **Actions** is on.

Log your own events with `context.action`. Components usually report them
through callback props or events; connect those in an environment or a page
script:

- **React:** [Props, children, and callbacks](react.md#props-children-and-callbacks)
- **Vue:** [Events and actions](vue.md#events-and-actions)
- **React Native Web:** an environment that adds `onPress`, as in
  [Write a first preview](react-native-web.md#write-a-first-preview)
- **Astro and HTML:** a page script calling `window.workbench?.action(…)`;
  see [Styles, scripts, and assets](astro.md#styles-scripts-and-assets) and
  [Inputs, states, and actions](html.md#inputs-states-and-actions)

## Links and navigation

A preview never leaves itself through the browser. Workbench decides where its
links and forms go, and the top bar's **Actions** switch decides whether they
go anywhere:

| | Actions off (the default) | Actions on |
| --- | --- | --- |
| A link to a spot on the same page, such as `#pricing` | Scrolls to it | Scrolls to it |
| A link or form whose address is in `links` | Nothing | Opens that preview, and the sidebar follows |
| Any other link or form: another route, another site, a download, `mailto:` | Nothing | Logged as `navigate` or `submit` |

Router components that render a real link, such as React Router's `Link` and
Vue Router's `RouterLink`, follow the same rules: they don't navigate when
Workbench has already handled the click.

### Map links to previews

`links` maps an address to a preview ID, to `{ preview, state }`, or to
`{ state }` for another state of the same preview:

```ts
export default definePreview({
  id: 'pages/pricing',
  adapter: 'astro',
  source: { entry: '../src/pages/pricing.astro' },
  links: {
    '/': 'pages/home',
    '/checkout/': { preview: 'pages/checkout', state: 'annual' },
    '/pricing/?billing=monthly': { state: 'monthly' },
  },
  states: { annual: {}, monthly: {} },
});
```

- An address matches a link that resolves to the same place, ignoring any
  `#fragment`: `'/'` matches `href="/"` and `href="/#features"`. A query string
  must match exactly.
- Write addresses the way your source writes its links. Relative addresses
  resolve against the preview's own address, as the page's relative links do.
- A form matches by its `action`.
- A target that names a preview or state that doesn't exist is reported with
  the other [configuration problems](troubleshooting.md#read-the-resolved-config),
  such as `<file>: link /checkout/ names unknown Workbench preview “pages/checkout”.`

### Navigate from code

When your page navigates from code rather than a link, such as a router call
in a click handler, call `context.navigate` with the same kinds of targets:

```ts
context.navigate('pages/checkout');
context.navigate({ preview: 'pages/checkout', state: 'annual' });
context.navigate({ state: 'monthly' });
```

It does nothing while **Actions** is off. Page scripts in HTML and Astro
previews reach it as `window.workbench?.navigate(…)`. To route your app's own
navigation through it, give the page a navigation function from an
environment, in the same provider your app uses to supply it.

### Portable exports

In a [portable export](workbench-previews.md#portable-exports), previews
behave as if **Actions** were on: a mapped link opens that preview in the
viewer, and other links and forms are logged.

## Recipes

### A page that loads its own data

The page fetches on mount and shows loading, error, empty, and filled states.
It needs no changes for Workbench.

`src/pages/CustomersPage.tsx`:

```tsx
import { useEffect, useState } from 'react';

type Customer = { id: string; name: string };

export function CustomersPage() {
  const [customers, setCustomers] = useState<Customer[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    fetch('/api/customers')
      .then(response => { if (!response.ok) throw new Error('HTTP ' + response.status); return response.json(); })
      .then(setCustomers, failure => setError(failure.message));
  }, []);
  if (error) return <p role="alert">Customers could not load: {error}</p>;
  if (!customers) return <p>Loading customers…</p>;
  if (!customers.length) return <p>No customers yet.</p>;
  return <ul>{customers.map(customer => <li key={customer.id}>{customer.name}</li>)}</ul>;
}
```

`src/pages/customers.workbench.tsx`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'pages/customers',
  title: 'Pages/Customers',
  adapter: 'react',
  source: { entry: './CustomersPage.tsx', export: 'CustomersPage' },
  fixtures: { customers: [{ id: 'c-1', name: 'Ada' }, { id: 'c-2', name: 'Grace' }] },
  requests: {
    'GET /api/customers': (request, context) => ({ body: context.fixtures.customers }),
  },
  states: {
    default: {},
    empty: { fixtures: { customers: [] } },
    loading: { requests: { 'GET /api/customers': { pending: true } } },
    'server-error': { requests: { 'GET /api/customers': { status: 500 } } },
    offline: { requests: { 'GET /api/customers': { failed: true } } },
  },
});
```

**Default** lists Ada and Grace, **Empty** says there are no customers yet,
**Loading** stays on its loading text, **Server Error** shows `HTTP 500`, and
**Offline** shows the browser's network error.

### A view that reads a context

Some apps split a page into a provider that loads data and a view that reads
it from a context. Render the view, and provide the context from an
environment with each state's fixtures:

`src/pages/album/album-environment.tsx`:

```tsx
import type { ReactElement } from 'react';
import type { PreviewContext } from '@canonic2/workbench';
import { AlbumContext } from './album-context';

export function wrap(element: ReactElement, context: PreviewContext) {
  const value = {
    album: context.fixtures.album ?? null,
    isLoading: context.state === 'loading',
    isError: context.state === 'error',
    onPlay: (trackId: string) => context.action('play', trackId),
  };
  return <AlbumContext.Provider value={value}>{element}</AlbumContext.Provider>;
}
```

`src/pages/album/album.workbench.tsx`:

```ts
import { definePreview } from '@canonic2/workbench';
import { album } from './album-fixtures';

export default definePreview({
  id: 'pages/album',
  adapter: 'react',
  source: { entry: './AlbumView.tsx', export: 'AlbumView' },
  environment: './album-environment.tsx',
  fixtures: { album },
  states: { default: {}, loading: {}, error: {} },
});
```

Playing a track logs `play` under **Actions**.

### A Vue component with app-provided data

A Vue component that reads a `provide`d value or a store gets it from the
environment's `configure`, which runs for every render's new app:

```ts
// src/workbench/vue-environment.ts
import type { App } from 'vue';
import type { PreviewContext } from '@canonic2/workbench';
import { cartKey } from '../cart';

export function configure(app: App, context: PreviewContext) {
  app.provide(cartKey, { items: context.fixtures.cart ?? [] });
}
```

A store such as Pinia works the same way: create it in `configure`, install
it with `app.use`, and set its state from `context.fixtures` before the
component mounts.

### An Astro page that fetches in its frontmatter

```astro
---
// src/pages/plans.astro
const response = await fetch('/api/plans');
const plans = response.ok ? await response.json() : null;
---
{plans ? <ul>{plans.map(plan => <li>{plan.name}</li>)}</ul> : <p>Plans could not load</p>}
```

```ts
// src/pages/plans.workbench.ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'pages/plans',
  adapter: 'astro',
  source: { entry: './plans.astro' },
  requests: { 'GET /api/plans': { body: [{ name: 'Team' }, { name: 'Business' }] } },
  states: {
    default: {},
    failed: { requests: { 'GET /api/plans': { status: 500 } } },
  },
});
```

### Pages that lead to each other

Give each page its own preview, map the routes between them in `links`, and
turn on **Actions** to walk the flow:

```ts
// sign-in.workbench.ts
links: {
  '/forgot-password': 'auth/reset-password',
  '/dashboard': 'pages/dashboard',
},
```

A sign-in form whose `action` is `/dashboard` opens the dashboard preview when
submitted. A form that only calls `fetch` does not navigate by itself; answer
its request with a mock, and call `context.navigate` from the code that would
move on in your app.
