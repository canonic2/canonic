# Custom size

The dialog **Custom size…** in the [size switcher](size-switcher.md)'s menu
opens: a new size's name, width, height, icon, whether it is a button, and,
from a page `workbench.yaml` lists, whether it is that page's own.

`wb-size-dialog` in `src/components/size-dialog/` is a modal form. It checks
the fields as they are typed and asks to add the size; the canvas controller
writes it to `workbench.yaml` through `/_workbench/sizes` and closes the
dialog, or shows why it couldn't. Open each example with its button.

## For the space

```example for-a-space
caption: allowPageOnly = false
```

Width and height each take a whole number from 1 to 8192, or **Fill** to
follow the canvas. **Add** stays disabled until the fields make a size, and
the line above it says why once something is typed.

## For a page

```example for-a-page
caption: allowPageOnly = true
```

**Only for this page** writes the size into the page's own `sizes`; such a
size can't be a button, so **Show as a button** turns off.

## When the save fails

```example with-an-error
caption: error = '…'
```

The controller's error replaces the field check, and clears when a field
changes.

## A preview definition's sizes

```example preview-definition
caption: notice = '…'
```

A note the controller sets, here for a page whose sizes its preview
definition lists.

## Public API

| Property | Description |
| --- | --- |
| `open` | Shows the dialog as a modal, with empty fields each time it opens |
| `pending` | While the controller writes: fields and buttons are disabled, and it can't be dismissed |
| `error` | Why the controller couldn't add the size; cleared when a field changes |
| `notice` | A note shown above the actions |
| `allowPageOnly` | Whether **Only for this page** is offered |
| `iconRenderer` | Optional function that draws the icon preview |
| `draft` | The fields as typed (read-only) |

`pending` and `allow-page-only` are reflected boolean attributes. The styling
part is `dialog`.

`wb-size-add` carries `{ name, width, height, icon?, button, pageOnly }`, with
each length a number or `'fill'`. `wb-size-dialog-close` carries `{}` when
Cancel, Escape, or the backdrop ask to close; the controller sets `open`.
Both bubble and are composed. Focus starts in **Name**, and Tab stays in the
dialog while it is open.
