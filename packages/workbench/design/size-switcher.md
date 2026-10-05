# Size switcher

The artboard's size, in the middle of the top bar: the space's button sizes
as a row of icon buttons, then a menu button that lists every size and, when
the workbench server is running, **Custom size…** and **Edit sizes…**. See
[Custom size](size-dialog.md) and [Edit sizes](sizes-editor.md) for the
dialogs they open.

`wb-size-switcher` in `src/components/size-switcher/` draws the sizes it is
given and says what was asked for. The canvas controller in
`workbench/workbench.js` supplies the space's sizes and the page's, owns the
current size, and handles every request. Sizes and their rules are in
`src/sizes/`; the product contract is `specs/sizes.md`.

## Buttons

```example buttons
caption: current = 'laptop'
```

The sizes the space marks `button: true`, in its order, each named by its
label and dimensions. The menu button shows a chevron while a button size is
on.

## More sizes

```example more-sizes
caption: current = 'tablet'; allowEdit = true
```

The menu lists the button sizes under **Sizes** and the rest — the space's,
then the page's own — under **More sizes**, each with its dimensions. A size
that isn't a button shows its icon in the menu button, pressed. Small phone
is set in `workbench.local.yaml` in the sample, which changes nothing here.

## Limited by the page

```example limited-by-page
caption: supported = ['sidebar', 'resizable']
```

Sizes the page doesn't list are disabled, and their titles say why.

## On a docs page

```example docs-page
caption: supported = []; current = ''
```

A docs page fills the canvas, so every size is disabled and none pressed.
**Custom size…** still adds a size to the space.

## Public API

| Property | Description |
| --- | --- |
| `sizes` | Every size the menu lists, in order: the space's, then the page's own. Each has `key`, `label`, `icon`, `kind`, `width`, `height`, and `button`. Copied on assignment. |
| `supported` | The keys the page supports, or `null` for every size; an empty list disables them all |
| `current` | The key of the size the artboard is at; empty for none |
| `iconRenderer` | Optional function that returns a decorative icon element |
| `allowEdit` | Whether the menu offers Custom size… and Edit sizes…; defaults to false |
| `disabled` | Disables every control and closes the menu |
| `open` | Programmatic menu state; defaults to false |

`allow-edit` and `disabled` are reflected boolean attributes, and
`unsupported-reason` sets what a disabled size's title says (*not supported by
this page* by default). Styling parts are `group`, `more`, and `menu`.

Events bubble and are composed: `wb-size-pick` carries `{ key }` when an
enabled size other than the current one is chosen; `wb-size-custom` and
`wb-size-edit` carry `{}`; `wb-size-toggle` carries `{ open }` when the menu
opens or closes by interaction. Setting inputs emits nothing, and picking
doesn't change `current`: the controller does.

The buttons are native buttons in a group named *Size*. The menu button opens
the menu with a click, or with Down or Up, focusing the checked size. In the
menu, arrows move between enabled entries and wrap, Home and End go to the
ends, Enter or Space choose, and Escape closes and returns focus to the menu
button. Tab, an outside press, or the window losing focus close it.
