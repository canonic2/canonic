# API map

| You want to | Use |
| --- | --- |
| Publish tools | `Tool`, `Annotations`, `Catalog::new` |
| Run a stdio server | `stdio::serve`, `stdio::Handler`, `stdio::Options` |
| Choose where a request runs | `Handler::admit` returning `stdio::Admission`; `stdio::Lane` |
| Return a result | `stdio::Outcome::Tool` with `ToolResult` and `Content` |
| Reply to an extension request | `stdio::Outcome::Method`; list methods in `SessionOptions` |
| Stop work on cancellation or deadline | `stdio::Context::check`, `Context::remaining` |
| Report progress | `stdio::Context::progress` |
| Ask the host's user | `stdio::Outcome::Confirm` with `stdio::Confirm`; `Decision` |
| Change invalid-argument results | `Handler::invalid_arguments` |
| Record frames for diagnostics | `stdio::Options::observer` |
| Drive the protocol from another runtime | `Session`, `Inbound`, `SessionOptions` |
| Validate JSON against a tool schema | `schema::validate`, `schema::check` |
| Parse or encode strictly within a limit | `json::parse`, `json::encode`, `json::encode_checked`, `json::measure` |
| Build JSON-RPC frames | `rpc::success`, `rpc::failure`, `rpc::notification`, `rpc::request`, `rpc::encode_success` |
| Cancel your own work | `Control` |

Build the API reference with `make docs` from `packages/mcp`; it opens from
`target/doc/canonic_mcp/index.html`.
