# Platform table

Every platform's `.vsix` in a table, as the Install page lists them. The row
for the visitor's computer is marked.

| Prop | Type | Description |
| --- | --- | --- |
| `downloads` | object | The release's downloads, from `workbenchDownloads()` in `src/lib/releases.js` |

## Release

```example release
caption: downloads={downloads}
```
