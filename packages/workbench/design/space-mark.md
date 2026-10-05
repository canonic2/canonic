# Space mark

The coloured square that stands for a space: in the
[space switcher](space-switcher.md), and at the start of the top bar's
breadcrumb when there is more than one space.

`wb-space-mark` in `src/components/space-mark/` draws it from the
space's `color`, `icon`, and `image`, which come from its `workbench.yaml`, so
the mark matches wherever the space is shown. It holds the space's image, else
its Lucide icon, else the first letter of its name. Set the `space` property
with those fields; `iconRenderer` supplies the host's icon renderer. The element
is decorative; its surrounding control supplies the accessible name.

## Named colors

```example named-colors
caption: color: blue
```

The nine named colors, each holding the space's initial.

## Hex colors

```example hex-colors
caption: color: "#2f7d55"
```

Any hex color. A light one, by its relative luminance, takes dark text.

## Icon

```example icons
caption: icon: palette
```

Any Lucide icon name, in place of the initial.

## Image

```example images
caption: image
```

An image with no color stands on its own; with a color, it sits on the
square.

## Fields

| Field | Values | Default |
| --- | --- | --- |
| `initial` | The first letter of the space's name | |
| `color` | `blue`, `green`, `orange`, `purple`, `pink`, `teal`, `red`, `yellow`, `gray`, or a hex color | none |
| `icon` | A kebab-case Lucide icon name | none |
| `image` | An image URL, as the server sends it | none |

## Public API

| Property | Description |
| --- | --- |
| `space` | The space's fields above, plus `name` for the initial when `initial` is missing. Copied on assignment. |
| `iconRenderer` | Optional function that returns a decorative icon element; without it, an `icon` shows the initial |

The element has no attributes, events, or parts and takes no focus: it is
`aria-hidden`, and the control around it names the space. It is 22 pixels square
unless the host's `width` and `height` say otherwise; `--wb-mark-radius` (6px) and
`--wb-mark-font-size` (12px) set its corners and its initial's size.
