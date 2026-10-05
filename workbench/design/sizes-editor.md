# Edit sizes

The dialog **Edit sizes…** in the [size switcher](size-switcher.md)'s menu
opens: the space's sizes in order, each with its name, width, height, icon,
and whether it is a button, to reorder or remove.

`wb-sizes-editor` in `src/components/sizes-editor/` keeps a draft of the rows
while it is open and asks to save them all at once; the canvas controller
writes them to `workbench.yaml` through `/_workbench/sizes`. Open each example
with its button.

## The space's sizes

```example space-sizes
caption: sizes = the space's
```

Fit and Resizable have no dimensions. A size set in `workbench.local.yaml` —
Small phone in the sample — is shown read-only, since only that file changes
it. Moving a row keeps focus on its button; removing one moves focus to the
next. Removing a size also removes it from every page that lists it, which
the server does when it saves.

## One size left

```example one-size
caption: sizes = [fit]
```

A space keeps at least one size, so the last can't be removed.

## Public API

| Property | Description |
| --- | --- |
| `sizes` | The space's sizes as the server resolves them, `local` included. Copied on assignment; the draft starts from them each time the dialog opens. |
| `open` | Shows the dialog as a modal |
| `pending` | While the controller writes: the rows and buttons are disabled |
| `error` | Why the controller couldn't save; cleared when a row changes |
| `iconRenderer` | Optional function that draws icon previews and row buttons |
| `draft` | The rows as they stand (read-only) |

`pending` is a reflected boolean attribute. The styling part is `dialog`.

`wb-sizes-save` carries `{ sizes }`: each row as `{ key, value }`, where
`value` is what `workbench.yaml` holds for it — `true` for a default size left
as it is, otherwise only the fields that differ. `wb-sizes-close` carries `{}`
when Cancel, Escape, or the backdrop ask to close. Both bubble and are
composed. Save is disabled, with the first problem shown, while a row can't
be written.
