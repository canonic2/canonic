# Several projects

One Workbench can hold several projects, such as a product and its design
system, and switch between them without opening another window. Each one
keeps its own configuration, screens, server, and port, as if it were open on
its own.

With one project, nothing changes: the window's project is the workbench, and
you never need to add anything.

## Describe several projects in one file

A `workbench.yaml` is one project, unless it lists several under `projects`.
Their folders don't matter: projects can share the file's folder, or each
serve a folder of its own with `root`.

```yaml
projects:
  web:
    name: Acme Web
    sections:
      - name: Pages
        items:
          - label: Sign in
            src: pages/sign-in.html
  design-system:
    name: Acme Design System
    root: packages/ui
    sections:
      - name: Components
        items:
          - label: Button
            src: button.html
```

Keys outside `projects`, such as `implementations` or `previews`, are shared
by every project. See [Projects](configuration.md#projects) in the reference
for how sharing and `root` work.

## Where projects come from

Workbench reads the `workbench.yaml` files of two kinds of folder, and lists
every project they describe:

- **The window's folders.** Every open folder with a `workbench.yaml` at its
  root, in the workspace's folder order. In a multi-root workspace, each such
  folder contributes its projects.
- **Folders you add.** Folders elsewhere on disk that you add with **Add a
  project…**. VS Code remembers them across windows and restarts, so their
  projects appear in every window.

The window's folders come first, and each file's projects are listed in the
order it gives them. A folder that is both open in the window and added is
listed once, as the window's. Adding, removing, or renaming a project in a
`workbench.yaml` updates the switcher as soon as you save.

## Switch projects

In VS Code, the project switcher is at the top of the Workbench view: the
project's mark and name, then a menu of every project when you select it. The
current project is checked. Pick another one, and the screen list and the
canvas tab both change to it.

You can also run **Workbench: Switch Project…** from the Command Palette.

When there's more than one project, the canvas toolbar starts with the
project's mark and name, before the screen. Select it to switch from there.

Each window remembers the project it last showed. Each project has its own
canvas settings, such as the last screen, width, and lens, because each runs
on its own address.

## Name, color, and icon

Each project is shown with its name and a mark. By default the mark is the
first letter of the name on a colored square, with the color chosen from the
project's folder, so a project looks the same everywhere. Set any of the
three at the top of `workbench.yaml`:

```yaml
name: Acme Design System
color: purple
icon: palette
```

- `name` is the project's name in the switcher, the screen list, and the
  browser tab. Without one, the switcher uses the folder's name.
- `color` is one of `blue`, `green`, `orange`, `purple`, `pink`, `teal`,
  `red`, `yellow`, or `gray`, or a hex value in quotes, such as `"#f5d76e"`.
  The letter or icon turns dark on a light color.
- `icon` is a [Lucide](https://lucide.dev/icons/) icon name, such as `palette`,
  or an image in the project, such as `brand/logo.svg`. Images can be `.svg`,
  `.png`, `.jpg`, `.webp`, or `.gif`, up to 256 KB. An image is shown as it is,
  on no color, unless you also set `color`.

A value Workbench can't use is listed as a problem above the screens, and the
default takes its place. So is an image that isn't in the project.

To mark a project differently on your machine only, such as two checkouts of
the same project, set `name`, `color`, or `icon` in
[`workbench.local.yaml`](configuration.md#local-overrides). Changes show in
the switcher as soon as you save either file.

## Add a project

1. Open the project switcher and select **Add a project…**, or run
   **Workbench: Add Project…**.
2. Choose the folder that holds the project's `workbench.yaml`.

Workbench adds every project the folder's `workbench.yaml` describes and
switches to the first. A folder without a `workbench.yaml` isn't added. Write
one first; see [Getting started](getting-started.md#2-write-workbenchyaml).

The name in the switcher is the project's `name`, or, without one, the folder's
name for a single-project file and the id for a project under `projects`. A
`workbench.yaml` with errors is listed as one project under the folder's name,
so you can open it and read the error.

## Remove a project

Hover over an added project in the switcher, or move to it with the arrow keys,
and select **×** (**Remove from Workbench**). What you added is a folder, so
this removes the folder: every project its `workbench.yaml` describes leaves
the list in every window, and their servers stop. It doesn't change the
folder. To drop one project of several, delete its entry from `projects`.

A window's own folders can't be removed from the switcher. Close the folder,
or remove it from the workspace, and its projects leave the list.

If you remove or rename an added folder's `workbench.yaml`, its projects stay
hidden until the file is back.

## Servers and start commands

Each project runs on its own [server](extension.md#the-server), with its own
port, [start commands](configuration.md#start-commands), and preview worker.
All projects in a window share one [screenshot helper](extension.md#the-screenshot-helper).

- A window's own project starts with the window.
- Any other project starts the first time you switch to it, and keeps running
  after you switch away, so switching back is immediate.
- Removing a project, deleting it from its `workbench.yaml`, or closing the
  window stops its server. Changing its `root` restarts it there the next
  time it opens.

A project's start commands run in terminals of the window you switched in,
with that project's root as their starting point. Adding a project trusts
it the way you trust the open workspace: its start commands and
[TypeScript previews](workbench-previews.md) run project code. Add only
folders you trust.

Workbench appears in the activity bar of any window once you've added a
project, even a window with no `workbench.yaml` of its own. In that case,
nothing starts until you open the Workbench view or the canvas.

## Handoffs, files, and agents

Everything stays in the project it belongs to:

- **Copy handoff** saves its screenshot in the current project's
  `.canonic/.handoffs/`.
- **Open the source** opens files from the current project's configuration.
- **Workbench: Open Canvas in Browser** and **Workbench: Copy Canvas URL** use
  the current project's address.
- Agents read the current view from the server of the project they work in.
  When several projects share a root, agents there read the one you switched
  to last. See [Copy a reference](canvas.md#copy-a-reference).

## In a browser

The standalone server takes several project folders:

```sh
node path/to/server.js path/to/product path/to/design-system
```

A folder whose `workbench.yaml` lists several projects serves them all, so
`node path/to/server.js .` is enough for a file with `projects`. It starts
every project, each on its own port, and prints one line per project:

```text
workbench on http://127.0.0.1:3579/_workbench/  Acme
workbench on http://127.0.0.1:3580/_workbench/  Acme Design System
```

Open any of the addresses. With more than one project, the browser canvas
shows the project switcher at the top of its screen list, and the project in
the toolbar. Switching goes to the other project's address. Adding and
removing projects is available only in VS Code.

**Workbench: Open Canvas in Browser** shows the same switcher for the projects
in the VS Code window. Switching there changes the browser tab only; the
editor stays on its project.

To find the server, see [Running without VS Code](extension.md#running-without-vs-code).

## Keyboard

In the project switcher:

| Key | Does |
| --- | --- |
| **Enter**, **Space**, **Down**, or **Up** on the switcher | Opens the menu with the current project focused |
| **Up**, **Down** | Move through the projects, **Remove** buttons, and **Add a project…** |
| **Home**, **End** | Jump to the first or last item |
| **Enter** or **Space** | Switch to the focused project, or run the focused button |
| **Escape** | Closes the menu |
