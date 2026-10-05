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

- `problems`: every line Workbench couldn't use, named by collection and page,
  such as `Pages › Sign in: implementation “dev” isn’t declared under implementations.`
- `pages`: each page's `design` file as an absolute path, and its `code`
  pointers resolved against their implementation's `root`, each with `exists`
  saying whether it is on disk. For a Markdown page, `design` is its Markdown
  file. For any other page with docs, `code` includes its Markdown, labeled
  `Docs`. For both, `code` includes each docs lens's example source,
- `implementations`: each implementation's resolved address, `root`, and start
  command, including values from `workbench.local.yaml`,
- `files`: whether `workbench.yaml` (`main`) and `workbench.local.yaml`
  (`local`) were read.

Read `problems` first whenever a lens, story, preview, catalog page, docs
example, or source link is missing. A hand-written page dropped for a bad
`src`, label, state id, or group isn't listed there; see
[A page is missing from the list](#a-page-is-missing-from-the-list).

In VS Code, the problems list in the sidebar shows the same problems when
TypeScript previews or a catalog are on. A standalone browser doesn't list
them, so read the config route there.

### Check a Storybook title

```sh
curl -s "http://127.0.0.1:3579/_workbench/stories?implementation=storybook&title=Components%2FButton"
```

| Answer | Means |
| --- | --- |
| 200 | The title's stories, with ids, names, states, and resolved source paths. |
| 404 | The title doesn't exist. The answer names the nearest titles, including the prefixed title when only the prefix differs. |
| 400 | No Storybook implementation has that name, `url: auto` found no running Storybook, or the request has no `title`. |
| 502 | Something answered at the address, but `/index.json` returned an error. It may not be Storybook 7 or later. |
| 503 | Storybook isn't answering at the configured address. |

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
  Open the folder that contains it, or add that folder to the workspace; see
  [Projects in a subfolder](extension.md#projects-in-a-subfolder).
- In a multi-root workspace, every folder with a `workbench.yaml` adds its
  spaces, but the view shows one at a time. Use the switcher at the top of the
  view; see [Several spaces](spaces.md).
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
| *nothing to show* | No collection has a usable page. Read `problems` for why each was dropped. |

See [YAML that the reader accepts](configuration.md#yaml-that-the-reader-accepts).

## A page is missing from the list

A page, group, or collection that can't be used is dropped, and the rest of
the list still builds. These problems are written to the canvas's console,
not to the config route: in VS Code, run
**Developer: Open Webview Developer Tools** and read the console. The usual
reasons:

- It has no `label` or no `src`.
- Its `src` starts with `/`, contains `..`, or contains `:`, `!`, or `~`.
- It is in a group inside another group. Groups don't nest.
- Its collection (its `collections` entry) has no `name`.

HTML pages aren't scanned for. A page that isn't listed in `workbench.yaml`
isn't shown, except for pages imported by a [catalog](configuration.md#catalogs)
and the previews and docs pages that `*.workbench.ts` files define.

## A page is blank or shows a 404

- The `src` path is relative to the project root, not to `workbench.yaml`'s
  folder or the page's folder. Check the page's `design` path in the config
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
- **Added or removed a file:** run **Workbench: Refresh Pages**. Only changes
  to the YAML files refresh the list on their own.
- **`Workbench previews: …`** The worker or `workbench.config.ts` failed. The
  log's `preview.worker` entries hold the worker's error output.
- **`unknown Workbench preview “…”`** A page's `kind: workbench` lens names an
  id that no definition declares.
- **The canvas shows an error instead of the component:** the message is the
  compile or render error, such as an unknown adapter or a framework package
  that isn't installed in the project.
- **`link … names unknown Workbench preview` or `… unknown state`:** an entry
  in the definition's `links` names a preview ID or state no definition
  declares. See [Links and navigation](preview-data.md#links-and-navigation).
- **`request … must be "[METHOD] /path [Operation]" or a full URL`:** a
  `requests` key isn't in that form. Paths start with `/`.
- **`The preview config environment must be a file inside the project`:**
  `environment` in `workbench.config.ts` names a missing file. It's relative to
  the project root.

## A docs page's examples don't render

A page's [docs](docs-pages.md) render their Markdown even when the examples
can't, and each example's panel says why it is empty.

- **Examples run your project's code.** They render only in a trusted
  workspace, and not when `workbench.yaml` sets `previews: false`.
- **Read the problems list,** or `problems` in the config route. It names the
  page for a missing Markdown file, an example block with an unknown key or a
  duplicate ID (with its line), an example a lens has that the docs never
  place, example file names that aren't kebab-case, and a lens that fails to
  build.
- **A panel says *Not available in …*:** the lens showing has no example with
  that ID. Switch lenses, or add the example to that lens's source.
- **A panel says there is no docs lens rendering examples:** the page is in
  its built-in **Docs** lens because it maps no `docs` implementation. Declare
  one and map it to the page's example source. See
  [Lenses](docs-pages.md#lenses).

See [Requirements and problems](docs-pages.md#requirements-and-problems).

## A preview shows the wrong data, or a request fails

- **A request answers `404` with `No Workbench request mock for …`:** the
  preview has `requests`, and none matches this one. **Actions** in
  **Preview controls** lists it as `request … — no mock`. Add a key for it, or
  answer it with `{ passthrough: true }` to let it reach the network.
- **The preview shows live data:** the preview has no `requests`, so every
  request reaches the network. Add mocks; once there is one, unmatched requests
  stop reaching live services.
- **A mock doesn't apply:** check the method, that the path starts with `/`,
  and that a GraphQL key's operation name matches the one the client sends. A
  state's `requests` replace the preview's for the same key only.
- **A request isn't mocked at all:** request mocks answer `fetch` and
  XMLHttpRequest. WebSocket, EventSource, `navigator.sendBeacon`, synchronous
  XMLHttpRequest, and requests from workers reach the network.
- **An Astro page fails after 10 seconds:** a frontmatter `fetch` answered by a
  `pending` or long `delay` mock. A server render can't show a loading state;
  see [Astro](astro.md#preview-a-page).

See [Give a page its data](preview-data.md#choose-how-to-give-a-page-its-data).

To check every preview without the canvas, run the extension's checker on the
project. It builds each preview and lists the failures:

```sh
node ~/.vscode/extensions/canonic.canonic-workbench-*/preview/cli.cjs check path/to/project
```

See [TypeScript Workbench previews](workbench-previews.md).

## States don't show or don't change anything

- **No states in the page list:** a page needs at least two valid states.
  Each needs an `id` and a `label`, and ids must be kebab-case.
- **The state is listed, but the page doesn't change:** the page has to answer to
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
Turn on **Actions** in the top bar. With actions on, a link to an `.html` page
in the project that isn't listed in `workbench.yaml` still does nothing; add
the page there. See [Links and actions](pages-and-states.md#links-and-actions).

In a [Workbench preview](preview-data.md#links-and-navigation), a link
opens another page only when the definition's `links` maps its address to a
preview. Every other link and form is recorded under **Actions** in
**Preview controls**, by design; map the address to make it open a preview.

## A size is disabled

The page's `sizes` doesn't list it. In a docs lens, every size in the size
switcher is disabled, because the docs fill the canvas and there is no
artboard; switch to the design lens to choose a size. Hover over the button to
see why. See [Sizes](pages-and-states.md#sizes).

## A lens is missing

- The lens switcher shows only when the page has two or more lenses.
- The page must list the implementation under its own `implementations`,
  and the implementation must be declared at the top level. Read `problems`.
- A docs lens needs the page's Markdown: a `.md` `src`, or `docs` on the
  page. A Markdown page has no design lens. See [Lenses](docs-pages.md#lenses).
- For `url`, every path must start with `/`, and a state map needs a path for
  the default state.
- For `storybook`, the title must match exactly. [Check the title](#check-a-storybook-title).
- An implementation whose `base` or `url` isn't an `http://` or `https://`
  address is dropped.

## A lens has the wrong name

- For an implementation lens, set `label` on the top-level implementation,
  whatever its kind. Renaming its key also changes page mappings and addresses;
  use the display label to keep them working.
- For the page's own authored lens, use `lensLabel` on its page entry. For
  discovered TypeScript previews, `previews.lensLabel` supplies the default;
  an explicit page's `lensLabel` wins over it.
- A page's `label` and a preview's `title` name the page in the sidebar,
  rather than its lens. `lensLabel` belongs in YAML, not `definePreview`.
- Blank or non-string `lensLabel` values are reported and ignored. Read
  `problems` in the [resolved config](#read-the-resolved-config).
- A docs lens takes its name from its `docs` implementation's `label`. A
  Markdown page has no authored lens, so `lensLabel` doesn't apply to it.

See [Customize lens labels](lenses.md#customize-lens-labels) for examples.

## A URL lens is blank, refuses to load, or loses its sign-in

Workbench loads the page through a loopback proxy that removes framing
headers (see [Embedding and sign-in](lenses.md#embedding-and-sign-in)). If it
still doesn't load:

- **The frame says the app isn't answering:** the server isn't running. Add a
  [start command](lenses.md#start-the-server-automatically), or start it.
- **Sign-in doesn't stick:** the sign-in returned to your app's own origin
  instead of the proxy. See [Keep the session](lenses.md#keep-the-session).
- **Something else:** open the canvas's developer tools and read the frame's
  console.

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
| A lens screenshot shows a sign-in page, or not what you typed, opened, or scrolled to | The page hadn't finished loading, so its preview bridge hadn't connected, and the helper loaded the URL itself. Take the screenshot again. See [Screenshots through a URL lens](lenses.md#screenshots-through-a-url-lens). |
| Screenshots of one page fail with a message | The page contains an `iframe`, `object`, or `embed`, or a canvas or video loaded from another origin without CORS. These pages can't be captured. |
| A font or image is missing in the first screenshot after loading | The helper's copy was still loading assets. Take it again. |
| Screenshots fail with an error | Read **Workbench: Show Log**. The screenshot helper needs macOS 13 or later, Windows, or Linux with a display; see [The screenshot helper](extension.md#the-screenshot-helper). |

See [What screenshots can and can't include](annotations-and-handoff.md#what-screenshots-can-and-cant-include).

## Copy handoff is missing

Handoffs need the VS Code extension's server. They work in the editor and in a
browser opened with **Workbench: Open Canvas in Browser**, but not with a server
you started yourself with `node server.js`. Use the camera there instead.

## An agent can't find the handoff's screenshot

The prompt's `Screenshot:` path is relative to the folder that holds
`workbench.yaml`. Run the agent in that folder, or give it the file from
`.canonic/.handoffs/` yourself. See
[Pasting into an agent](annotations-and-handoff.md#pasting-into-an-agent).

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
