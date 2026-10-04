# Code block

A code sample with its file name and a Copy button, as the overview and the
Install page use it. Without JavaScript the code is still there to select.

| Prop | Type | Description |
| --- | --- | --- |
| `file` | string | The file name or label above the code |
| `code` | string | The code |
| `copies` | string | What the Copy button's accessible name says it copies. Defaults to `the code`. |

## A file

```example workbench-yaml
caption: file="workbench.yaml"
```

## One line

```example one-line
caption: file="address"
```

A label that isn't a file name works the same way.
