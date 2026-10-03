# workbench.yaml reference

`workbench.yaml` at the project root is the whole of a project's Workbench
setup. This page lists every key. For a guided introduction, start with
[Getting started](getting-started.md).

- [A complete example](#a-complete-example)
- [Top level](#top-level)
- [Previews](#previews)
- [Sections](#sections)
- [Folders](#folders)
- [Screens](#screens)
- [States](#states)
- [Viewports](#viewports)
- [Implementations](#implementations)
- [A screen's implementations](#a-screens-implementations)
- [Code pointers](#code-pointers)
- [Start commands](#start-commands)
- [Catalogs](#catalogs)
- [Local overrides](#local-overrides)
- [YAML that the reader accepts](#yaml-that-the-reader-accepts)
- [How problems are reported](#how-problems-are-reported)
- [Editing with the form](#editing-with-the-form)

## A complete example

```yaml
name: Acme

previews:
  include:
    - packages/ui/**/*.workbench.tsx

implementations:
  preview:
    kind: workbench
  storybook:
    kind: storybook
    url: auto
    root: packages/ui
    catalog:
      icon: book-open
      icons:
        Forms: text-cursor-input
    start:
      command: pnpm storybook
      check:
        port: 6006
      ready:
        url: http://localhost:6006/index.json
      timeout: 90
  dev:
    kind: url
    label: Local app
    base: http://localhost:3000
    root: apps/web
    start:
      command: pnpm dev
      cwd: apps/web
      check:
        port: 3000
  staging:
    kind: url
    base: https://staging.example.com

sections:
  - name: Pages
    icon: file-text
    items:
      - folder: Auth
        items:
          - label: Sign in
            src: design/pages/sign-in.html
            icon: log-in
            viewports:
              - desktop
              - mobile
            states:
              - id: default
                label: Default
              - id: error
                label: Wrong password
            implementations:
              dev:
                default: /sign-in
                error: /sign-in?error=1
              staging: /sign-in
            code:
              dev:
                - src/routes/sign-in
                - src/components/sign-in-form.tsx

  - name: Components
    icon: component
    items:
      - label: Button
        src: design/components/button.html
        viewports:
          - fit
          - responsive
        implementations:
          preview: components/button
          storybook: Components/Button
        code:
          storybook: src/button
```

## Top level

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | string | no | Titles the screen list and the canvas tab. Defaults to `Workbench`. |
| `sections` | list of [sections](#sections) | no, unless `previews: false` and no [catalog](#catalogs) | The sidebar's sections, in order. |
| `previews` | map or `false` | no | Where [TypeScript previews](#previews) are discovered. `false` turns them off. |
| `implementations` | map of name to [implementation](#implementations) | no | Where screens also exist as running code. |

With `previews: false`, no catalog, and no usable section, the config fails
with *nothing to show — a config needs at least one section with one screen in
it.*

## Previews

Workbench discovers [TypeScript previews](workbench-previews.md) in
`**/*.workbench.ts` and `**/*.workbench.tsx` files without any configuration.
Each preview becomes a screen, in the section named by the first segment of its
title, or in **Previews** when the title has no `/`. Previews run
project code, so in VS Code they need a trusted workspace.

```yaml
previews:
  include:
    - src/**/*.workbench.ts
  config: workbench.config.ts
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `include` | list of glob patterns | no | Which files are preview definitions, relative to the project root. Replaces the default patterns. Patterns can't start with `/` or contain `..`. |
| `config` | path | no | The preview configuration file, relative to the project root. Defaults to `workbench.config.ts`. See [Register other technologies](workbench-previews.md#register-other-technologies). |

`previews: false` turns off discovery and never runs preview code. An invalid
`previews` value is reported, and previews stay off until it is fixed.

A screen whose `src` is a preview definition, such as
`src: src/button.workbench.ts`, keeps its place in your sections and takes its
states from the definition.

## Sections

```yaml
sections:
  - name: Pages
    icon: file-text
    items:
      - label: Sign in
        src: pages/sign-in.html
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | string | yes | The section's label. [Previews](#previews) and [catalog](#catalogs) screens that belong in a section with the same name are added to it. |
| `icon` | Lucide icon name | no | Shown on the section and on its screens that don't set their own. Defaults to `file-text`. |
| `items` | list of [screens](#screens) and [folders](#folders) | yes | A section with nothing usable in it is dropped and reported. |

Icon names are [Lucide](https://lucide.dev/icons/) names in kebab-case, such as
`file-text`, `component`, `layout-dashboard`, or `shield-check`. Every Lucide
icon ships with the extension.

## Folders

A folder groups screens inside a section. Folders don't nest.

```yaml
items:
  - folder: Auth
    items:
      - label: Sign in
        src: pages/sign-in.html
      - label: Reset password
        src: pages/reset.html
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `folder` | string | yes | The folder's label. |
| `items` | list of [screens](#screens) | yes | An empty folder is dropped and reported. A folder inside a folder is rejected. |

## Screens

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `label` | string | yes | The screen's name in the sidebar, in screenshots, and in handoffs. |
| `src` | path | yes | The HTML file or [preview definition](#previews), relative to the project root. |
| `icon` | Lucide icon name | no | Overrides the section's icon for this screen. |
| `states` | list of [states](#states) | no | Variations of the page. Shown only when there are two or more. |
| `viewports` | list of [viewports](#viewports) | no | Which frame sizes the screen supports. Defaults to all four. |
| `implementations` | map | no | [Where this screen is in each implementation](#a-screens-implementations). |
| `code` | map | no | [Where this screen's code lives](#code-pointers), per implementation. |

`src` rules:

- It is relative to the project root. It can't start with `/` or contain `..`.
- It can't contain `:` or `~`, which mark the state and the lens in the
  [address](canvas.md#links-and-the-address).
- A screen without both `label` and `src` is dropped and reported, and the rest
  of the sidebar still builds.

`src` usually points at an HTML file, but any file the server can serve works,
including a page with a query string (`preview/index.html?component=button`).

## States

```yaml
states:
  - id: default
    label: Default
  - id: error
    label: Wrong password
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | kebab-case string | yes | Travels in the URL (`?state=error`), in the address, and in screenshot filenames. Lowercase letters, digits, and hyphens only. |
| `label` | string | yes | The row's name in the sidebar and the handoff. |

- The **first** state is the page as authored, whatever its id. The page loads
  without a `?state=` parameter for it. By convention it is called `default`.
- A screen with fewer than two states shows no state rows.
- Declaring a state only adds the row. The page has to answer to the id; see
  [Pages and states](pages-and-states.md#states).

## Viewports

```yaml
viewports:
  - desktop
  - mobile
  - responsive
```

| Value | Frame | Export reference size |
| --- | --- | --- |
| `desktop` | Laptop, MacBook Pro 14 | 1512 × 982 |
| `mobile` | iPhone 15 Pro | 393 × 852 |
| `responsive` | A frame you resize by dragging its edges | Both 1512 × 982 and 393 × 852 |
| `fit` | Fills the available canvas | 1440 × 900 |

- Omitting `viewports` enables all four.
- Width buttons for viewports a screen doesn't list are disabled while it is
  showing.
- A single value may be written without a list: `viewports: mobile`.
- An unknown value is reported and skipped. If no value is valid, all four
  are enabled.
- [Design-system exports](design-system-export.md) capture one reference per
  listed viewport, removing duplicate sizes.

## Implementations

Declared once at the top level, then referred to by name from screens. The
name is a kebab-case key, and it is also the lens button's label unless
`label` says otherwise (`local-dev` is shown as *Local dev*).

```yaml
implementations:
  dev:
    kind: url
    base: http://localhost:3000
```

| Key | Kinds | Required | Description |
| --- | --- | --- | --- |
| `kind` | all | yes | `workbench`, `url`, `storybook`, `ios-simulator`, or `window`. |
| `label` | all | no | The lens button's label. Defaults to the name in sentence case. |
| `base` | `url` | yes | The app's origin and optional base path, starting with `http://` or `https://`. Screen paths are appended to it. A trailing slash is removed. |
| `url` | `storybook` | yes | Storybook's origin, starting with `http://` or `https://`, or `auto` to [detect a running Storybook](storybook.md#detect-the-port-with-url-auto). |
| `device` | `ios-simulator` | no | `booted` (default) for every booted Simulator, or one exact device name or UDID. |
| `app` | `window` | yes | The macOS application whose window is streamed: its bundle ID, or part of it, such as `com.example.app`. Letters, digits, dots, and hyphens only. |
| `root` | all but `workbench` | no | The folder where this implementation's code lives, relative to `workbench.yaml` or absolute. Needed for [code pointers](#code-pointers) and Storybook source paths. Must be a path, not a URL. |
| `catalog` | `storybook`, `ios-simulator` | no | Import screens automatically. See [Catalogs](#catalogs). |
| `start` | `url`, `storybook` | no | A command that starts the implementation in VS Code. See [Start commands](#start-commands). |

A `workbench` implementation shows this project's
[TypeScript previews](#previews). It takes no address, `start`, or `root`: its
root is always the project root, and any other `root` is reported and the
implementation dropped. A `catalog` or `start` on a kind that doesn't support
it is reported and ignored.

Guides: [TypeScript previews](workbench-previews.md#configure-discovery-and-lenses),
[URL implementations](lenses.md), [Storybook](storybook.md),
[iOS Simulator](ios-simulator.md), [App windows](windows.md).

## A screen's implementations

A screen lists the implementations it exists in, and where. The value depends
on the implementation's kind.

**`url`**: one path for every state, or a map of the screen's state ids to
paths. Paths start with `/` and are appended to `base`.

```yaml
implementations:
  staging: /sign-in
  dev:
    default: /sign-in
    error: /sign-in?error=1
```

- In a map, the first state's id or `default` gives the path for the page as
  authored, and it is required.
- A state without its own entry uses the default path.
- A map key that isn't one of the screen's state ids is reported.

**`workbench`**: a preview ID, such as `components/button`: kebab-case
segments separated by `/`. Each of the screen's states opens the preview state
with the same id, and any other state opens the preview's first state. The
preview's source file is added to the screen's code pointers.

```yaml
implementations:
  preview: components/button
```

An ID that matches no discovered preview is reported.

**`storybook`**: a story title, exactly as Storybook shows it, including its
prefix. `Components/Button` and `Button` are different titles.

```yaml
implementations:
  storybook: Components/Button
```

**`ios-simulator`**: a device name or UDID.

```yaml
implementations:
  simulator: iPhone 16 Pro
```

**`window`**: the window's title, or part of it, ignoring case. When several
of the app's windows match, the largest is streamed.

```yaml
implementations:
  emulator: Example Phone
```

A screen may only name implementations declared at the top level.

## Code pointers

`code` maps an implementation to the path, or list of paths, of this screen's
code. Paths are relative to that implementation's `root`, or absolute.

```yaml
code:
  dev: src/routes/sign-in
  storybook:
    - src/button/button.tsx
    - src/button/button.css
```

The canvas's **Open the source** menu lists the design file and every code
pointer, and opens one in the editor. Handoffs include their absolute paths, so
an agent edits the implementation the screenshot shows. A code pointer for an
implementation without `root` can't be resolved, and the
[config route](troubleshooting.md#read-the-resolved-config) says so. See
[Code pointers](lenses.md#point-at-the-code).

## Start commands

A `url` or `storybook` implementation can name a command that brings it up.
In a trusted VS Code workspace, Workbench checks whether the implementation is
answering, and runs the command in a visible terminal only when it isn't.

```yaml
start:
  command: pnpm storybook
  cwd: apps/storybook
  check:
    port: 6006
  ready:
    url: http://localhost:6006/index.json
  timeout: 90
```

| Key | Type | Required | Description |
| --- | --- | --- | --- |
| `command` | string | yes | A shell command. |
| `cwd` | path | no | Where to run it, relative to `workbench.yaml`. Defaults to `.`. |
| `check` | probe | yes | How to tell whether it is already running. |
| `ready` | probe | no | How to tell that it has finished starting. Defaults to `check`. |
| `timeout` | integer, 1–300 | no | Seconds to wait for `ready`. Defaults to 60. |

A **probe** has exactly one of:

| Key | Description |
| --- | --- |
| `port` | A TCP port, 1–65535. It is ready when something accepts a connection. Add `host` to check somewhere other than `127.0.0.1`. |
| `url` | An `http://` or `https://` URL. It is ready when the URL answers with a successful response. |

Use `port` for `check` when you only need to know something is listening, and
a `url` for `ready` when the server listens before it can serve, as Storybook
does while it builds. If `ready` times out, the canvas still opens and reports
the problem. Start commands never run in untrusted workspaces or outside
VS Code. See [Start the server automatically](lenses.md#start-the-server-automatically).

## Catalogs

`catalog` imports screens instead of listing them by hand.

- On a **Storybook** implementation it imports every story title as a screen,
  with its stories as states. See [Import the whole catalog](storybook.md#import-the-whole-catalog).
- On an **iOS Simulator** implementation it imports each matching booted
  device as a screen. See [iOS Simulator](ios-simulator.md).

`catalog: true` turns it on. A map turns it on and sets icons:

```yaml
catalog:
  icon: book-open            # fallback for imported screens
  icons:                     # Storybook title prefix -> icon; the longest match wins
    UI: palette
    UI/Components: component
    UI/Components/Button: mouse-pointer-click
```

The default icon is `book-open` for Storybook and `smartphone` for the
Simulator. Other kinds don't take `catalog`; TypeScript previews are
[discovered](#previews) without one.

## Local overrides

`workbench.local.yaml` beside `workbench.yaml` holds what belongs to one
machine: a port, an absolute `root`, a different start command. Add it to
`.gitignore`.

```yaml
# workbench.local.yaml
implementations:
  dev:
    base: http://localhost:4000
    root: /Users/me/code/acme-web
```

It is merged over `workbench.yaml`:

- `implementations` merges one level deep. Each implementation's keys
  replace the committed ones by name, so the example above changes `dev`'s
  `base` and `root` and keeps its `kind` and `start`. A `start` block is
  replaced whole.
- Every other top-level key, such as `name` or `sections`, replaces the
  committed one whole.

A missing local file is normal. A local file that doesn't parse is reported by
its own name.

## YAML that the reader accepts

Workbench reads a subset of YAML with its own small parser, the same in the
browser and the server:

- Block mappings, block lists, strings, numbers, `true`, `false`, `null`,
  quotes, and comments.
- No flow collections (`{a: b}`, `[a, b]`), multi-line strings, anchors,
  aliases, tags, or multiple documents.
- Indent with spaces. Tabs are rejected.
- `#` starts a comment at the start of a line or after a space, outside quotes.
  Quote a value that contains a space followed by `#`. `"#00a1ff"` and `a#b`
  are values.

A line the parser doesn't understand is reported with its line number, and
nothing loads until it is fixed.

## How problems are reported

Workbench reports what it can't use rather than guessing, and builds
everything else:

- A screen, folder, section, state, or implementation that is invalid is
  dropped. Its problem names where it was, such as
  `Pages › Auth › Sign in: state id “Error” must be kebab-case`.
- In the canvas, every problem found while reading the file is written to the
  browser console.
- `GET /_workbench/config` on the workbench server lists, under `problems`,
  problems with implementations, previews, viewports, screens' implementation
  mappings, and `code`, plus catalogs and previews that couldn't load. It sits
  alongside the config as your machine resolves it. A screen dropped for a bad
  `src`, label, state id, or folder is left out of it without a problem, so
  check the console for those. See
  [Troubleshooting](troubleshooting.md#read-the-resolved-config).
- When previews or a catalog are on, the Workbench view in VS Code shows the
  same problems above its screen list.

## Editing with the form

**Configure pages**, in the canvas toolbar's **More** menu, edits sections,
folders, screens, labels, source paths, and viewports in a form. See
[Configure pages](canvas.md#configure-pages). Saving rewrites only the
`sections` block of `workbench.yaml`. Implementations, states, implementation
mappings, and code pointers are preserved. Comments outside `sections` are
kept; comments inside it are not.
