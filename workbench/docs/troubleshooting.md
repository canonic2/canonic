# Troubleshooting

Workbench reports what it can't use instead of guessing, so most problems have
a message somewhere. This page lists where to look, then common symptoms.

## Where to look

### Read the resolved config

The server answers with the config as your machine resolves it, including
every problem it found:

```sh
curl -s http://127.0.0.1:3579/_workbench/config
```

Use **Workbench: Copy Canvas URL** for the actual port. The answer includes:

- `problems`: every line Workbench couldn't use, named by section and screen,
  such as `Pages › Sign in: implementation “dev” isn’t declared under implementations.`
- `screens`: each screen's `design` file as an absolute path, and its `code`
  pointers resolved against their implementation's `root`, each with `exists`
  saying whether it is on disk,
- `implementations`: each implementation's resolved address, `root`, and start
  command, including values from `workbench.local.yaml`,
- `files`: whether `workbench.yaml` (`main`) and `workbench.local.yaml`
  (`local`) were read.

Read `problems` first whenever a screen, lens, story, or source link is missing.

### Check a Storybook title

```sh
curl -s "http://127.0.0.1:3579/_workbench/stories?implementation=storybook&title=Components%2FButton"
```

| Answer | Means |
| --- | --- |
| 200 | The title's stories, with ids, names, states, and resolved source paths. |
| 404 | The title doesn't exist. The answer names the nearest titles, including the prefixed title when only the prefix differs. |
| 503 | Storybook isn't running at the configured address, or `url: auto` found none. |

### Other places

- **The Workbench log.** Run **Workbench: Show Log** for screenshot, server,
  handoff, preview worker, Simulator, and window stream errors. For slow
  TypeScript previews, `preview.request.completed` events separate the time a
  request waited in the worker's queue (`queueMs`) from compiling and serving
  it (`workMs`), and `preview.browser.phase` events time setup, mounting,
  fonts, images, and readiness.
- **The canvas's developer tools.** In VS Code, run
  **Developer: Open Webview Developer Tools** to see the canvas's console.
  Config problems are logged there, as well as framing and cookie errors from
  lenses.
- **The page on its own.** **Open on its own** opens the current page outside
  the workbench, which shows whether a problem is the page's or Workbench's.

## The Workbench view doesn't appear

- `workbench.yaml` must be at the root of an open folder, not in a subfolder.
- In a multi-root workspace, only the first folder with a `workbench.yaml` is
  served.
- The workspace must be trusted. VS Code doesn't run Workbench in Restricted
  Mode.
- After installing or updating, run **Developer: Reload Window**.

## The canvas says there's a problem in workbench.yaml

The message names the file and, for a YAML syntax error, the line. Common
causes:

| Message or cause | Fix |
| --- | --- |
| A tab character | Indent with spaces. |
| `{a: b}` or `[a, b]` | Write the map or list in block style. Flow collections aren't supported. |
| A value cut short at `#` | A space followed by `#` starts a comment. Quote the value. |
| *nothing to show* | No section has a usable screen. Read `problems` for why each was dropped. |

See [YAML that the reader accepts](configuration.md#yaml-that-the-reader-accepts).

## A screen is missing from the list

Each dropped screen is named in `problems`. The usual reasons:

- It has no `label` or no `src`.
- Its `src` starts with `/`, contains `..`, or contains `:` or `~`.
- It is inside a folder inside a folder. Folders don't nest.
- Its section has no `name`.

Nothing is scanned. A page that isn't listed in `workbench.yaml` isn't shown,
except for screens imported by a [catalog](configuration.md#catalogs).

## A screen is blank or shows a 404

- The `src` path is relative to the project root, not to `workbench.yaml`'s
  folder or the page's folder. Check the screen's `design` path in the config
  route.
- Assets with root-relative paths (`/styles/app.css`) resolve from the project
  root. If your pages expect another root, use relative paths.
- Open the page with **Open on its own** to see its own errors.

## A TypeScript preview is missing or broken

- **Missing from the list:** read `problems`. Each definition that couldn't be
  loaded is named with its file and reason, such as a duplicate id or a
  `source.entry` that doesn't exist. The other previews still show.
- **Never discovered:** the file must end in `.workbench.ts` or
  `.workbench.tsx`, match `previews.include` when you set it, and sit outside
  hidden folders, `node_modules`, and build output such as `dist` and `build`.
  `previews: false` turns discovery off.
- **Added or removed a file:** run **Workbench: Refresh Screens**. Only changes
  to the YAML files refresh the list on their own.
- **`Workbench previews require a trusted workspace.`** Trust the folder.
- **`Workbench previews: …`** The worker or `workbench.config.ts` failed. The
  log's `preview.worker` entries hold the worker's error output.
- **`unknown Workbench preview “…”`** A screen's `kind: workbench` lens names an
  id that no definition declares.
- **The canvas shows an error instead of the component:** the message is the
  compile or render error, such as an unknown adapter or a framework package
  that isn't installed in the project.

To check every preview without the canvas, run the extension's checker on the
project. It builds each preview and lists the failures:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-*/preview/cli.cjs check path/to/project
```

See [TypeScript Workbench previews](workbench-previews.md).

## States don't show or don't change anything

- **No state rows:** a screen needs at least two valid states. Each needs an
  `id` and a `label`, and ids must be kebab-case.
- **The row is there, but the page doesn't change:** the page has to answer to
  the id. Check the spelling matches, and that you used one of the
  [three hooks](pages-and-states.md#states).
- **The first state doesn't match:** the first state always appears in the page
  as `data-wb-state="default"`, whatever its id.
- **Content added by scripts doesn't change:** `data-wb-state-only`, `-not`, and
  `data-wb-set-*` are applied once, at `DOMContentLoaded`. Style late content
  with `html[data-wb-state=…]` instead.
- **The page opened from disk ignores states:** only pages served by Workbench
  get `states.js`.

## Links don't work in a page

Actions are off by default, so links don't navigate and forms don't submit.
Turn on **Actions** in the toolbar. See [Links and actions](pages-and-states.md#links-and-actions).

## A width button is disabled

The screen's `viewports` doesn't list it. Hover over the button to see why. See
[Viewports](pages-and-states.md#viewports).

## A lens is missing

- The screen must list the implementation under its own `implementations`,
  and the implementation must be declared at the top level. Read `problems`.
- For `url`, every path must start with `/`, and a state map needs a path for
  the default state.
- For `storybook`, the title must match exactly. [Check the title](#check-a-storybook-title).
- An implementation whose `base` or `url` isn't an `http://` or `https://`
  address is dropped.

## A URL lens is blank, refuses to load, or loses its sign-in

Open the canvas's developer tools and look for:

- **`X-Frame-Options` or `frame-ancestors` errors:** the app refuses to be
  framed. Relax that in its development environment. See
  [Allow framing](lenses.md#allow-framing).
- **Cookie warnings:** the session cookie isn't sent in a frame. See
  [Keep the session](lenses.md#keep-the-session).
- **Connection refused:** the server isn't running. Add a
  [start command](lenses.md#start-the-server-automatically), or start it.

Workbench never falls back to another way of showing the page. **Open on its
own** works while you fix the app.

## A start command doesn't run

- The workspace must be trusted, and you must be in VS Code. Standalone servers
  don't run start commands.
- If the `check` already passes, nothing is started. Check that the port or URL
  is the right one.
- If startup times out, raise `timeout` (up to 300 seconds), or point `ready`
  at a URL that answers once the server can actually serve.
- The command's terminal stays open in VS Code. Read its output.

## Storybook problems

See [Storybook troubleshooting](storybook.md#troubleshooting).

## Simulator and window streams

See [iOS Simulator troubleshooting](ios-simulator.md#troubleshooting) and
[App window troubleshooting](windows.md#troubleshooting).

## Screenshots look wrong

| Symptom | Cause |
| --- | --- |
| A lens screenshot shows a sign-in page | Without the preview bridge, the screenshot helper loads the page in its own session. [Add the bridge](lenses.md#screenshots-through-a-url-lens). |
| A lens screenshot doesn't show what you typed or scrolled to | Same cause: add the bridge. |
| Screenshots of one page fail with a message | The page contains an `iframe`, `object`, or `embed`, or a canvas or video loaded from another origin without CORS. These pages can't be captured. |
| A font or image is missing in the first screenshot after loading | The helper's copy was still loading assets. Take it again. |
| Screenshots fail with an error | Read **Workbench: Show Log**. If the bundled helper can't start, Workbench uses Chrome; set `canonic.capture.chromePath` if it can't find one. |

See [What screenshots can and can't include](markup-and-handoff.md#what-screenshots-can-and-cant-include).

## Copy handoff is missing

Handoffs need the VS Code extension's server. They work in the editor and in a
browser opened with **Workbench: Open Canvas in Browser**, but not with a server
you started yourself with `node server.js`. Use the camera there instead.

## Source links don't open

- Hover over the entry in the **Open the source** menu. Its tooltip says
  *Not on this machine* when the path doesn't exist, or that the implementation
  has no `root`.
- The implementation needs a `root`, and each `code` path is relative to it.
  Check `exists` in the config route.
- Opening files needs the VS Code extension's server. With a server you started
  yourself, the menu lists the entries, but picking one reports *Couldn't open it*.

## Something changed after an update

Run **Developer: Reload Window** in every open project window. Running windows
keep the previous version's server and helper until they reload.
