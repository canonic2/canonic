# Read every page

List commands and tools accept `cursor` and `limit`. Start without a cursor,
then send the returned `nextCursor` with the same filters. Stop when it is null.
The default is 50 items and the maximum is 100. Invalid limits are refused.

```sh
testrun target list --limit 10 --json
testrun target list --limit 10 --cursor 'RETURNED_CURSOR' --json
testrun project list --root . --limit 50
testrun project discover --root . --limit 50
```

Human output includes an exact continuation command. JSON carries the same
page fields as MCP. Pages may end early at 256 KiB of serialized items; a
cursor can conservatively lead to an empty final page. `invalid_cursor` means
restart without a cursor, including after changing filters or the underlying
immutable observation. Cursors are temporary and should not be saved.

Targets, apps and connected targets sort by the first 64 identity bytes, then
the full identity digest as a unique tie-breaker. Projects and build history
use ascending identity order. Discovery traverses directories in component-digest
preorder (parents before descendants); explicit path selections use full path
digest order. These bounded sort keys permit long IDs and paths in a cursor.
UI matches and shallow snapshot nodes use document order.
Snapshot nodes include JSON-pointer `path` values identifying their ancestry.
UI cursors belong to the observed tree and fail when it changes.

`ui_snapshot`, `diagnostics_read` and `build_log` count bytes: default 16384,
maximum 65536. Diagnostic bytes are base64 encoded.
Log pages also include exact `dataBase64` bytes, since a
byte boundary can split a UTF-8 character in the readable `text`. Finished output returns
null at its end. Live output reports `running` and returns a cursor at its
current end so a later read can collect new bytes. The
[documentation tools](mcp.md) count lines for `docs_read` (100 by default,
maximum 400).

Diagnostic row queries follow retained index order, or timestamp and source
ordinal for a single-clock timeline. Keep all filters on continuation, including
view-specific options. Performance measurement-group previews name a
`diagnostics_query` route with `collection: "measurement_groups"`.

Browser `events_read` takes paging fields on its request. Browser
`diagnostics_read` takes them inside `query`. Their `nextRequest` can be sent
directly as the next `browser_send` request. Both histories are bounded;
eviction can invalidate an older cursor. See [browser diagnostics](browser.md).

Project discovery examines manifests without executing project code. It scans
six directory levels by default, skips hidden/vendor/build directories and
symlinks, and accepts explicit `--path` selections for deeper projects. Each
manifest has a 256 KiB read bound; warnings identify incomplete inspection.
Configuration proposals describe the current page's candidates. Select the
intended paths explicitly when initializing a project configuration.
