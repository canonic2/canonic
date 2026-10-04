# Annotations and handoff

Draw on any page, through any lens, then save a screenshot or hand the whole
thing to an agent: a screenshot with your annotations plus a written account of
every annotation and the element under it.

## The toolbar

The toolbar floats at the bottom of the canvas.

| Tool | Does |
| --- | --- |
| **Select** (Esc) | Uses the page normally, and selects annotations to move, resize, or delete them. |
| **Scribble** | Freehand drawing. |
| **Arrow** | Points at something. Drag from the tail to the head. |
| **Shape** | Draws a rectangle, a circle, or a line. The caret beside it switches between them. |
| **Text** | A note written on the canvas. Click where it goes and type. |
| **Comment** | A comment pinned to a point on the page. Click the thing you're commenting on and type. |
| **Undo** (⌘Z or Ctrl+Z) | Removes the last annotation. |
| **Clear annotations** | Removes every annotation. |
| **Save screenshot** | Downloads the artboard, or the part of a docs page in view, with its annotations. See [Screenshots](#screenshots). |
| **Copy handoff** | Saves the screenshot and copies a prompt. See [Hand off to an agent](#hand-off-to-an-agent). |

While you have annotations, a count such as **3 annotations** beside **Copy handoff**
shows how many the handoff will carry. A drawing tool stays selected after each
annotation, so you can draw several in a row, and the new annotation is
selected so you can nudge or resize it straight away. Selecting a drawing tool
you're already using returns to **Select**.

Annotations are drawn in one color and weight, so an agent can tell them from
the design. Text and comments can hang past the artboard's edge onto the canvas;
screenshots are cropped to the artboard either way.

### Working with annotations

- **Select an annotation** with the Select tool to show its handles. Drag it
  to move it, or drag a handle to resize it.
- **Delete** or **Backspace** removes the selected annotation.
- **Arrow keys** nudge the selected annotation by 1 pixel, or 8 with Shift.
- **Escape** leaves a drawing tool, finishes editing a note, or deselects the
  annotation.
- **Double-click** a note or comment with the Select tool to edit its text. A
  note or comment left empty is removed.
- **Shift** while drawing keeps arrows and lines at 45° steps and makes
  rectangles and circles square. Shift while dragging a corner handle keeps
  them square.
- **Annotations don't block the page.** In Select mode, only an annotation's
  outline and handles take clicks. Clicking inside an empty rectangle reaches
  the page underneath, so you can keep using the page while annotations are on
  it.
- **Annotations belong to a page and state.** They are cleared when you
  switch to another page or state. Changing size or lens keeps them, so you can
  draw on the design and then check the same spots on the implementation.
- Annotations stay in place when you zoom and pan, because they scale with the
  artboard. On a docs page, they stay on the page as it scrolls and zooms.

Every key is listed in [Keyboard shortcuts](canvas.md#keyboard-shortcuts).

## Screenshots

**Save screenshot** (the camera) downloads a JPEG of the artboard at its real
size, whatever the zoom, with your annotations on it. On a
[docs page](#on-a-docs-page), it captures the part of the page in view. It is
named after the page's file, state, and lens:

| View | Filename |
| --- | --- |
| `pages/sign-in.html`, first state, Design | `sign-in.jpg` |
| Same, state `error` | `sign-in-error.jpg` |
| Same, through the `staging` lens | `sign-in-error-staging.jpg` |
| TypeScript preview `src/button.workbench.ts`, state `disabled` | `button.workbench-disabled.jpg` |

On a Storybook lens, the story stands where the state does. The file goes to
your browser's downloads, and a message names it.

Screenshots use 90% JPEG quality. Their pixel size matches the artboard's CSS
size, so Retina displays don't double it. The camera doesn't write into your
project.

### How screenshots are taken

A background screenshot helper keeps a live copy of the page you're looking at:
its DOM, open shadow roots, styles, form values, scroll positions, hover
state, and your annotations. When you take a screenshot, it brings the copy up
to date and captures it. That is why screenshots are fast and show what you see,
including what you typed and where you scrolled, without running your page's
scripts a second time.

The helper ships with the extension and starts with the workbench. On macOS it
runs in the background with no Dock icon or window. See
[The screenshot helper](extension.md#the-screenshot-helper).

### What screenshots can and can't include

| Included | Not included |
| --- | --- |
| The page's DOM, open shadow roots, and stylesheets | Closed shadow roots |
| Form values and scroll positions | Browser UI, such as native select menus |
| `<canvas>` elements and video | Changes still in progress at the moment of capture |
| Open dialogs and popovers, and CSS hover states | Rules a script changes at runtime in a linked stylesheet |
| Animation at its current point | |

Some pages can't be captured at all, and the camera or handoff shows a message
instead of a screenshot:

- A page that contains an `iframe`, `object`, or `embed`.
- A page with a canvas or video loaded from another origin without CORS.

For a [URL lens](lenses.md#screenshots-through-a-url-lens) or a
[Storybook lens](storybook.md#screenshots-of-stories), the preview bridge
Workbench adds to the page sends its live document, so the screenshot shows
what you see. If the bridge hasn't connected yet, the helper loads the page
itself, and a page behind sign-in may be captured as its sign-in page.

## Hand off to an agent

**Copy handoff** turns the current view into something any coding agent can
act on:

1. The screenshot, with annotations, is saved to `.canonic/.handoffs/` in the
   project, named like the camera's file. A second handoff of the same view
   gets `-2`, `-3`, and so on.
2. The annotations are cleared from the canvas.
3. A prompt is copied to your clipboard, and VS Code confirms it with a
   notification. Paste it into Claude Code, Codex, Cursor's chat, or any other
   conversation.

The prompt says which page, state, and size the screenshot shows, where it
is saved, and what each annotation is, with its position and the element under
it:

```text
Here is a page from Workbench.

- Page: Sign in — `pages/sign-in.html`
- State: Wrong password
- Width: Mobile · iPhone 15 Pro, 393 × 852 (artboard is 393 × 852 CSS px)
- Screenshot: `.canonic/.handoffs/sign-in-error.jpg`

The screenshot has 3 annotations drawn over it. **Everything red in the image
is an annotation, not the design** — …

1. Arrow pointing at (168, 254) — on button.primary “Sign in”
2. Rectangle at (24, 300), 342 × 96 — on input#email
3. Note at (30, 410): “make this full width”

In words, the notes say:
- “make this full width”
```

Through a lens, the prompt also names the implementation and its URL, and says
that the screenshot shows the implementation while the design it should match
is the page's `src`. For a Storybook lens, it names the story and its id.
When the page has [code pointers](lenses.md#point-at-the-code), a `Source:`
line gives their absolute paths, so the agent changes the right code. That
line appears on the Design lens too. For a
[TypeScript preview](workbench-previews.md), it gives the preview's source
file. For a docs page, see [On a docs page](#on-a-docs-page).

When the elements under the annotations can't be read, the prompt says so, and
the coordinates are what the agent has to go on. An annotation over empty space
is counted, so the agent knows to check the screenshot. A handoff with no
annotations says the screenshot shows the page as it stands.

Handoffs need the VS Code extension, which copies the prompt. The button shows
in the editor and in a browser opened with **Workbench: Open Canvas in
Browser**, but not when you run the server yourself. Add `.canonic/.handoffs/`
to `.gitignore`. The screenshots are review material, not source.

### Pasting into an agent

The handoff isn't tied to any agent: it is text on your clipboard and a JPEG
in your project. Nothing is sent anywhere until you paste it.

- **Run the agent in the project folder.** The screenshot path, such as
  `.canonic/.handoffs/sign-in-error.jpg`, is relative to the folder that holds
  `workbench.yaml`. An agent working in that folder, such as Claude Code or
  Codex in a terminal there, or the chat of the editor window that has the
  project open, can open the file from that path.
- **Agents that can't read image files still get the request.** The prompt
  describes every annotation, its position, and the element under it in words,
  so the screenshot corroborates rather than carries it.
- **Attach the image yourself** when your agent only accepts pasted or
  attached images. The file is in `.canonic/.handoffs/`.
- **Give it the code.** [Code pointers](lenses.md#point-at-the-code) add a
  `Source:` line with absolute paths, so an agent working in another folder,
  such as an app repository beside a design repository, still finds the code.

### Writing good handoffs

- **Say what you want in words.** Text notes and comments are quoted at the end
  of the prompt as the request. Shapes say where; words say what.
- **Point at one element per annotation.** Each annotation names the element
  under it, so an arrow on the button is more useful than a box around the whole
  form.
- **Hand off through the lens you mean.** To fix the implementation, annotate
  the implementation's lens, so the agent gets its URL and code. To change the
  design, use the Design lens.
- **Use states.** Pick the error state in the page list before handing off,
  so the prompt and filename say which state it is.

## On a docs page

A [docs page](docs-pages.md) fills the canvas and scrolls, so annotations,
screenshots, and handoffs follow the page rather than an artboard:

- **Annotations** stay on the page as it scrolls and zooms, over the example
  or the text you drew them on.
- **Screenshots** capture the part of the page in view, with your annotations.
- **Handoffs** name the docs page's Markdown file, the lens that renders its
  examples, and the examples in view with their source files. In place of a
  width, the prompt gives the size of the part in view.

Scroll the example you mean into view before handing off, so the prompt names
it.

## Copy a reference instead

When you only need to point at a page, with no screenshot or annotations, use
**Copy reference** in the top bar. See [Copy a reference](canvas.md#copy-a-reference).
