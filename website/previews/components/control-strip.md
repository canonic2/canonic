# Control strip

The overview's control screenshots with a hotspot over each control. Pointing
at, focusing, or tapping a hotspot captions it from the control's row in the
list in the default slot. Without JavaScript the list is the reference.

| Prop | Type | Description |
| --- | --- | --- |
| `groups` | array | `controls` from `src/data/screenshots.json`: each group's image and its controls |

Each row of the list is a `[data-label]` element with a `<dt>` and a `<dd>`,
so each description is written once.

## Controls

```example controls
caption: groups={screenshots.controls}
```

The list has only a few rows, so the hotspots without a row stay hidden.
