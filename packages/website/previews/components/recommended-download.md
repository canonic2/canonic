# Recommended download

The Install page's panel with the `.vsix` for the visitor's computer, and the
other build of the same system: Macs with Intel, and Arm for Windows and
Linux. It stays hidden until the page recognizes the computer; the platform
table lists every file anyway.

| Prop | Type | Description |
| --- | --- | --- |
| `downloads` | object | The release's downloads, from `workbenchDownloads()` in `src/lib/releases.js` |

## Release

```example release
caption: downloads={downloads}
```

What the panel offers depends on the computer showing this page.
