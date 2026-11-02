# API map

| You want to | Use |
| --- | --- |
| Publish tools | `Tool`, `Annotations`, `Catalog::new` |
| Publish bundled workflows | `SkillCatalog`, `SkillDefinition`, `SkillFile`, `SessionOptions::skills` |
| Publish your guides as documentation tools | `docs::build::embed` (feature `docs-build`), `include_docs!`, `docs::Bundle`, `docs::Documentation`, `docs::DocsError` |
| Enable modern protocol handling | `ModernOptions`, `SessionOptions::modern`, `Session::try_new` |
| Enable optional task handles | `task::TaskOptions`, `SessionOptions::tasks`, `RequestContext::task_eligible` |
| Collect fields, choices or URL consent | `elicitation::Form`, `elicitation::Request`, `elicitation::Requests`, `elicitation::Response` |
| Drive rich legacy waits | `Session::request_elicitation`, `Inbound::Elicitation`, `stdio::Elicit`, `stdio::Outcome::Elicit`, `stdio::ElicitationPolicy` |
| Validate modern rounds or task input | `RequestContext::elicitation`, `RequestContext::elicitation_answers`, `task::State::RichInputRequired`, `task::Snapshot::input_answers` |
| Handle task operations | `task::TaskRequest`, `Inbound::Task`, `stdio::Request::Task`, `stdio::Outcome::Task`, `task::TaskResult` |
| Validate persisted task state | `task::Metadata`, `task::Snapshot`, `task::State`, `task::Timestamp`, `task::InputHistory` |
| Encode task replies in another runtime | `RequestContext::encode_task` |
| Inspect per-request metadata and retries | `Context::request_context`, `Session::request_context`, `RequestContext` |
| Set cache freshness and scope | `CachePolicy`, `CacheScope`, `ModernOptions` |
| Publish resources and templates | `Resource`, `ResourceTemplate`, `ResourceCatalog::new`, `SessionOptions::resources` |
| Return resource contents | `ResourceContents`, `ReadResourceResult`, `stdio::Outcome::Resource` |
| Return tool artifacts | `Content::resource_link`, `ResourceLink`, `Content::resource`, `ResourceContents`; see [artifact results](../guides/resources.md#return-artifacts-from-tools) |
| Refuse an unknown resource | `Outcome::ResourceNotFound` for revision-aware replies; `resource::not_found` emits a legacy error |
| Run a stdio server | `stdio::serve`, `stdio::Handler`, `stdio::Options` |
| Choose where a request runs | `Handler::admit` returning `stdio::Admission`; `stdio::Lane` |
| Return a result | `stdio::Outcome::Tool` with `ToolResult` and `Content` |
| Return audio | `Content::audio`, `AudioContent`, `ContentError`; see [audio results](../guides/audio.md) |
| Attach presentation hints | `ContentAnnotations`, `Role`, `Content::with_annotations`, `Resource::annotations`, `ResourceTemplate::annotations` |
| Reply to an extension request | `stdio::Outcome::Method`; list methods in `SessionOptions` |
| Stop work on cancellation or deadline | `stdio::Context::check`, `Context::remaining` |
| Report progress | `stdio::Context::progress` |
| Ask the host's user | `stdio::Outcome::Confirm` with `stdio::Confirm`; `Decision` |
| Ask on a modern request | `RequestContext::confirmation`, `Outcome::InputRequired`, consumer-owned state validation and `confirmation_decision` |
| Change invalid-argument results | `Handler::invalid_arguments` |
| Record frames for diagnostics | `stdio::Options::observer` |
| Drive the protocol from another runtime | `Session`, `Inbound`, `SessionOptions` |
| Encode modern replies in another runtime | `RequestContext::encode_success` for complete results; `RequestContext::encode_interim` for `InputRequiredResult` |
| Encode revision-checked modern errors | `RequestContext::encode_error`; low-level `rpc::encode_failure` only builds the bounded envelope |
| Publish and render prompts | `Prompt`, `PromptArgument`, `PromptCatalog`, `PromptCall`, `PromptMessage`, `GetPromptResult`; `Request::Prompt` and `Outcome::Prompt` |
| Suggest prompt/template argument values | `SessionOptions::completions`, `CompleteRequest`, `CompletionReference`, `CompletionArgument`, `CompleteResult`; `Request::Completion` and `Outcome::Completion` |
| Page tool discovery | `Catalog::list_page` with parameters and protocol revision; `Catalog::listing` and `Catalog::tools` provide the full catalog |
| Prepare reusable JSON Schema validation | `schema::PreparedSchema`, `schema::SchemaError`; see [schemas](../guides/schemas.md) |
| Validate JSON once | `schema::validate`, `schema::check`, `schema::ArgumentErrorKind` |
| Check a producing tool's result | `Catalog::result_schema`, `ResultSchema`, `Context::result_schema`, `RequestContext::result_schema` |
| Check durable results before commit and after restore | `ResultSchema::validate_snapshot` |
| Encode checked legacy tool results | `ResultSchema::encode_legacy` |
| Parse or encode strictly within a limit | `json::parse`, `json::encode`, `json::encode_checked`, `json::measure` |
| Build JSON-RPC frames | `rpc::success`, `rpc::failure`, `rpc::notification`, `rpc::request`, `rpc::encode_success` |
| Cancel your own work | `Control` |

Build the API reference with `make docs` from `packages/mcp-kit`; it opens from
`target/doc/canonic_mcp_kit/index.html`.
