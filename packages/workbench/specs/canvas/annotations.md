# Annotation editing

This spec owns drawing, selection and annotation state. It does not introduce
an edit-history or storage system. [Interaction](interaction.md)
owns focus and shortcut routing; [capture](../review/capture.md) owns pixel
acquisition and handoff output. The [user guide](../../docs/annotations-and-handoff.md)
describes the visible controls.

**Status:** implemented in the legacy content renderer: the tools, editing,
per-artboard ownership and review clearing below. Not settled or implemented:
consistent clearing across lens changes, annotation bounds and a transactional
multi-artboard clear; see [implementation gaps](#implementation-gaps).

## Tools and editing

| Tool or command | Behavior |
| --- | --- |
| Select | Use the page normally; select an annotation by its outline or handles |
| Scribble | Draw a freehand path |
| Arrow | Drag from tail to tip |
| Shape | Draw a rectangle, circle or line; choose its kind through the shape menu |
| Text | Place and edit a text note |
| Comment | Place and edit a pinned comment |
| Undo | Remove the most recently created remaining annotation |
| Clear annotations | Remove every annotation in the selected artboard |

A drawing tool stays active after creation; choosing the active drawing tool
returns to Select. A new annotation is selected. Drag a selected annotation
to move it, or its handles to resize it. Shift constrains arrows/lines to 45°
steps and rectangles/circles to square bounds when drawing or corner-resizing.
Arrow keys nudge by one local CSS pixel, or eight with Shift. Delete/Backspace
removes the selected annotation outside text editing.

Double-click a note/comment in Select to edit it; finishing an empty note removes
it. Escape finishes text editing, leaves a drawing tool or deselects, as applicable.
Selection controls are editing affordances, not part of the captured drawing.
Undo is creation-order removal, not a command history for moves, resizing or
text edits. Redo and durable annotation history are outside the existing workflow.

## Ownership and coordinates

- Each artboard/content renderer owns its annotations, current tool and
  selection. Selecting another existing artboard preserves each one's drawings;
  commands apply to the selected artboard only.
- Annotations use local CSS-pixel coordinates. Canvas zoom/pan transforms their
  display without rewriting their geometry. Resizing a view keeps the drawings
  at their original coordinates rather than scaling them to new content.
- Docs annotations follow document scroll. Capturing Docs includes the visible
  viewport under the [Docs contract](../pages/docs-pages.md#screenshots-handoff-and-export).
- Text/comments may extend outside an artboard; capture clips to the content
  viewport. In Select, empty shape interiors pass pointer input to the page.
- Closing or replacing an instance releases its annotation UI and listeners.
  Retention preserves drawings only while their owning instance survives.
  [Persistence](persistence.md#session-only-state-and-exclusions) excludes them
  from restart restoration.

## Review integration and failures

Camera capture preserves annotations. Full-canvas handoff clears only the
annotations in its captured snapshot, after saving and delivering it.
Capture/save/delivery failure must not clear them. An
annotation added after acquisition must survive completion of that review.
The review controller owns capture locks, immutable membership and completion;
the annotation owner supplies geometry and clear-captured operations.

Handoff target inspection uses each annotation's local anchor (including an
arrow's tip). An unreadable target remains a coordinate-based annotation; it
must not be silently omitted or assigned to another artboard. Text is carried
as user content, not inserted as executable page markup.

A single-view screenshot's file name carries the page, scenario (or story),
iteration ID, lens key and size key, so iterations of one artboard do not
overwrite each other's images.

## Acceptance and evidence

- Draw, select, move, resize, nudge, edit and delete each supported kind without
  changing another artboard or losing input focus to the shell.
- Page controls remain usable through empty shapes in Select. Shift constraints
  and geometry remain correct at different zooms and while Docs scroll.
- Undo removes the latest remaining creation; Clear affects only the selected
  artboard. Neither shortcut consumes the focused page/note's own text editing.
- Camera leaves drawings; failed capture/save/delivery leaves drawings;
  post-delivery clearing affects only captured drawings, including when a new
  one was added meanwhile. Cleanup failures follow the gap below.

Implementation is in [annotations.js](../../ui/legacy/annotations.js) and its
[styles](../../ui/legacy/annotations.css). [Capture tests](../../ui/legacy/annotations-capture.test.js)
cover acquisition/handoff paths; [review controller tests](../../ui/src/modules/review/controller.test.js)
cover completion and failure ownership. [Canvas browser checks](../../scripts/smoke-canvas-ui.cjs)
exercise instance annotation isolation. None of them establishes complete
editing or accessibility acceptance: there is no dedicated behavioral suite
covering every drawing/editing gesture, text-input composition, touch/pen input
and keyboard-only creation.

## Implementation gaps

- The legacy reset keys annotations by page `src`, scenario and the address's
  iterations map: a change of any of them clears the drawings, while a lens or
  size change keeps them, so a design's notes stay over its implementation for
  comparison. The user guide describes a fresh artboard on page/scenario/lens
  changes. Verify ordinary-renderer retention and explicit artboard replacement
  together before promising consistent cross-lens clearing or preservation.
- Annotation count, freehand-point and text-length bounds are unspecified.
  Establish practical limits with interaction/capture evidence rather than
  assuming the artboard count or image-size limits bound annotation work.
- Post-delivery clearing is not transactional across renderers. A clear-captured
  failure can follow an already saved/delivered handoff and partial cleanup;
  there is no rollback. Acceptance of failure preservation applies before
  delivery, not to an atomic multi-artboard clear that the code does not provide.
