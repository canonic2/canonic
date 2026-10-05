# Actions switch

Whether the page's links navigate and its forms submit, in the middle of the
top bar after the [size switcher](size-switcher.md): a pointer icon and a
switch. Off, the preview records a link or a submit instead of following it.

`wb-actions-switch` in `src/components/actions-switch/` shows the setting and
asks for a change. The canvas controller in `workbench/workbench.js` owns the
setting, remembers it, and reloads the page with it; the preview's half is
`workbench/actions.js`.

## Off

```example off
caption: checked = false
```

The default: clicking around a page you're reviewing doesn't take you out of
it.

## On

```example on
caption: checked = true
```

The track carries the accent color, so the state reads without hovering.

## Through a lens

```example through-a-lens
caption: checked = true; disabled = true
```

A lens served by something other than the workbench, such as a URL or
Storybook lens, doesn't load `actions.js`, so the switch shows on and stays
put. Its title says why.

## In the top bar

```example in-the-top-bar
caption: after the size switcher
```

## Public API

| Property | Description |
| --- | --- |
| `checked` | Whether links and forms work in the page; defaults to false |
| `disabled` | Locks the switch where it is |
| `hint` | What the title says after *Actions —*; *let links navigate and forms submit* by default |
| `iconRenderer` | Optional function that returns a decorative icon element; without it, the switch shows alone |

`checked`, `disabled`, and `hint` reflect to attributes of the same names. The
styling part is `button`.

Activating the switch emits `wb-actions-toggle`, bubbling and composed, with
`{ checked }` set to the state asked for. Setting inputs emits nothing, and
asking doesn't change `checked`: the controller does.

The control is a native button with the `switch` role, named *Actions*, with
`aria-checked` reporting the state. Enter and Space toggle it.
