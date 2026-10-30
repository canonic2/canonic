# Release entry

One release on the Changelog: its version, date, and tag beside its notes,
its links on GitHub, and its builds when it has any. The examples show it in
the Changelog's list.

| Prop | Type | Description |
| --- | --- | --- |
| `release` | object | One entry of `workbenchReleases()` in `src/lib/releases.js` |
| `latest` | boolean | Marks the release the Install sections link to. Defaults to `false`. |

## Latest, with builds

```example latest
caption: latest
```

## Without builds

```example without-builds
caption: files: []
```

## Without notes

```example without-notes
caption: notes: [], files: []
```
