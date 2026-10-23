# Camera, focus and input routing

Status: zoom, pan and keyboard/clipboard bridges are implemented. This topic
owns their existing interaction contract, independently of page content and
editor keybindings. [Sizes](sizes.md) owns layout dimensions;
[annotations](annotations.md) owns drawing gestures.

## Camera and scrolling

| Input | Result |
| --- | --- |
| Command/Ctrl + wheel, or pinch | Zoom around the pointer |
| Wheel over canvas background | Pan; wheel over page content scrolls that page |
| Space + drag, or middle-button drag | Pan the canvas |
| Command/Ctrl + plus or minus | Zoom by powers of two from the current level |
| Command/Ctrl + 0, or Shift 0 | Return to 100% |
| Shift 1 | Fit the content |
| Recenter view | Center at the current zoom without changing page scroll |

The view controls offer the same zoom operations and fixed 50%/200% choices.
The shipping canvas geometry clamps ordinary artboard zoom to 0.01–800%; Docs
reading additionally enforces a minimum of 10% and enough scale to keep the
viewport height within 8192 logical pixels. The older singleton zoom helper
clamps at 10–800%; that helper's tests do not establish the new canvas bounds.
Fitting does not magnify above 100%; automatic fitting follows viewport/size changes until the user
manually changes the camera. Zoom changes presentation, not the content's
layout dimensions or capture resolution.

Docs shown alone scroll as a document rather than panning an artboard; zoom
keeps the Docs layout width. Explicitly composed artboards share one outer
camera. [Artboard geometry](multiple-artboards.md) owns placement and content
fitting; this spec does not introduce a second camera inside a renderer.

## Focus and command ownership

- Typing, caret navigation, selection, copy/cut/paste, select-all and text undo
  remain in the focused page control or annotation note. Follow composed event
  paths into Shadow DOM so an editable field is not mistaken for shell chrome.
- Shift 0/1 do not invoke canvas commands while typing. AltGraph typing and
  modified caret movement remain native to the focused editor.
- Canvas zoom chords affect the canvas rather than VS Code's window zoom.
  Other editor shortcuts and multi-key chord continuations relay through the
  content iframe and panel wrapper to VS Code. Relaying an ordinary key must
  not cancel its native typing behavior merely because it may complete a chord.
- Select-all over shell controls must not select the entire application chrome.
  Clipboard operations target the document that owns the selection.
- Menus/dialogs preserve meaningful focus and return it to their invoking
  control when closed, under the [component contract](../ui/web-components.md#lifecycle-and-interaction).
- Only the active content destination may relay commands. Parked, replaced or
  disposed documents cannot issue navigation, editor or clipboard effects for
  the active view.

## Host and origin differences

Same-origin served pages can relay editor context menus and their selected text.
Respect a page-provided context menu. Menu selection context is cleared after
subsequent key/pointer activity; Cut rechecks that text is still selected.
Password fields do not expose their value as menu selection text.

Storybook uses its own key channel, checked against the active iframe and exact
origin. Its channel does not provide the same focused-input forwarding as a
same-origin page. Other foreign-origin pages retain their native input/scroll
behavior where the host cannot bridge it. Opening a page on its own retains
normal browser behavior.

The standalone browser keeps browser menus and shortcuts when no editor relay
is enabled. Sidebar tree/search/resize keyboard behavior follows the
[user-visible shortcut table](../../docs/canvas.md#sidebar-keys) and page-list
presentation; it must not depend on installing VS Code keybindings.

## Acceptance and evidence

- Repeat camera operations with shell, preview, input and Shadow DOM focus;
  layout size, form contents and scroll stay correct.
- Fit follows viewport changes; manual pan/zoom does not get silently replaced
  by a later automatic fit. Recenter preserves zoom and content scroll.
- Copy/cut/paste/select-all/undo affect the intended field/note/page. Password
  selection is omitted and a stale Cut cannot delete a character at the caret.
- Wrong-source and wrong-origin Storybook keys, and inactive renderer commands,
  have no effect. Editor chord continuations preserve ordinary typing.
- Verify Docs scrolling and annotation alignment at different zooms; verify
  ordinary browser behavior without the extension host.

[zoom.test.js](../../ui/legacy/zoom.test.js) and
[keys.test.js](../../ui/legacy/keys.test.js) exercise camera math and command
classification/relay. [Panel tests](../../extension/panel.test.js) cover the
host boundary; component tests own dialog focus behavior. These tests do not
establish real editor/menu/IME behavior on every supported OS. A full native
keyboard-layout, composition and assistive-technology matrix remains unverified.
Camera restoration is excluded from [durable persistence](persistence.md).

The legacy chrome adapter supplies power-of-two zoom steps to the shipping
shell. The standalone canvas element's own key handler instead multiplies by
1.25/0.8. Keep that component-level discrepancy separate from the required shell
behavior; [element tests](../../ui/src/components/canvas/element.test.js) and
[geometry](../../ui/src/modules/canvas/geometry.js) are the owning evidence
for the composed canvas, not only the singleton zoom tests.
