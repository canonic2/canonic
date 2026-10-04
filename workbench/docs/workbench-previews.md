# TypeScript Workbench previews

A Workbench preview renders a component or screen from your own source code,
in named states, next to your design pages. You define it in a
`.workbench.ts` or `.workbench.tsx` file, with an adapter for HTML, React, Vue,
Astro, React Native Web, or a technology you register yourself.

Like a story in Storybook, a preview renders your real component or page, but
not a running copy of your app. Each state supplies the data the screen would
load, through props, providers, or [mocked requests](preview-data.md#request-mocks),
and Workbench keeps it on the canvas: its links open other previews, and
everything else it would do on a real site, such as following a route,
submitting a form, or starting a download, is recorded under **Actions**
instead. See [Preview data, mocks, and actions](preview-data.md).

This page covers what every preview shares: definitions, states, controls,
lifecycle hooks, the command-line tools, and portable exports. Each framework
has its own guide:

| Adapter | Renders | Guide |
| --- | --- | --- |
| `react` | A React component, with `inputs` as props | [React](react.md) |
| `react-native-web` | A React Native component through React Native Web | [React Native Web](react-native-web.md) |
| `vue` | A Vue 3 single-file component or module, with `inputs` as props | [Vue](vue.md) |
| `astro` | An Astro component or page, rendered on the server | [Astro](astro.md) |
| `html` | An HTML file with its styles and scripts, or a module that mounts into the canvas | [HTML](html.md) |
| Your own | Anything you can mount from a browser module | [Custom adapters](custom-adapters.md) |

You need:

- `workbench.yaml` at the project root. `name: Acme` alone is enough when
  previews supply every screen.
- A [trusted workspace](extension.md#workspace-trust), since previews run
  project code.
- The framework you render with, installed in your project. Workbench compiles
  TypeScript, JSX, and Vue files itself, but uses your project's React, Vue,
  Astro, or React Native Web. It never installs or replaces them.

## Add a preview

1. Write a definition next to the component, such as `src/Button.workbench.ts`:

   ```ts
   import { definePreview } from '@canonic2/workbench';

   export default definePreview({
     id: 'components/button',
     title: 'Components/Button',
     adapter: 'react',
     source: { entry: './Button.tsx', export: 'Button' },
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

   Workbench supplies `@canonic2/workbench` when it compiles the file. For
   TypeScript and your editor, install it as a development dependency; see
   [Types](#types).
   This example renders a React component; each framework guide has a complete
   example of its own.

2. Run **Workbench: Refresh Screens**, or save `workbench.yaml`. The preview
   appears in the sidebar with its states, and opens on the canvas like any
   other screen.

Workbench finds every `*.workbench.ts` and `*.workbench.tsx` file in the
project. It skips hidden folders and `node_modules`, `dist`, `build`, and
`coverage`. To choose other files or turn previews off, see
[Previews in the configuration reference](configuration.md#previews).

The `title` places the preview in the sidebar. Its first segment names the
section, its last the screen, and any segments between them a folder:
`Components/Forms/Button` is **Button**, in a **Forms** folder, in the
**Components** section. A title without `/` goes in a **Previews** section.
Without a `title`, the `id` is used.

A file that fails to load or has an invalid definition is reported with the
other [configuration problems](troubleshooting.md#read-the-resolved-config).
The remaining previews still load.

To place a preview yourself, give a screen in `workbench.yaml` the definition
as its `src`, such as `src: src/Button.workbench.ts`. The screen keeps its place
and label, and takes its states from the definition. The file must still match
the discovery patterns.

## Define a preview

| Key | Description |
| --- | --- |
| `id` | Required. Kebab-case segments separated by `/`, such as `components/button`. Unique in the project. |
| `title` | Where the preview appears in the sidebar. Defaults to `id`. |
| `adapter` | Required. `html`, `react`, `vue`, `astro`, `react-native-web`, or a [registered](#register-other-technologies) name. |
| `source` | Required. `entry` is the file to render, relative to the definition and inside the project. `export` names its export and defaults to `default`. |
| `inputs` | Data passed to the source: props for React and Vue, `Astro.props` for Astro. |
| `controls` | Inputs you can edit in **Preview controls**. See [Controls](preview-data.md#controls). |
| `states` | A map of kebab-case state IDs to [states](#states). |
| `links` | The addresses the source links or submits to, each mapped to the preview it opens. See [Links and navigation](preview-data.md#links-and-navigation). |
| `requests` | Answers to the page's `fetch` and `XMLHttpRequest` calls. See [Request mocks](preview-data.md#request-mocks). |
| `viewports` | The frame sizes the preview supports: any of `fit`, `desktop`, `mobile`, and `responsive`. See [Viewports](pages-and-states.md#viewports). |
| `docs` | Text shown under **Documentation** in **Preview controls**. |
| `fixtures`, `globals` | Data and settings that environments, hooks, and request handlers read from the context. See [Fixtures and globals](preview-data.md#fixtures-and-globals). |
| `styles` | Stylesheets to load with the preview, relative to the definition. |
| `assets` | Extra local files or folders the source or a compiler plugin reads at runtime, relative to the definition. |
| `environment` | A module, relative to the definition, that wraps or configures every state. See [Environments](preview-data.md#environments). |
| `setup`, `play`, `ready` | [Lifecycle hooks](#lifecycle-hooks) for every state. |

`inputs`, `fixtures`, and `globals` must be plain, cloneable data.

### States

Each state can set:

- `label`, the name in the sidebar. It defaults to the ID in title case, so
  `is-busy` reads **Is Busy**.
- `inputs`, `fixtures`, and `globals`, which override the preview's values
  key by key; see [What you can set, and where](preview-data.md#what-you-can-set-and-where).
- `source`, to render a different entry or export in this state.
- `requests`, tried before the preview's
  [request mocks](preview-data.md#which-mock-answers).
- `setup`, `play`, and `ready` hooks.

The first state is the one a preview opens in. Every state also opens directly
with `?state=<id>` on the preview's address; an unknown state shows an error.
A preview without `states` has one state, `default`.

The `<html>` element carries `data-wb-state` with the current state ID, so CSS
keyed off it works as it does in [design pages](pages-and-states.md#1-css-keyed-off-the-root).

## Adapters

`adapter` picks how the `source` renders. The built-in adapters are `html`,
`react`, `vue`, `astro`, and `react-native-web`; see the table at the top of
this page for each one's guide. Any other name must be
[registered](#register-other-technologies).

## Data, controls, and actions

A preview gets the data its real screen would load, and you can change it per
state and while you review. [Preview data, mocks, and actions](preview-data.md)
covers all of it:

- **Inputs and controls:** props for the component, and fields under
  **Preview controls** that edit them live. **Reset state** drops your edits
  and the action log.
- **Fixtures and globals:** data and settings for the code around the component.
- **Environments:** providers, plugins, stores, and setup, for one preview or,
  from `workbench.config.ts`, for the whole project.
- **Request mocks:** answers to the page's own `fetch` and XMLHttpRequest
  calls, with empty, loading, error, and offline states.
- **Actions:** what the screen tried to do, listed under **Actions** in
  **Preview controls**.
- **Links and navigation:** links that open other previews, with the
  toolbar's **Actions** switch on.

## Lifecycle hooks

Hooks and sources receive a context with `id`, `state`, `inputs`, `fixtures`,
`globals`, an abort `signal`, `action(name, ...values)`, `navigate(to)`, and
`error(error)`.

- `setup(context)` runs before the source mounts and may return a cleanup
  function. It runs from the project's [environment](preview-data.md#environments),
  then the preview's `environment`, then the preview, then the state.
  [Request mocks](preview-data.md#request-mocks) are already active, so
  `setup` can fetch.
- `play({ canvas, ...context })` runs after mounting, for an interaction
  sequence such as opening a menu. The preview's runs before the state's.
- `ready(context)` runs last, for anything the preview must wait for.

An [environment](preview-data.md#environments) module can export `setup` and
`ready` too, and a hook for its adapter, such as `wrap` for React.

A preview is ready once its hooks finish, its fonts load, and its visible
images load or 3 seconds pass. Screenshots and handoffs wait for that, and fail
with the rendering error, or with `Workbench preview did not finish rendering
before capture` after 8 seconds.

Before a preview renders again, its signal is aborted and cleanups run in
reverse order. Tie subscriptions, timers, and listeners to `context.signal` or a
cleanup. A failed cleanup doesn't stop the others, and **Reset state** recovers
after a failed render.

Errors are shown in the preview's frame: thrown errors, rejected promises,
React error boundaries, Vue's error handler, and anything a custom adapter
passes to `context.error(error)`.

Saving the definition or any file it uses reloads the open preview within a
second or so. This is a full reload, not hot module replacement. If you add or
remove a definition, or change its `title` or states, run
**Workbench: Refresh Screens** to update the sidebar.

## Register other technologies

Adapter names aren't a fixed list. `workbench.config.ts` at the project root,
or the file named by [`previews.config`](configuration.md#previews), registers
adapters and sets compiler plugins, `aliases`, `dedupe`, `resolveExtensions`,
and `define` for every preview. An adapter whose name isn't built in or
registered fails with `Unknown adapter`. See [Custom adapters](custom-adapters.md).

## Compare a design with a preview

A screen can show a preview as a lens next to its design, with a `workbench`
implementation. See
[Compare a design with a Workbench preview](lenses.md#compare-a-design-with-a-workbench-preview).

## Types

Install the authoring types so `tsc` and your editor resolve
`@canonic2/workbench`:

```sh
npm install --save-dev @canonic2/workbench
```

Use `pnpm add -D` or `yarn add -D` with those package managers. The types
check that each state's `inputs` match the preview's. Workbench still compiles
previews with its own copy, so the installed version affects types only; keep
it at the version of your extension to type the newest fields.

Without an installed package, the `init` command below writes the same types to
a `workbench-env.d.ts` file. Include that file in your `tsconfig.json`.

## Command-line tools

The extension includes a command-line tool at `preview/cli.cjs` in its install
folder. Run it with Node 18 or later:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-<version>/preview/cli.cjs check .
```

| Command | What it does |
| --- | --- |
| `init <project>` | Writes `workbench-env.d.ts`, the types for `@canonic2/workbench`, and a `workbench.yaml` named after the folder. Existing files are left alone. |
| `check <project>` | Finds and compiles every preview without opening a browser. Prints `Built <id>` for each, lists every failure, and exits with an error if there were any. |
| `build <project> [output]` | Writes the [standalone viewer](#portable-exports) to `output`, or `workbench-static` in the project. The folder must be empty or missing. Successful previews are written even when others fail; failures are listed and the exit code is nonzero. |

## Portable exports

**Download design-system ZIP** includes compiled previews in a `browser/`
folder, alongside their editable sources. See
[Design-system export](design-system-export.md). Serve the extracted folder
with any static HTTP server and open `browser/index.html`. The `build` command
writes the same viewer on its own. Viewing it needs no Workbench, Electron,
package installation, or build step.

The viewer has:

- a searchable list of previews, and the build warnings, if any,
- **State** and **Viewport** menus, with **Width** and **Height** for
  **Resizable**,
- **Reload**, and **Preview controls** with the same inputs, reset, actions,
  and documentation as the canvas,
- **Open preview**, which opens the component or screen on its own page. That
  page also accepts `?state=<id>`.

The viewer's address keeps the preview, state, viewport, and resizable size, so
you can copy it to share a selection.

Astro previews carry every authored state but no input controls; see
[Astro portable exports](astro.md#portable-exports).
