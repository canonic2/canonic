# TypeScript Workbench previews

A Workbench preview renders a component or screen from your own source code,
in named states, next to your design pages. You define it in a
`.workbench.ts` or `.workbench.tsx` file, with an adapter for HTML, React, Vue,
Astro, React Native Web, or a technology you register yourself.

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
   import { definePreview } from '@canonic/workbench';

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

   You don't install `@canonic/workbench`: Workbench supplies it when it
   compiles the file. For editor types, see [Command-line tools](#command-line-tools).
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
| `controls` | Inputs you can edit in **Preview controls**. See [Controls and actions](#controls-and-actions). |
| `states` | A map of kebab-case state IDs to [states](#states). |
| `viewports` | The frame sizes the preview supports: any of `fit`, `desktop`, `mobile`, and `responsive`. See [Viewports](pages-and-states.md#viewports). |
| `docs` | Text shown under **Documentation** in **Preview controls**. |
| `fixtures`, `globals` | Data the source and hooks read from the [context](#lifecycle-hooks). |
| `styles` | Stylesheets to load with the preview, relative to the definition. |
| `assets` | Extra local files or folders the source or a compiler plugin reads at runtime, relative to the definition. |
| `environment` | A module, relative to the definition, that wraps or configures every state. See [Lifecycle hooks](#lifecycle-hooks). |
| `setup`, `play`, `ready` | [Lifecycle hooks](#lifecycle-hooks) for every state. |

`inputs`, `fixtures`, and `globals` must be plain, cloneable data.

### States

Each state can set:

- `label`, the name in the sidebar. It defaults to the ID in title case, so
  `is-busy` reads **Is Busy**.
- `inputs`, `fixtures`, and `globals`, which override the preview's values of
  the same name.
- `source`, to render a different entry or export in this state.
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

## Controls and actions

When a preview is ready, **Preview controls** appears in the toolbar. It opens
a panel with:

- a field for each entry in `controls`,
- **Reset state**, which drops your edits and the action log and renders the
  state again,
- **Actions**, the last 30 calls to `context.action(name, ...values)`,
- **Documentation**, the preview's `docs` text, when it has some.

| Control `type` | Field | Options |
| --- | --- | --- |
| `text` | Text box | |
| `number` | Number box | `min`, `max`, `step` |
| `boolean` | Checkbox | |
| `select` | Menu | `options`, a nonempty list of strings or numbers |
| `json` | Text area of JSON | |

Every control also takes a `label`, which defaults to the input's name. An edit
applies when the field changes. Edits last until you reset, change state, or
leave the screen; they never change the definition, screenshots of reference
states, or exports.

## Lifecycle hooks

Hooks and sources receive a context with `id`, `state`, `inputs`, `fixtures`,
`globals`, an abort `signal`, `action(name, ...values)`, and `error(error)`.

- `setup(context)` runs before the source mounts and may return a cleanup
  function. It runs from the `environment`, then the preview, then the state.
- `play({ canvas, ...context })` runs after mounting, for an interaction
  sequence such as opening a menu. The preview's runs before the state's.
- `ready(context)` runs last, for anything the preview must wait for.

The `environment` module can export `setup` and `ready`, and one adapter hook:

| Adapter | Export |
| --- | --- |
| `react` | `wrap(element, context)`, returning the element wrapped in providers |
| `vue` | `configure(app, context)`, to install plugins, and `wrap(vnode, context)` |
| `html`, `astro` | `mount(canvas, context)`, called after the rendered HTML is in place |

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

## Command-line tools

The extension includes a command-line tool at `preview/cli.cjs` in its install
folder. Run it with Node 18 or later:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-<version>/preview/cli.cjs check .
```

| Command | What it does |
| --- | --- |
| `init <project>` | Writes `workbench-env.d.ts`, the types for `@canonic/workbench`, and a `workbench.yaml` named after the folder. Existing files are left alone. |
| `check <project>` | Finds and compiles every preview without opening a browser. Prints `Built <id>` for each, lists every failure, and exits with an error if there were any. |
| `build <project> [output]` | Writes the [standalone viewer](#portable-exports) to `output`, or `workbench-static` in the project. The folder must be empty or missing. Successful previews are written even when others fail; failures are listed and the exit code is nonzero. |

For types in your editor, include `workbench-env.d.ts` in your
`tsconfig.json`. The types check that state `inputs` match the preview's.

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
