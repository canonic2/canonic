# Using the canvas

The canvas shows one page at a time, on an artboard at a real device size, on
a surface you can zoom and pan like a design tool. In a
[docs lens](docs-pages.md), a page's docs fill the canvas instead; see
[Docs on the canvas](#docs-on-the-canvas). This page covers each part of it.

## Layout

| Part | Where | Does |
| --- | --- | --- |
| Sidebar | The **Workbench** view in VS Code's activity bar, or the left edge of a standalone browser window | The space switcher, the collection list, the page list, and search |
| Space switcher | Top of the sidebar | The space showing, and the others to switch to. In VS Code it's always there, for **Add a space…**; in a browser, only with more than one space. See [Several spaces](spaces.md). |
| Collection list | Sidebar, under the heading **Collections** | The space's collections |
| Page list | Sidebar | The chosen collection's groups, pages, and states |
| Search | Bottom of the sidebar | Filters every collection's pages, groups, and states |
| Top bar | Top of the canvas | The breadcrumb with the state switcher, lenses, size, actions, reload, source, reference, and more |
| Artboard | Middle | The page, labeled with its name and size by the artboard label |
| Toolbar | Floating at the bottom | Annotation tools, undo, clear, screenshot, and handoff |
| View controls | Floating at the bottom right | Recenter view, zoom out, the zoom level and its menu, and zoom in |

Inside VS Code, the sidebar is where a file tree usually goes, in your
editor's theme, and the canvas tab doesn't repeat it.

## The sidebar

- **The collection list**, headed **Collections**, is at the top. Pick a
  collection to show its pages in the page list.
- **Groups** in the page list hold related pages. Select a group to expand or
  collapse it.
- **Pages** with two or more states expand to show them. Pick a state to show
  it; the first is the page as authored. Selecting the page itself shows its
  first state, and selecting it again while it is showing collapses it. A
  [Markdown page](docs-pages.md#a-markdown-page) has nothing listed under it.
- **Search**, the field at the bottom (*Search pages…*), filters every
  collection's pages, groups, and states as you type, and groups the results
  by collection.
- [TypeScript previews](workbench-previews.md) and Markdown pages defined in
  `*.workbench.ts` files appear in the collections their titles name, beside the
  pages listed in `workbench.yaml`.

In VS Code, the page list is one stop in the tab order. **Up** and **Down**
move through its groups, pages, and states, **Right** expands a group or page,
**Left** collapses it, **Home** and **End** jump to the first and last, and
**Enter** or **Space** shows the focused page or state. The problems list,
above the page list, shows problems importing a catalog or TypeScript previews.

In a standalone browser, ⌘K or Ctrl+K jumps to search. Drag the sidebar's edge
to resize it, focus the edge and use the arrow keys, or double-click it to
reset the width. The width is remembered.

## The top bar

| Control | Does |
| --- | --- |
| **Breadcrumb: space** | The space's mark and name, before the page. Select it to switch to another [space](spaces.md). Shown when there is more than one space. |
| **Breadcrumb: page and state** | The page's name, then the state showing. The state is the state switcher: select it to pick another state. On a [Storybook lens](storybook.md), it picks a story of the current title instead. Shown when there is more than one to choose from. |
| **Lens switcher** | Switches between the authored page and each [implementation](lenses.md) the page has. Shown when the page has two or more lenses. The authored lens defaults to **Design**, or **Workbench** for a TypeScript preview. [Every label is customizable](lenses.md#customize-lens-labels). A page with [docs](docs-pages.md) also lists its docs lenses, one for each renderer of its examples. |
| **Size switcher** | **Fit**, **Laptop**, **Mobile**, and **Resizable**. See [Artboard sizes](#artboard-sizes). Disabled in a docs lens, because the docs fill the canvas. |
| **Actions** (pointer icon and switch) | Lets links navigate and forms submit in the page. Off by default. In a Workbench preview, links open the previews they're mapped to and everything else is recorded under **Actions**; see [Links and navigation](preview-data.md#links-and-navigation). Through a lens whose page is served by something other than the workbench, such as a URL or Storybook lens, it is always on and can't be switched. See [Links and actions](pages-and-states.md#links-and-actions). |
| **Reload** | Reloads the current page. |
| **Open the source** (`</>`) | Lists the design file and the page's [code pointers](lenses.md#point-at-the-code), and opens one in the editor. For a TypeScript preview, it lists the preview definition and its source file; for a page with docs, its design file, then **Docs**, its Markdown, then each docs lens's example source. On a Markdown page, **Docs** is the first row. A path that isn't on this machine is listed but can't be opened. |
| **Copy reference** | Copies a short text reference to the current view. See [Copy a reference](#copy-a-reference). |
| **Open on its own** | Opens the current page in your browser, outside the workbench. |
| **Preview controls** | Opens the inputs, **Reset state**, action log, and documentation of a TypeScript preview. Shown once the preview is ready. See [Preview data, mocks, and actions](preview-data.md). |
| **More** | **Configure pages** and **Download design-system ZIP**, which [exports the whole current space](design-system-export.md) across its collections and pages. Shown when the workbench server is running. |

When the canvas is narrow, **Reload**, **Open the source**, **Copy reference**,
**Open on its own**, and **Preview controls** fold into **More** rather than
overlapping.

Your choices of lens, size, and actions are remembered across pages and
sessions. A page that doesn't support a remembered choice uses its own
default: the Design lens, a Markdown page's own docs lens, or the first size
it supports.

## Artboard sizes

The size switcher sets the artboard's size:

| Button | Size | Notes |
| --- | --- | --- |
| **Fit** | The available canvas | Follows the canvas as you resize the editor. |
| **Laptop** | 1512 × 982 | A 14-inch MacBook Pro. |
| **Mobile** | 393 × 852 | An iPhone 15 Pro. |
| **Resizable** | 1024 × 768 to start, at least 320 × 320 | Drag the artboard's left, right, or bottom edge, or a bottom corner. The last size is kept. |

The page always lays out at the artboard's real size. An artboard larger than
the canvas is zoomed out to fit, never squeezed, so a 1512-pixel layout is
still 1512 pixels wide to the page. Screenshots are always taken at the
artboard's real size, whatever the zoom.

A page's [`sizes`](pages-and-states.md#sizes) decide which sizes are
enabled.

## Zoom and pan

The canvas behaves like Figma's:

| Input | Does |
| --- | --- |
| ⌘ or Ctrl + scroll wheel, or pinch | Zoom at the pointer |
| Scroll wheel on the canvas around the artboard, or Shift + wheel there | Pan. Over the page, the wheel scrolls the page. |
| Space + drag, or middle-button drag | Pan, anywhere on the canvas |
| ⌘ or Ctrl + `=` (or `+`), and ⌘ or Ctrl + `−` | Zoom in and out, in powers of two |
| ⌘ or Ctrl + 0, or Shift 0 | Zoom to 100% |
| Shift 1 | Zoom to fit |

The view controls have **Zoom out** and **Zoom in** buttons on either side of
the current level. Select the level for a menu with the same commands plus 50%
and 200%. **Recenter view**, beside the zoom buttons, brings the artboard back
to the middle of the canvas without changing the zoom level or the page's
scroll position.

Until you zoom or pan by hand, the artboard stays fitted: changing size or
resizing the editor fits it again. Fitting never magnifies past 100%.

These shortcuts work while a page has focus, because the workbench reads them
inside the pages it serves, and through Storybook's key channel for a Storybook
lens. A page from another origin, such as a URL lens, keeps its own scroll
wheel.

## Docs on the canvas

The canvas has two modes, and the lens decides which. In the default canvas
mode, the page is on an artboard. In a [docs lens](docs-pages.md#lenses), the
canvas is in its docs mode: there is no artboard, the page's docs fill the
canvas, at most 960 pixels wide and centered, and the wheel scrolls them like
a page in a browser. The size switcher stays visible but is disabled.
Switching back to the design lens restores the artboard at its size. Zoom,
annotations, screenshots, and handoffs work in a docs lens too; see
[On the canvas](docs-pages.md#on-the-canvas).

## Keyboard and clipboard in the editor

Inside VS Code, pages behave like pages:

- Typing, caret movement, selection, copy, cut, paste, select all, and undo
  stay in the focused field or page.
- Editor shortcuts, such as the Command Palette or switching tabs, reach
  VS Code, including multi-key chords.
- Right-clicking a design page opens VS Code's context menu with the page's
  selected text.
- Select all works in a field, a note, or the page, but not over the top bar
  or the toolbar.

Pages from another origin, such as a URL lens, can't receive the editor's
menu or clipboard integration. Storybook doesn't forward keys while an input in
the story is focused. Use **Open on its own** when you need full native
behavior.

## Keyboard shortcuts

Workbench adds no VS Code keybindings; these keys work inside the canvas and
the sidebar. Use Ctrl where a shortcut shows ⌘ on Windows and Linux.

### Canvas keys

| Keys | Does |
| --- | --- |
| ⌘ `=` or ⌘ `+` | Zoom in |
| ⌘ `−` | Zoom out |
| ⌘ 0, or Shift 0 | Zoom to 100% |
| Shift 1 | Zoom to fit |
| Space + drag | Pan |
| Escape | Close an open menu |

Shift 0 and Shift 1 don't apply while you type in a field or a note.

### Annotation keys

| Keys | Does |
| --- | --- |
| Escape | Leave the drawing tool for **Select**; with **Select**, deselect the annotation; while typing a note, finish it |
| ⌘ Z | Undo the last annotation, unless a field, a note, or the page has focus |
| Delete or Backspace | Remove the selected annotation |
| Arrow keys, Shift + arrow keys | Nudge the selected annotation 1 or 8 pixels |
| Shift while drawing | Keep arrows and lines at 45° steps, and rectangles and circles square |
| Shift while dragging a corner handle | Keep a rectangle or circle square |
| Double-click a note or comment | Edit its text, with **Select** |

### Sidebar keys

| Keys | Where | Does |
| --- | --- | --- |
| ⌘ K | Standalone browser | Jump to search |
| Escape | In search | Clear it |
| Down | In search, in VS Code | Move to the first result in the page list |
| Up, Down, Home, End | In the page list, in VS Code | Move between groups, pages, and states |
| Right, Left | In the page list, in VS Code | Expand or collapse a group or page; Left on a page or state inside one moves to it |
| Enter or Space | On a page or state | Show it |
| Left, Right, Shift + Left or Right | On the sidebar's edge, in a standalone browser | Resize the sidebar by 8 or 32 pixels |

## Links and the address

The current view is recorded in the address's hash, so reloading or opening a
copied link lands on the same page, state, size, and lens:

```text
#pages/sign-in.html:error@1512~staging
 └ src              └ state └ width └ lens
```

| Part | Values | Left out when |
| --- | --- | --- |
| src | The page's `src` | Never |
| `:state` | A state id, or on a Storybook lens the part of the story's id after `--` | The first state is showing, or the page has no states |
| `!example` | In a docs lens, an example's ID; the docs open scrolled to that example | No example is named, or the lens isn't a docs lens |
| `@width` | `fit`, `1512`, `393`, or `resizable` | In a docs lens, which has no width. Elsewhere the workbench always writes it, though a link you type may omit it |
| `~lens` | An implementation name | The Design lens is showing, or on a Markdown page, its own docs lens |

The src is always the page's `src`, also in a docs lens of a page with a
design. See [Addresses](docs-pages.md#addresses) for examples of addresses in a
docs lens.

After a page loads, the address is rewritten to describe exactly what is
showing.

To get a link:

- **Workbench: Copy Canvas URL** copies the workbench's address.
- **Workbench: Open Canvas in Browser** opens it in your browser.
- In a standalone browser, copy the address bar.

The workbench only listens on `127.0.0.1`, so a link works on your machine
while the workbench is running. The port is the first free one from 3579, so
with several windows or spaces open, a saved link may point at another space
after a restart; the hash part stays valid. Share a
[reference](#copy-a-reference) or a screenshot with other people instead.

## Copy a reference

**Copy reference** copies a few lines of text that identify the current view:
the page's label and design file, its state or story, and the lens with its
implementation URL. Paste it into a conversation to point an agent or a
colleague at a page. It doesn't take a screenshot or include annotations. For
those, use a [handoff](annotations-and-handoff.md#hand-off-to-an-agent).

Agents can also read the same reference without a paste. Every open canvas
tells the Workbench server which page it shows, and
`GET /_workbench/view` on the server returns the reference from the canvas
you changed most recently. Projects configured with Canonic's Shield can give
it to Claude Code and Codex with every prompt, and to MCP clients through a
`current_view` tool. A project with a `.canonic` folder gets the server's
address in `.canonic/.workbench/server.json`; add that folder to
`.gitignore`.

## Configure pages

**More** › **Configure pages** edits the collections and their pages in a
form:

- Add, remove, and rename collections, and set their icons.
- Rename and remove groups, and add pages to them.
- Add and remove pages, and edit their labels and source paths.
- Choose each page's supported sizes.

Saving rewrites only the `collections` block of `workbench.yaml`, and the
sidebar refreshes. States, implementation mappings, and code pointers in that
block are preserved. Everything outside it, including comments, is left alone;
comments inside it are removed. The form edits `workbench.yaml` only, so
collections that `workbench.local.yaml` overrides on your machine keep showing
the local version. Editing the YAML directly always works too; see the
[reference](configuration.md).

## Using a standalone browser

The canvas also works in a regular browser:

- From VS Code, run **Workbench: Open Canvas in Browser**. The extension still
  serves the page, so handoffs and opening files in the editor work too.
- Without VS Code, run the server yourself. See
  [Running without VS Code](extension.md#running-without-vs-code). Everything
  works except handoffs, opening files in the editor, and
  [start commands](configuration.md#start-commands).

In a browser, the sidebar is on the left of the page, browser shortcuts and
menus work normally, and the camera downloads screenshots. Problems importing
a catalog or TypeScript previews are listed only in VS Code's sidebar.
