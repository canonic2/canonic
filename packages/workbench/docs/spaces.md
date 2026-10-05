# Several spaces

One Workbench can hold several spaces, such as a product and its design
system, and switch between them without opening another window. Each one
keeps its own configuration, pages, server, and port, as if it were open on
its own.

With one space, nothing changes: the window's space is the workbench, and you
never need to add anything.

## Describe several spaces in one file

A `workbench.yaml` is one space, unless it lists several under `spaces`.
Their folders don't matter: spaces can share the file's folder, or each serve
a folder of its own with `root`.

```yaml
spaces:
  web:
    name: Acme Web
    collections:
      - name: Pages
        items:
          - label: Sign in
            src: pages/sign-in.html
  design-system:
    name: Acme Design System
    root: packages/ui
    collections:
      - name: Components
        items:
          - label: Button
            src: button.html
```

Keys outside `spaces`, such as `implementations` or `previews`, are shared
by every space. See [Spaces](configuration.md#spaces) in the reference for
how sharing and `root` work.

## Where spaces come from

Workbench reads the `workbench.yaml` files of two kinds of folder, and lists
every space they describe:

- **The window's folders.** Every open folder with a `workbench.yaml` at its
  root, in the workspace's folder order. In a multi-root workspace, each such
  folder contributes its spaces.
- **Folders you add.** Folders elsewhere on disk that you add with **Add a
  space…**. VS Code remembers them across windows and restarts, so their
  spaces appear in every window.

The window's folders come first, and each file's spaces are listed in the
order it gives them. A folder that is both open in the window and added is
listed once, as the window's. Adding, removing, or renaming a space in a
`workbench.yaml` updates the switcher as soon as you save.

## Switch spaces

In VS Code, the space switcher is at the top of the Workbench view: the
space's mark and name, then a menu of every space when you select it. The
current space is checked. Pick another one, and the sidebar and the canvas
tab both change to it.

Use Enter, Space, or an arrow key to open the switcher from the keyboard. Up and
Down move through its actions; Home and End jump to the ends. Escape closes the
menu and returns focus to the switcher. Tab closes it and moves to the next
control; Shift+Tab moves to the previous control.

You can also run **Workbench: Switch Space…** from the Command Palette.

When there's more than one space, the breadcrumb in the top bar starts with
the space's mark and name, before the page. Select it to switch from there.

Each window remembers the space it last showed. Each space has its own canvas
settings, such as the last page, size, and lens, because each runs on its own
address.

## Name, color, and icon

Each space is shown with its name and a mark. By default the mark is the
first letter of the name on a colored square, with the color chosen from the
space's folder, so a space looks the same everywhere. Set any of the three at
the top of `workbench.yaml`:

```yaml
name: Acme Design System
color: purple
icon: palette
```

- `name` is the space's name in the switcher, the sidebar, and the browser
  tab. Without one, the switcher uses the folder's name.
- `color` is one of `blue`, `green`, `orange`, `purple`, `pink`, `teal`,
  `red`, `yellow`, or `gray`, or a hex value in quotes, such as `"#f5d76e"`.
  The letter or icon turns dark on a light color.
- `icon` is a [Lucide](https://lucide.dev/icons/) icon name, such as `palette`,
  or an image in the project, such as `brand/logo.svg`. Images can be `.svg`,
  `.png`, `.jpg`, `.webp`, or `.gif`, up to 256 KB. An image is shown as it is,
  on no color, unless you also set `color`.

A value Workbench can't use is reported in the problems list, and the
default takes its place. So is an image that isn't in the project.

To mark a space differently on your machine only, such as two checkouts of
the same project, set `name`, `color`, or `icon` in
[`workbench.local.yaml`](configuration.md#local-overrides). Changes show in
the switcher as soon as you save either file.

## Add a space

1. Open the space switcher and select **Add a space…**, or run
   **Workbench: Add Space…**.
2. Choose the folder that holds the space's `workbench.yaml`.

Workbench adds every space the folder's `workbench.yaml` describes and
switches to the first. A folder without a `workbench.yaml` isn't added. Write
one first; see [Getting started](getting-started.md#2-write-workbenchyaml).

The name in the switcher is the space's `name`, or, without one, the folder's
name for a single-space file and the id for a space under `spaces`. A
`workbench.yaml` with errors is listed as one space under the folder's name,
so you can open it and read the error.

## Remove a space

Hover over an added space in the switcher, or move to it with the arrow keys,
and select **×** (**Remove from Workbench**). What you added is a folder, so
this removes the folder: every space its `workbench.yaml` describes leaves
the list in every window, and their servers stop. It doesn't change the
folder. To drop one space of several, delete its entry from `spaces`.

A window's own folders can't be removed from the switcher. Close the folder,
or remove it from the workspace, and its spaces leave the list.

If you remove or rename an added folder's `workbench.yaml`, its spaces stay
hidden until the file is back.

## Servers and start commands

Each space runs on its own [server](extension.md#the-server), with its own
port, [start commands](configuration.md#start-commands), and preview worker.
All spaces in a window share one [screenshot helper](extension.md#the-screenshot-helper).

- A window's own space starts with the window.
- Any other space starts the first time you switch to it, and keeps running
  after you switch away, so switching back is immediate.
- Removing a space, deleting it from its `workbench.yaml`, or closing the
  window stops its server. Changing its `root` restarts it there the next
  time it opens.

A space's start commands run in terminals of the window you switched in,
with that space's root as their starting point. Adding a space trusts its
folder the way you trust the open workspace: its start commands,
[TypeScript previews](workbench-previews.md), and
[docs page](docs-pages.md) examples run project code. Add only folders you
trust.

Workbench appears in the activity bar of any window once you've added a
space, even a window with no `workbench.yaml` of its own. In that case,
nothing starts until you open the Workbench view or the canvas.

## Handoffs, files, and agents

**More** › **Export…** exports a page, selected pages, a collection, or the whole
current space as a source ZIP, PDF, images, or portable viewer. Export each space separately by
switching to it before downloading. An included `workbench.yaml` may declare
several spaces, but the export's page records and reference screenshots cover
only the current one. See [Design-system export](design-system-export.md).

Everything stays in the space it belongs to:

- **Copy handoff** saves its screenshot in the current space's
  `.canonic/.handoffs/`.
- **Open the source** opens files from the current space's configuration.
- **Workbench: Open Canvas in Browser** and **Workbench: Copy Canvas URL** use
  the current space's address.
- Agents read the current view from the server of the space they work in.
  When several spaces share a root, agents there read the one you switched
  to last. See [Copy a reference](canvas.md#copy-a-reference).

## In a browser

The standalone server takes several folders:

```sh
node path/to/server.js path/to/product path/to/design-system
```

A folder whose `workbench.yaml` lists several spaces serves them all, so
`node path/to/server.js .` is enough for a file with `spaces`. It starts
every space, each on its own port, and prints one line per space:

```text
workbench on http://127.0.0.1:3579/_workbench/  Acme
workbench on http://127.0.0.1:3580/_workbench/  Acme Design System
```

Open any of the addresses. With more than one space, the browser canvas
shows the space switcher at the top of its sidebar, and the space in the top
bar's breadcrumb. Switching goes to the other space's address. Adding and
removing spaces is available only in VS Code.

**Workbench: Open Canvas in Browser** shows the same switcher for the spaces
in the VS Code window. Switching there changes the browser tab only; the
editor stays on its space.

To find the server, see [Running without VS Code](extension.md#running-without-vs-code).

## Keyboard

In the space switcher:

| Key | Does |
| --- | --- |
| **Enter**, **Space**, **Down**, or **Up** on the switcher | Opens the menu with the current space focused |
| **Up**, **Down** | Move through the spaces, **Remove** buttons, and **Add a space…** |
| **Home**, **End** | Jump to the first or last entry in the menu |
| **Enter** or **Space** | Switch to the focused space, or run the focused button |
| **Escape** | Closes the menu |
