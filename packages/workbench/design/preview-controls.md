# Preview controls

The canvas and portable viewer use the same panel for transient preview inputs,
reported actions and optional documentation. The preview-controls capability owns
selection, inputs, actions and whether the panel is open; top-bar bindings supply
the element's model.

## Inputs

```example inputs
```

Text, number, boolean, select and JSON inputs use native form controls. Invalid
JSON stays editable and sends no change. Reset asks the capability to restore the
scenario. Updating an action log preserves focused fields.

## Actions

```example action-log
```

Reported actions appear with their values, alongside optional documentation.

## Public API

- `model`, default `null`: `{ instance, id, scenario, controls, inputs, actions, docs }`.
  `instance` identifies the owning renderer, separately from the preview's `id`.
  Assignment is silent and does not mutate the supplied object.
- `wb-preview-input`: detail `{ name, value }`, bubbles and composes.
- `wb-preview-reset`: no detail, bubbles and composes.
- Native `hidden` controls visibility. No other reflected attributes or slots.
- Native form keyboard behavior, validation and focus apply. Disconnect releases
  listeners; reconnect does not duplicate events.
- Uses `--wb-panel`, `--wb-fg`, `--wb-raised`, `--wb-line`, `--wb-focus`,
  `--wb-ui-font` and `--wb-radius-8`, with standalone defaults. No styling parts.
