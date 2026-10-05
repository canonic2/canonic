# Docs sidebar

The docs navigation: every guide by group, with the current one marked, and a
search field that filters the list from the docs' search index. It stays open
on wide screens and collapses on narrow ones. Without JavaScript the search
field is hidden and the list is complete.

| Prop | Type | Description |
| --- | --- | --- |
| `groups` | array | The guide groups, each with a `title` and its `entries` |
| `current` | string | The ID of the guide being shown |

Here the search index answers with the sample guides in `fixtures.ts`.

## Getting started open

```example getting-started-open
caption: current="getting-started"
```

Type in the search field to filter the guides.

## Search unavailable

```example search-unavailable
caption: current="getting-started"
```

The search index answers 503 here. Searching says *Search unavailable* and the
list stays complete.
