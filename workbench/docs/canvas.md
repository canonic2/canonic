# Using the canvas

The canvas shows one screen at a time, in a frame at a real device size, on a
surface you can zoom and pan like a design tool. This page covers each part of
it.

## Layout

| Part | Where | Does |
| --- | --- | --- |
| Screen list | The **Workbench** view in VS Code's activity bar, or the left edge of a standalone browser window | Sections, then the chosen section's folders, screens, and states, with a filter |
| Toolbar | Top of the canvas | State, actions, lens, width, reload, source, reference, and more |
| Frame | Middle | The screen, labeled with its name and size |
| Markup dock | Floating at the bottom | Drawing tools, undo, clear, screenshot, and handoff |
| Zoom control | Floating at the bottom | Zoom level and zoom menu |

Inside VS Code, the screen list is in the sidebar where a file tree usually
goes, in your editor's theme, and the canvas tab doesn't repeat it.

## The screen list

- **Sections** are listed at the top. Pick one to show its screens.
- **Folders** group screens. Select a folder to expand or collapse it.
- **Screens** with two or more states expand to show them. Pick a state to show
  it; the first is the page as authored.
- **The filter** at the bottom searches screens and states as you type.
- Arrow keys move through the list. **Right** expands a folder or screen,
  **Left** collapses it, and **Home** and **End** jump to the first and last
  rows.

In a standalone browser, drag the list's edge to resize it, or focus the edge
and use the arrow keys. The width is remembered.

## The toolbar

| Control | Does |
| --- | --- |
| **State** | Picks a state of the current screen. Shown when the screen has states. |
| **Actions** | Lets links navigate and forms submit in the page. Off by default. See [Links and actions](pages-and-states.md#links-and-actions). |
| **Lens** | Switches between **Design** and each [implementation](lenses.md) the screen has. Shown when it has any. |
| **Story** | Picks a story of the current Storybook title. Shown on a [Storybook lens](storybook.md). |
| **Widths** | **Fit**, **Laptop**, **Mobile**, and **Resizable**. See [Frame widths](#frame-widths). |
| **Reload** | Reloads the current page. |
| **Open the source** (`</>`) | Lists the design file and the screen's [code pointers](lenses.md#point-at-the-code), and opens one in the editor. |
| **Copy reference** | Copies a short text reference to the current view. See [Copy a reference](#copy-a-reference). |
| **Open on its own** | Opens the current page in your browser, outside the workbench. |
| **More** | **Configure pages** and **Download design-system ZIP**. |

When the canvas is narrow, secondary controls fold into **More** rather than
overlapping.

Your choices of lens, width, and actions are remembered across screens and
sessions. A screen that doesn't support a remembered choice uses its own
default: the Design lens, or the first width it supports.

## Frame widths

| Button | Size | Notes |
| --- | --- | --- |
| **Fit** | The available canvas | Follows the canvas as you resize the editor. |
| **Laptop** | 1512 × 982 | A 14-inch MacBook Pro. |
| **Mobile** | 393 × 852 | An iPhone 15 Pro. |
| **Resizable** | Any | Drag an outside edge or corner of the frame. The last size is kept. |

The page always lays out at the frame's real size. A frame larger than the
canvas is zoomed out to fit, never squeezed, so a 1512-pixel layout is still
1512 pixels wide to the page. Screenshots are always taken at the frame's real
size, whatever the zoom.

A screen's [`viewports`](pages-and-states.md#viewports) decide which buttons
are enabled.

## Zoom and pan

The canvas behaves like Figma's:

| Input | Does |
| --- | --- |
| ⌘ or Ctrl + scroll wheel, or pinch | Zoom at the pointer |
| Scroll wheel, or Shift + wheel | Pan |
| Space + drag, or middle-button drag | Pan |
| ⌘ = and ⌘ − | Zoom in and out, in powers of two |
| ⌘ 0 or Shift 0 | Zoom to 100% |
| Shift 1 | Zoom to fit |

The zoom control shows the current level. Its menu adds 50% and 200%.

Until you zoom or pan by hand, the frame stays fitted: changing width or
resizing the editor fits it again. Fitting never magnifies past 100%.

These shortcuts work while a page has focus, because the workbench reads them
inside the pages it serves, and through Storybook's key channel for a Storybook
lens. A page from another origin, such as a URL lens, keeps its own scroll
wheel.

## Keyboard and clipboard in the editor

Inside VS Code, pages behave like pages:

- Typing, caret movement, selection, copy, cut, paste, select all, and undo
  stay in the focused field or page.
- Editor shortcuts, such as the Command Palette or switching tabs, reach
  VS Code, including multi-key chords.
- Right-clicking a design page opens VS Code's context menu with the page's
  selected text.
- Select all works in a field, a note, or the page, but not over the toolbar.

Pages from another origin, such as a URL lens, can't receive the editor's
menu or clipboard integration. Storybook doesn't forward keys while an input in
the story is focused. Use **Open on its own** when you need full native
behavior.

## Links and the address

The current view is recorded in the address's hash, so reloading or opening a
copied link lands on the same screen, state, width, and lens:

```text
#pages/sign-in.html:error@1512~staging
 └ src              └ state └ width └ lens
```

| Part | Values | Left out when |
| --- | --- | --- |
| src | The screen's `src` | Never |
| `:state` | A state id, or a story's id after `--` on a Storybook lens | The first state is showing |
| `@width` | `fit`, `1512`, `393`, or `resizable` | Never; the workbench always writes it, though a link you type may omit it |
| `~lens` | An implementation name | The Design lens is showing |

After a screen loads, the address is rewritten to describe exactly what is
showing.

To get a link:

- **Workbench: Copy Canvas URL** copies the workbench's address.
- **Workbench: Open Canvas in Browser** opens it in your browser.
- In a standalone browser, copy the address bar.

The workbench only listens on `127.0.0.1`, so a link works on your machine
while the workbench is running. Share a [reference](#copy-a-reference) or a
screenshot with other people instead.

## Copy a reference

**Copy reference** copies a few lines of text that identify the current view:
the screen's label and design file, its state or story, and the lens with its
implementation URL. Paste it into a conversation to point an agent or a
colleague at a screen. It doesn't take a screenshot or include markup. For
those, use a [handoff](markup-and-handoff.md#hand-off-to-an-agent).

## Configure pages

**More** › **Configure pages** edits the screen list in a form:

- Add, remove, and rename sections, and set their icons.
- Add and remove screens, and edit their labels and source paths.
- Choose each screen's supported viewports.

Saving rewrites only the `sections` block of `workbench.yaml`, and the screen
list refreshes. States, implementation mappings, and code pointers in that
block are preserved. Everything outside it, including comments, is left alone,
while comments inside it may be reformatted. Editing the YAML directly always
works too; see the [reference](configuration.md).

## Using a standalone browser

Everything except handoffs and opening files in the editor also works in a
regular browser:

- From VS Code, run **Workbench: Open Canvas in Browser**.
- Without VS Code, run the server yourself. See
  [Running without VS Code](extension.md#running-without-vs-code).

In a browser, the screen list is on the left of the page, browser shortcuts
and menus work normally, and the camera downloads screenshots.
