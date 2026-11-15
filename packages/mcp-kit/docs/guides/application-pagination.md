# Return application lists in pages

Use `canonic_mcp_kit::page` when a tool or library operation returns records,
search matches or stream chunks. It checks requests and cursors and admits
items incrementally, so a handler can stop reading its store when the page is
full. MCP's protocol catalogs use their [own pagination](pagination.md).

## Build a page

This example uses an in-memory fixture. A real store should seek strictly after
the decoded key and read only as far as the page needs.

```rust
use canonic_mcp_kit::page::{self, Builder, Casing, Limits, Position, Request, Scope};
use serde_json::json;

let arguments = json!({"limit": 2});
let limits = Limits::STANDARD;
let request = Request::from_mcp(&arguments, limits)?;
// Include every effective filter and ordering choice in canonical query bytes.
let scope = Scope::new("acme.record.list", page::digest(b"all;order=id-ascending"));
let after = request.cursor.as_deref().map(|c| scope.decode(c)).transpose()?;
let after = match after {
    Some(Position::Keyset(bytes)) => Some(bytes),
    None => None,
    _ => return Err(page::Error::InvalidCursor),
};
// Reserve envelope space before selecting the items-array transport allowance.
let mut builder = Builder::new(&scope, request.limit, limits, 1024 * 1024, "record_read")?;
let mut exhausted = true;
for id in ["a", "b", "c"].into_iter().filter(|id| {
    after.as_deref().is_none_or(|key| id.as_bytes() > key)
}) {
    if !builder.push(json!({"id": id}), &Position::Keyset(id.as_bytes().to_vec()))? {
        exhausted = false;
        break;
    }
    if builder.is_full() {
        exhausted = false; // No lookahead: more may exist.
        break;
    }
}
let result = builder.finish(!exhausted).render(Casing::Snake);
assert_eq!(result["items"], json!([{"id":"a"},{"id":"b"}]));
assert!(result["next_cursor"].is_string());
# Ok::<(), page::Error>(())
```

Pass the returned string as `cursor` with the same filters. Follow it until
`next_cursor` is null; a conservative continuation can lead to an empty last
page. `Casing::Camel` uses `nextCursor`. Keep your unique ordering tie-breaker
inside the key. Keys can contain arbitrary bytes up to `page::MAX_KEY_BYTES`
(110 bytes); use a compact
reversible store key if a composite key is larger. Hashing a sort key loses the
position needed to seek.

## Declare inputs and handle errors

`Limits::STANDARD` is 50 items by default, at most 100, with a 256 KiB encoded
items-array budget. Declare smaller custom values with `Limits { default, max,
max_bytes }.validate()?`. Ordinary request parsers enforce the standard ceiling
automatically. Streams name their units and use `validate_units`, `unit_limit`
and `Request::from_mcp_units` / `from_cli_units` for their declared count limits.
Out-of-range requests fail rather than clamp.

Merge `limits.properties()` into the tool's input-schema `properties`. Merge
`page::output_properties(casing)` into output properties and put the selected
continuation name in `required`. The input cursor schema leaves syntax checks
to the helper, preserving the `invalid_cursor` error category.

For a CLI, declare your own `--cursor <CURSOR>` and `--limit <N>` flags. Use
`Request::from_cli(cursor.as_deref(), limit, limits)` or `page::arguments` to
build MCP-shaped arguments with omitted optional values. Library stores receive
`Limit` and `Position` values. No CLI framework is required by this module.

`Error::code()` provides stable categories. Invalid cursors say to restart
without a cursor. Never silently restart. A cursor binds the list, query and
format version but grants no access rights. Its checksum detects accidental
edits; it is not a signature. Check authorization and validate store-key bounds
on every request.

## Immutable content and byte budgets

For finished logs and static bundles, use
`Scope::offset(list, query_digest, content_revision_digest)` and
`Position::Offset(next_unread_unit)`. A changed revision refuses the cursor.
Mutable lists use keysets. Live streams retain a keyset cursor at their current
end and separately report that they are running.

Pass already serialized JSON values to `push`, and stop on false or `is_full`.
False means the item was not admitted. A first item larger than the page budget
is returned alone when it fits the transport allowance. Otherwise the error
names the single-item read tool you supplied. The allowance covers the items
array after reserving any result envelope and duplicated text content.

For bundled guide tools, see [Bundle your documentation](documentation.md).
