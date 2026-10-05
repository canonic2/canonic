# Space switcher

Which space is showing, and the others to switch to: a button with the
space's [mark](space-mark.md) and name, and under it a menu of every space,
the current one checked.

It sits at the top of the sidebar: always in VS Code's Workbench view, and
in a standalone browser window when there is more than one space. It's drawn
by `wb-space-switcher` in `src/components/space-switcher/`. The controller gets
spaces from the extension or the server's `/_workbench/spaces`; the component
draws them and emits user intent. Its markup and styles live in open Shadow DOM.

## Closed

```example closed
caption: currentId = 'acme'
```

The top of the sidebar: the current space's mark and name.

## Open

```example open
caption: open = true
```

Every space, the current one checked, then **Add a space…** when the host
sets `allowAdd`.

## Removable space

```example removable
caption: removable: true
```

A space added from elsewhere on disk shows a remove button on hover, in
place of the check. Hover a row or move keyboard focus to its remove action.

## One space

```example one-space
caption: spaces.length === 1
```

With one space, the menu still lists it and offers **Add a space…**.

## Public API

| Property | Description |
| --- | --- |
| `spaces` | Display data; each listed space has a unique ID; copied on assignment |
| `currentId` | The selected space ID; defaults to empty |
| `iconRenderer` | Optional function that returns a decorative icon element |
| `allowAdd`, `allowRemove` | Whether add/remove actions are available; both default to false |
| `disabled` | Disables interaction and closes the menu; defaults to false |
| `open` | Programmatic menu state; defaults to false |

`allow-add`, `allow-remove`, and `disabled` are presence-based reflected boolean
attributes. `variant="breadcrumb"` selects the compact top-bar presentation;
the default is the sidebar. `open`, selection, icons and structured data are
properties only. There are no slots. Styling parts are `trigger`, `name`, and
`menu`; shell colors come from shared `--wb-*` tokens.

Events bubble and are composed, with typed detail: `wb-space-pick` and
`wb-space-remove` carry `{ id }`, `wb-space-add` carries `{}`, and
`wb-space-toggle` carries `{ open }`. They are notifications of user intent and
are not cancelable. Picking the current space closes the menu without a pick
event. Property updates emit no user-intent events and do not change controller
selection. Programmatic `open` updates do not emit toggle events; a host that dims
neighboring content must also update that presentation for programmatic changes.

Arrow keys open the menu from its trigger and move through all pick/remove/add
actions with wrapping; Home/End select the ends. Escape closes and restores the
trigger; Tab closes while allowing normal focus traversal. Enter/Space activate
native buttons. Outside pointer presses and window blur close the menu. Updates
preserve focus on surviving actions; if a focused action disappears, focus moves
to the first available action. Disconnect releases connection listeners and
closes the menu; reconnect restores an interactive closed control.
