# Configure pages

Status: the collections form and native config writes are implemented. This
topic owns editing authored page declarations through **More → Configure pages**.
[Core](../core.md#configuration-and-serving) owns manifest composition and
[sizes](../canvas/sizes.md) owns size validation and its separate editing dialogs.

## Editing workflow

- Open the modal from a server-backed canvas and read that server's selected
  space from `GET /_workbench/config-file`. Show loading/read failures in the
  form; do not treat an unreadable manifest as an empty collection list.
- Add/remove/rename collections and set their icons. Add groups to a
  collection, from its row or its inspector: a new group is named *New group*,
  starts with one untitled page (a group needs one) and opens with its name
  field focused; groups don't nest, so groups and pages offer no **Add group**.
  Rename/remove groups and add pages within them; add/remove pages and edit
  label/source/size choices. Removing a collection or group confirms removal
  of its contained pages.
- Edits remain a draft until Save. Closing the form discards its draft; it
  does not write YAML. New page declarations do not create source files.
- Save validates required collection names, a name and at least one page for
  every group, and page labels/source paths, then
  posts `{ collections }`. Disable duplicate Save while the request is pending.
  On success close and refresh the resolved page model/sidebar. A save failure
  stays visible in the form and leaves the draft available to retry.

Closing before Save writes nothing. Closing while a submitted Save is in flight
does not undo that write; there is no write-cancellation endpoint. A late success
must not be presented as cancellation or applied to a newly opened form.

The form edits authored collections, not discovered definitions, catalog imports,
implementation services or project source content. Discovery changes continue
to come from their source definitions. Scenarios, mappings, docs/code pointers and
other retained fields without form controls survive edits to an existing entry.

## Write ownership and preservation

| Boundary | Rule |
| --- | --- |
| File | Write `workbench.yaml` in the server's configuration directory, never `workbench.local.yaml` |
| Space | Replace only the selected space's collections; if inherited, write its own override without changing sibling spaces |
| Text | Replace the collections block; retain outside text/comments. Comments inside the rewritten block are not preserved |
| Sizes | Preserve page-owned sizes and unedited keys; the size contract owns resolved choices and the separate `/_workbench/sizes` write path |
| Publication | Parse generated YAML before writing, write a temporary sibling, preserve file permissions and rename into place; clean up a failed temporary write |

A local `collections` override can continue to hide the committed edit in the
resolved view. Successful save means the committed file was written, not that
every current local override was replaced. Saving does not persist the user's
canvas selection; [restoration](../canvas/persistence.md) handles navigation
after refreshed configuration removes a selected page.

## API and failure behavior

`GET` answers `{ ok: true, collections, sizes }`, where `sizes` contains resolved
space size keys. Missing config is 404; read/parse failures are reported.
`POST` answers `{ ok: true, collections }` after saving. Invalid JSON, a non-list
collections value or write failure is reported as an error; unsupported methods
are 405. The shared [request body limit](../system/access-boundaries.md#request-and-process-bounds)
applies. No caller-supplied filesystem path selects the destination file.

Atomic replacement prevents publishing a partially written YAML document. It
does not provide draft conflict detection: the server rereads the file before
replacing its collections block, but accepts the submitted collection objects
without an expected revision. Do not claim a field-level merge with edits made
since the form opened or complete manifest semantic validation on Save.

## Acceptance and evidence

- Edit one page while preserving its scenarios, lens mappings, code/docs pointers
  and page-owned sizes. Saving one space preserves sibling spaces and unrelated
  YAML text; local overrides retain their normal precedence.
- Closing before Save produces no write; closing during Save follows the
  submitted-write rule above. Read/parse/write errors are visible and do not
  publish partial YAML. Retrying Save does not create duplicate entries.
- A successful save refreshes the sidebar/canvas against the same space;
  removed destinations follow routing/restoration rather than stale metadata.
- Closed, reopened or retargeted forms ignore old reads/save completions.
  A completion from space A must not refresh or overwrite space B's draft.

The form is in [config-editor.js](../../ui/legacy/config-editor.js); native
write/preservation code is in [edit.rs](../../server/src/config/edit.rs) and
[text.rs](../../server/src/config/edit/text.rs). The latter has block-replacement
and YAML-value round-trip tests. That evidence does not establish the full
browser form workflow, concurrent editing or injected filesystem failure matrix.

## Gaps and open decisions

- Resolved custom/page-owned size cards remain [size-editor delivery work](../canvas/sizes.md#remaining-delivery).
- The legacy form does not fence asynchronous requests by opening/space
  generation; stale completion acceptance above remains implementation work.
- Choose a conflict policy for external YAML edits and two open forms. A
  revision check/refusal is a candidate, not an agreed API or implemented merge.
- The form has no unsaved-draft warning or draft persistence. Whether to add
  either, and how to expose local-override shadowing, remains a product decision.
