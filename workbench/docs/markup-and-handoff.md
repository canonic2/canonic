# Markup and handoff

Draw on any screen, through any lens, then save a screenshot or hand the whole
thing to an agent: an annotated screenshot plus a written account of every
mark and the element under it.

## The markup dock

The dock floats at the bottom of the canvas.

| Tool | Does |
| --- | --- |
| **Select** (Esc) | Uses the page normally, and selects marks to move, resize, or delete them. |
| **Scribble** | Freehand drawing. |
| **Arrow** | Points at something. Drag from the tail to the head. |
| **Shape** | Draws a rectangle, a circle, or a line. The caret beside it switches between them. |
| **Text** | A note written on the canvas. Click where it goes and type. |
| **Comment** | A comment pinned to a point on the page. Click the thing you're commenting on and type. |
| **Undo** (⌘Z or Ctrl+Z) | Removes the last mark. |
| **Clear markup** | Removes every mark. |
| **Save screenshot** | Downloads the frame with its marks. See [Screenshots](#screenshots). |
| **Copy handoff** | Saves the screenshot and copies a prompt. See [Hand off to an agent](#hand-off-to-an-agent). |

While you have marks, a count such as **3 marks** beside **Copy handoff** shows
how many the handoff will carry. Selecting a drawing tool you're already using
returns to **Select**.

Marks are drawn in one color and weight, so an agent can tell annotation from
design. Text and comments can hang past the frame's edge onto the canvas;
screenshots are cropped to the frame either way.

### Working with marks

- **Select a mark** with the Select tool to show its handles. Drag it to move
  it, or drag a handle to resize it.
- **Delete** or **Backspace** removes the selected mark.
- **Arrow keys** nudge the selected mark by 1 pixel, or 8 with Shift.
- **Escape** leaves a drawing tool, finishes editing a note, or deselects the
  mark.
- **Marks don't block the page.** In Select mode, only a mark's outline and
  handles take clicks. Clicking inside an empty rectangle reaches the page
  underneath, so you can keep using the page while annotations are on it.
- **Marks belong to a screen and state.** They are cleared when you switch to
  another screen or state. Changing width or lens keeps them, so you can draw
  on the design and then check the same spots on the implementation.
- Marks stay in place when you zoom and pan, because they scale with the
  frame.

## Screenshots

**Save screenshot** (the camera) downloads a JPEG of the frame at its real
size, whatever the zoom, with your marks on it. It is named after the screen's
file, state, and lens:

| View | Filename |
| --- | --- |
| `pages/sign-in.html`, first state, Design | `sign-in.jpg` |
| Same, state `error` | `sign-in-error.jpg` |
| Same, through the `staging` lens | `sign-in-error-staging.jpg` |
| TypeScript preview `src/button.workbench.ts`, state `disabled` | `button.workbench-disabled.jpg` |

On a Storybook lens, the story stands where the state does. The file goes to
your browser's downloads, and a message names it.

Screenshots use 90% JPEG quality. Their pixel size matches the frame's CSS
size, so Retina displays don't double it. The camera doesn't write into your
project.

### How screenshots are taken

A background screenshot helper keeps a live copy of the page you're looking at:
its DOM, open shadow roots, styles, form values, scroll positions, hover
state, and your marks. When you take a screenshot, it brings the copy up to
date and captures it. That is why screenshots are fast and show what you see,
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
| Canvas and video frames | Changes still in progress at the moment of capture |
| Open dialogs and popovers, and CSS hover states | Rules a script changes at runtime in a linked stylesheet |
| Animation at its current point | |

Some pages can't be captured at all, and the camera or handoff shows a message
instead of a screenshot:

- A page that contains an `iframe`, `object`, or `embed`.
- A page with a canvas or video loaded from another origin without CORS.

For a [URL lens](lenses.md#screenshots-through-a-url-lens) or a
[Storybook lens](storybook.md#screenshots-of-stories), the helper loads the
page itself unless the page loads the preview bridge. Without the bridge, a
page behind sign-in may be captured as its sign-in page.

## Hand off to an agent

**Copy handoff** turns the current view into something any coding agent can
act on:

1. The screenshot, with marks, is saved to `.canonic/.handoffs/` in the
   project, named like the camera's file. A second handoff of the same view
   gets `-2`, `-3`, and so on.
2. The marks are cleared from the canvas.
3. A prompt is copied to your clipboard. Paste it into Claude Code, Codex, or
   any other conversation.

The prompt says which screen, state, and width the screenshot shows, where it
is saved, and what each mark is, with its position and the element under it:

```text
Here is a screen from Workbench.

- Screen: Sign in — `pages/sign-in.html`
- State: Wrong password
- Width: Mobile · iPhone 15 Pro, 393 × 852 (frame is 393 × 852 CSS px)
- Screenshot: `.canonic/.handoffs/sign-in-error.jpg`

The screenshot has 3 marks drawn over it. **Everything red in the image is
annotation, not design** — …

1. Arrow pointing at (168, 254) — on button.primary “Sign in”
2. Rectangle at (24, 300), 342 × 96 — on input#email
3. Note at (30, 410): “make this full width”

In words, the notes say:
- “make this full width”
```

Through a lens, the prompt also names the implementation and its URL, and says
that the screenshot shows the implementation while the design it should match
is the screen's `src`. For a Storybook lens, it names the story and its id.
When the screen has [code pointers](lenses.md#point-at-the-code), a `Source:`
line gives their absolute paths, so the agent changes the right code. That
line appears on the Design lens too. For a
[TypeScript preview](workbench-previews.md), it gives the preview's source
file.

When the elements under the marks can't be read, the prompt says so, and the
coordinates are what the agent has to go on. A mark over empty space is
counted, so the agent knows to check the screenshot. A handoff with no marks
says the screenshot shows the screen as it stands.

Handoffs need the VS Code extension, which copies the prompt. The button shows
in the editor and in a browser opened with **Workbench: Open Canvas in
Browser**, but not when you run the server yourself. Add `.canonic/.handoffs/`
to `.gitignore`. The screenshots are review material, not source.

### Writing good handoffs

- **Say what you want in words.** Text notes and comments are quoted at the end
  of the prompt as the request. Shapes say where; words say what.
- **Point at one element per mark.** Each mark names the element under it, so
  an arrow on the button is more useful than a box around the whole form.
- **Hand off through the lens you mean.** To fix the implementation, mark up
  the implementation's lens, so the agent gets its URL and code. To change the
  design, use the Design lens.
- **Use states.** Hand off the error state from its own state row, so the
  prompt and filename say which state it is.

## Copy a reference instead

When you only need to point at a screen, with no screenshot or marks, use
**Copy reference** in the toolbar. See [Copy a reference](canvas.md#copy-a-reference).
