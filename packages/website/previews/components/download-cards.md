# Download cards

A card for each platform's `.vsix`, from `workbenchDownloads()` in
`src/lib/releases.js`. The card for the visitor's computer is marked.

| Prop | Type | Description |
| --- | --- | --- |
| `downloads` | object or `null` | The release's downloads, or `null` before the first release |

## Release

```example release
caption: downloads={downloads}
```

## Before the first release

```example no-release
caption: downloads={null}
```
