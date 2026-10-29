# API map

| You want to | Use |
| --- | --- |
| Publish tools | `Tool`, `Annotations`, `Catalog::new` |
| Publish bundled workflows | `SkillCatalog`, `SkillDefinition`, `SkillFile`, `SessionOptions::skills` |
| Enable modern protocol handling | `ModernOptions`, `SessionOptions::modern`, `Session::try_new` |
| Inspect per-request metadata and retries | `Context::request_context`, `Session::request_context`, `RequestContext` |
| Set cache freshness and scope | `CachePolicy`, `CacheScope`, `ModernOptions` |
| Publish resources and templates | `Resource`, `ResourceTemplate`, `ResourceCatalog::new`, `SessionOptions::resources` |
| Return resource contents | `ResourceContents`, `ReadResourceResult`, `stdio::Outcome::Resource` |
| Refuse an unknown resource | `Outcome::ResourceNotFound` for revision-aware replies; `resource::not_found` emits a legacy error |
| Run a stdio server | `stdio::serve`, `stdio::Handler`, `stdio::Options` |
| Choose where a request runs | `Handler::admit` returning `stdio::Admission`; `stdio::Lane` |
| Return a result | `stdio::Outcome::Tool` with `ToolResult` and `Content` |
| Reply to an extension request | `stdio::Outcome::Method`; list methods in `SessionOptions` |
| Stop work on cancellation or deadline | `stdio::Context::check`, `Context::remaining` |
| Report progress | `stdio::Context::progress` |
| Ask the host's user | `stdio::Outcome::Confirm` with `stdio::Confirm`; `Decision` |
| Ask on a modern request | `RequestContext::confirmation`, `Outcome::InputRequired`, consumer-owned state validation and `confirmation_decision` |
| Change invalid-argument results | `Handler::invalid_arguments` |
| Record frames for diagnostics | `stdio::Options::observer` |
| Drive the protocol from another runtime | `Session`, `Inbound`, `SessionOptions` |
| Encode modern replies in another runtime | `RequestContext::encode_success` for complete results; `RequestContext::encode_interim` for `InputRequiredResult` |
| Validate JSON against a tool schema | `schema::validate`, `schema::check` |
| Parse or encode strictly within a limit | `json::parse`, `json::encode`, `json::encode_checked`, `json::measure` |
| Build JSON-RPC frames | `rpc::success`, `rpc::failure`, `rpc::notification`, `rpc::request`, `rpc::encode_success` |
| Cancel your own work | `Control` |

Build the API reference with `make docs` from `packages/mcp`; it opens from
`target/doc/canonic_mcp/index.html`.
