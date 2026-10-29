# Use Shield from an agent (MCP)

MCP lets an agent inspect Shield policy, explain decisions, read guides, and
propose changes. You approve changes through the host or terminal. Native hooks
check ordinary tool requests separately; MCP alone does not install enforcement.

For third-party servers and pack-contributed MCP permissions, see
[project MCP connections](mcp-connections.md).

## Start with a task

| You want to… | Start here |
| --- | --- |
| Check the active policy and setup | `status`, then `doctor` if needed. |
| Find a suitable pack | `pack_search`, then `pack_info` to inspect a result. |
| Understand an existing rule | `rule_list` with its owner-qualified reference. |
| Learn to write a rule | [Read the bundled guides](#reading-shields-documentation). |
| Change policy | [Propose and review a change](#proposals-and-operator-review). |
| Evaluate a hypothetical request | [Use diagnostic input](#diagnostic-input-and-snapshots); this does not execute it. |

For a pack walkthrough, see [reviewing an available pack](#reviewing-an-available-pack).

## Connect the server

Register MCP together with native hooks through the operator CLI. Run
`shield init` in your project, then add your chosen agent if you did not select
it during initialization:

```sh
shield mcp setup --agent claude
```

Use `shield mcp setup --agent codex` for Codex. Restart and trust the agent as described in
[your first project](getting-started.md#2-load-the-agent-integration). The
server uses that project's confirmed active policy. You do not need to start
it manually when using these registrations.

For client implementation, see [transport details](#responses-and-transport-lifetime).

| Category | Tools |
| --- | --- |
| Status | `status`, `doctor` |
| Packs and registries | `pack_list`, `registry_list`, `pack_search`, `pack_info` |
| Policy and rules | `policy_show`, `rule_list`, `suggestion_list`, `review_defer`, `policy_propose`, `policy_apply`, `policy_withdraw`, `policy_receipt` |
| Approvals and history | `request_list`, `history_list` |
| Diagnostics | `policy_evaluate`, `policy_compare` |
| Documentation | `shield_docs_list`, `shield_docs_search`, `shield_docs_read` |

Packs, rules, policy, approvals and history support list/show through optional
selectors (`reference`, `rule_ref`, `proposal_id`, `request_id`, `entry_id`). A
selector cannot coexist with filters or paging. Lists accept a limit of 1–100
(default 50) and an opaque cursor bound to that connection and unchanged query.
To continue, copy `next_cursor` into `cursor` on the same tool and connection,
preserving all other arguments, including the limit and omitted fields. If the
cursor is unavailable, restart the list without `cursor`.
Retained cursor results remain stable while project data changes. Rules use
owner-qualified references: custom rule ID, or installed pack selector plus rule ID.
History returns joined request entries; configuration proposals and diagnostics
create no request entries or action requests.

## Reading Shield's documentation

The executable bundles every Markdown page under Shield's `docs/` directory at
build time, including nested pages. The bundle is compressed and decoded in
memory by Shield; no external decompressor is needed. These tools work offline
and return the binary's version; they do not read project files or fetch online documentation.
Page paths are relative to that directory, such as `policy-rules.md`.

Fresh integrations approve the documentation tools without asking by default.
Existing integrations keep their saved tool approvals. After installing a build
with additional tools, rerun `shield mcp setup --agent codex` or `shield mcp setup --agent claude` and
mark the documentation tools on the Tool permissions page, then restart the
agent session. Rerunning setup preserves saved selections until you change them.

- `shield_docs_list`: list page paths, titles and line counts. Optional `offset`
  starts at zero; `limit` is 1–100, default 50. Continue with `next_offset`.
- `shield_docs_search`: find case-insensitive literal `query` text in the guides.
  Surrounding spaces are significant; whitespace-only queries are invalid.
  Optional `path` selects one listed page. Results contain `path`, one-based
  `line` and matching `text`, in path and line order. Use `offset` and `limit`
  as for listing; continue with `next_offset` and the same query and path.
- `shield_docs_read`: read a listed `path`, optionally starting at `start_line`
  (one-based, default 1). `limit` is 1–400 lines, default 100. The result includes
  `content`, `total_lines` and `next_start_line` for continuation.

An unknown page returns `not_found` and directs the agent to `shield_docs_list`;
these are catalog paths, not filesystem paths. Searches with no matches return
an empty list, and a read past the last line returns empty content. Pagination
is stable for the lifetime of the installed binary.

Before drafting a reusable command rule, search for the relevant matching
grammar and read its explanation and examples. Inspect the current rules with
`rule_list` so reviewed variants can extend an existing custom rule.

### Example: find the argument-matching guide

Call `shield_docs_search` with these arguments:

```json
{"query":"operands"}
```

The result includes matching page paths, line numbers, and text. Choose a
relevant match in `policy-rules.md`. To read that guide from its beginning,
call `shield_docs_read` with:

```json
{"path":"policy-rules.md","start_line":1,"limit":100}
```

The result's `content` contains Markdown. If `next_start_line` is present and
non-null, use that number as the next `start_line` to continue reading. To jump
to a search result instead, use its returned `line` as `start_line`; do not
assume that line numbers are the same in another installed build.

For a new reader, read `introduction.md` or `README.md` first. These tools
expose the same public guides used by people configuring Shield.

## Reviewing an available pack

Call `pack_info` with a `reference` for a compact summary: the selected release
and provenance, description, rule count, declared outcomes for each profile,
permission groups, variables, preconditions and dependencies. Repeated command
variants share an explanation and count. These are declared rule outcomes;
conditions, requirements, configuration and other active policy still apply.
The summary contains no full rule bodies and stays within 16 KiB. Large summaries
identify omitted sections in `missing` and set `truncated`.

For exact evidence, use the same tool with `rule_id`, or set `view` to `rules`
and optionally `query` to filter rule IDs and reasons. Rule listings default to
10 entries per page; `limit` accepts 1–100. Continue with `next_cursor` and the
same reference, view, query and limit. Cursor pages retain the acquired evidence
even if the registry changes. A `rule_id` cannot coexist with view, filters or
paging. Summaries do not accept filters or paging.
Use `view: "settings"` for complete variable definitions, preconditions and
dependencies, including sections omitted from a large summary. This view also
does not accept filters or paging.

```json
{"reference":"main/canonic/node"}
{"reference":"main/canonic/node","view":"rules","query":"install","limit":5}
{"reference":"main/canonic/node","rule_id":"npm-install"}
```

Review through these tools directly; exporting the complete pack to a file or
processing it with shell tools is unnecessary. Inspection and proposal preparation
need no policy approval. Explain the relevant permissions, propose the change,
then submit its exact `apply_arguments` to `policy_apply` for the user's
approval. Declining leaves the active policy unchanged.

### Example: propose a pack installation

After searching and inspecting a pack, call `policy_propose` with the
following shape. Replace `PACK_REFERENCE` with the exact reference you reviewed:

```json
{
  "change": {
    "kind": "pack_add",
    "reference": "PACK_REFERENCE",
    "profile": "recommended"
  },
  "reason": "Use the reviewed Recommended permissions for this project's tool."
}
```

The response includes the proposal ID and `apply_arguments`; policy is unchanged.
Explain the scope, then copy `apply_arguments` unchanged into `policy_apply`.
Wait for the host approval and result. A successful application returns a receipt;
declined or unavailable approval leaves the proposal inactive. A stale proposal
needs fresh preparation. See the [review reference](#proposals-and-operator-review).

## Proposals and operator review

`policy_propose` accepts one tagged `change` or a `changes` array of
1–16 changes. A batch applies in order to one candidate; approval activates all
changes together or none. Optional fields include a reason, history and
supersession links, and an ASCII `idempotency_key` of 1–128 characters.

| Change | Purpose |
| --- | --- |
| `rule_add`, `rule_edit`, `rule_remove` | Add, replace, or remove a custom rule. |
| `pack_add`, `pack_update`, `pack_remove` | Install, update, or remove a pack and its dependencies. |
| `pack_configure` | Set variables or rule overrides on an installed pack. |
| `access_path_add`, `access_path_remove` | Add or remove one absolute read/write directory. |

Every change except `pack_update` and `pack_remove` accepts `layer: project`
(default) or `local` for uncommitted local configuration. Rules use the project
condition schema.

`pack_configure` requires an installed `reference` and at least one of `set`,
`unset`, `override`, or `clear_override`. Overrides map rule IDs to `off`,
`allow`, `ask`, `deny`, or `{on_unmet}`.

Pack proposals use CLI resolution and verified acquisition. They require enrolled
sources and capture the full compatible dependency set for offline confirmation.
Pins and explicit profiles are preserved. Removal prunes unused dependencies;
a required pack returns `dependency_required` with its dependents. Packs cannot
enroll sources or change trust keys. See
[registry workflows](local-registry.md#updating-and-removing-dependency-packs).

### Apply the reviewed proposal

Copy the returned `apply_arguments` object unchanged into `policy_apply`.
Shield compares the complete object with the saved proposal; editing its text
or reference cannot authorize another change. To recover from a mismatch,
inspect `policy_show` with `proposal_id` and copy its `apply_arguments`.

Claude receives the proposal ID, digest, change list, and review summary.
Codex receives readable `change`, optional `details`, and a short `reference`.
Detailed evidence remains available through proposal inspection.

The host confirmation describes pack/profile, rule, setting, or folder changes.
It shows permissions and comparison limits, including before/after matching
scope and removed restrictions. Supplied rule names are labeled as such;
shortened or omitted constraints are identified.

| Host | Approval |
| --- | --- |
| Claude | An MCP confirmation form. Accept applies the proposal; decline, cancel, disconnect, or timeout leaves it pending. |
| Codex | The per-call tool prompt. Allow applies; Cancel leaves it pending. `never` approval policy cannot run apply. |
| Other or unrecognized host | Review and activate with `shield policy review` in a terminal. |

Claude's form works in `default`, `auto`, and `bypassPermissions` modes and
requires its own Accept response, independently of ordinary tool approval.
The client must advertise form elicitation or apply returns
`approval_unavailable`. Only one form can be open per connection, with a
five-minute response deadline. It waits outside the worker so other calls can
finish. Codex controls its prompt's heading, buttons, and field rendering.

Shield rechecks arguments and current policy before committing. Stale or changed
candidates need a fresh proposal and approval. Success returns a receipt;
repeating an applied proposal returns that receipt. Terminal review through
`shield policy review` is also available.

Inspection and proposal preparation run without prompts by default; setup
preserves any tool-permission choices you changed. Native hook messages keep
proposal payloads separate from their user-facing explanation.

### Replay and withdrawal

Without an idempotency key, identical arguments return the same proposal;
changed arguments create another. With a key, the same input returns the saved
identity and candidate even after state changes; changed input is refused.
A mapped host session permits reconnect replay. Otherwise the key is
connection-scoped.

If `policy_propose` reports a deadline error, its inactive proposal may
already be saved. Retry the identical arguments on the same connection to
recover the proposal and its approval arguments, or inspect pending proposals
with `policy_show`. A timeout does not establish that the proposal was absent
or that policy changed. Report an unresolved failure instead of silently
dropping the offer.

Only the original registered proposer can withdraw a proposal. Repeating an
owned withdrawal makes no further change. Unrecognized hosts can inspect and
propose, but advertising form support does not grant activation authority or
install native hooks.

## Rule suggestions

After completing a task, call `suggestion_list` with the hook-reported
`session_id`. It lists recorded policy Ask requests since the last activation,
including the reason, count, any pack request for a project rule, and observed
`request.executable`, `arguments`, and optional `cwd`. Each command in a
compound request appears separately. Command rows supply evidence, not rule drafts.

Check `coverage` before treating the list as complete. Disabled, unavailable,
evicted or scan-limited history can leave missing evidence. Partial command
analysis appears as `review_status: unsupported`, with a history-inspection
`next_action`; it does not produce an inferred argv or rule draft. Lists default
to 16 rows. Copy `next_cursor` into `cursor` with the same `session_id` and
`limit` to continue the same view.

`times_asked` counts recorded decisions, not user approvals. The separate
`intentional_invocations` count excludes duplicate tool identities and explicit
sandbox escalation retries. Missing identity makes recurrence unknown and keeps
`eligible_for_offer` false. Offer one grouped custom-rule review when a group
has two intentional invocations, or when the user explicitly asks for reusable
permission. Finish already-authorized work first. Unique pack offers follow
their own once-per-conversation instruction.

Each row includes a stable `review_id`, `next_action` and `review_status`.
Structured results also include complete canonical `authoring_examples` and
host-specific `approval_guidance`, so an ordinary review does not need a docs
search. Inspect effects and relevant existing rules before adapting an example.
Copy `next_action.proposal_context` (`source_entry_id` and `review_ids`) into the
proposal alongside its canonical `change` or `changes`. A linked pending offer
returns its proposal-inspection route instead of another automatic offer.

To defer an unchanged review, call `review_defer` with its `session_id`
and an array of current `review_ids`. Deferral survives reconnect and history
clearing; it changes no policy or approval. Storage holds at most 1,024 deferred
IDs and returns a capacity error rather than silently forgetting a choice.
Declined or cancelled approval must not be retried without a new user request.

Recognized read-only curl/wget downloads instead include a web-fetch
`suggested_change`, grouped by site. Local-server suggestions cover
`localhost`, `127.0.0.1`, and `[::1]` on any port. Activation clears the list.

### Draft reusable rules

Offer rules for recurring tasks, skipping one-off lookups such as help output.
Start from `authoring_examples` or the complete examples in
`policy_propose`'s `inputSchema`.
The schema describes argument and path predicates at their definitions. Use
`shield_docs_search` and `shield_docs_read` for additional grammar details.
Inspect scripts, lifecycle hooks, and option effects before proposing. Group
reviewed variants in one rule, and inspect `rule_list` before adding a
duplicate. If effects are unknown, investigate or skip the offer.

For example, after reviewing `dev`, `predev`, `postdev` and the code they execute,
adapt this complete `policy_propose` call to allow only `npm run dev`
from the project root, with no extra arguments:

```json
{
  "change": {
    "kind": "rule_add",
    "rule": {
      "id": "allow-reviewed-dev",
      "decision": "allow",
      "conditions": {
        "domain": "argv",
        "executable": "npm",
        "arguments": {
          "kind": "options",
          "prefix": ["run", "dev"],
          "flags": [],
          "values": [],
          "operands": false
        },
        "cwd": {"exact": {"root": "project"}}
      }
    }
  },
  "agent_reason": "Allow the reviewed dev command from the project root; later script edits also execute under this permission."
}
```

This creates an inactive proposal. Copy its returned `apply_arguments` unchanged
into `policy_apply` for user approval. The `exact` path predicate takes an
object; `{"exact":"/workspace/acme"}` is invalid here. Using `within` instead
includes subdirectories, so choose it only when that broader scope is intended.
Script contents are reviewed by the agent; this rule does not pin their bytes.

Project rules must work across checkouts. `policy_propose`, `shield rule add`,
and `shield rule edit` reject machine paths in conditions or requirements,
including project, home, temporary, and agent
configuration folders. The error suggests a portable form, such as replacing
`--prefix /path/to/project` with `cwd within {root: project}`. Edits preserving
conditions and requirements are exempt; exact-request rules keep their bound cwd.
Observed machine paths are marked `not_portable` in request evidence.

After reviewing a test script and its watch option, this argument grammar covers
`npm test` and `npm test -- --watch`:

```yaml
arguments:
  kind: options
  prefix: [test]
  flags: []
  values: []
  operands:
    match: {exact: --watch}
    min: 0
    max: 1
```

The rule also fixes `executable: npm`. After `--`, the watch flag is an operand
matched literally. Other scripts, flags, extra operands, and repeated watch flags
miss this rule. Optional flags that load code, write elsewhere, or change
credentials need separate review.

### Submit for approval

After the task, explain each suitable rule's scope and examples, propose it,
then submit its `apply_arguments` directly to `policy_apply` before the
final reply. The host's approval prompt is the question; no preliminary question
is needed. Wait for each result before the next proposal. Declining or cancelling
keeps the existing policy. Unique pack offers use the same flow, once per
conversation.

`policy_receipt` reads committed receipts; pending proposals return
`operator_review_required`. It cannot apply a change. Stored action requests
are separate: inspect `request_list`, then use `shield request approve` or
`shield request deny` in an operator terminal.

## Diagnostic input and snapshots

`status` and `policy_show` expose `active_candidate_id`. Submission or
inspection of a proposal exposes `before_candidate_id` and `after_candidate_id`.
Handles retain immutable bytes until the connection closes; policy activation
cannot retarget them. Reconnect requires new handles. Unknown/foreign handles return
`candidate_unavailable`. Each connection retains at most 256 distinct snapshots;
capacity is reserved before persisting a proposal that needs new handles.

`policy_evaluate` accepts typed input, its preserved vendor `original_input`, cwd
within the enrolled project, and a nullable explicit environment map. It selects
active policy by default, or a retained `candidate_id`. For example:

```json
{
  "name": "policy_evaluate",
  "arguments": {
    "input": {
      "kind": "argv",
      "executable": "git",
      "arguments": ["status"],
      "original_input": {"command": "git status"}
    },
    "context": {
      "cwd": "/workspace/acme",
      "environment": {"PATH": "/usr/bin:/bin"}
    }
  }
}
```

This shows a tool call; pass the inner `arguments` object when your client
already selects the tool by name. Replace the example cwd with an existing
directory inside the registered project and supply the environment available
for the request you are investigating. The result depends on that project's
active policy; `git status` is not universally allowed.

Read `data.decision` for the policy result and the returned matching-rule and
requirement evidence for its explanation. Keep `candidate_id` if you want to
inspect rules from that same snapshot. Changing the arguments or cwd may change
the result. This diagnostic does not run Git or supply permission for a later
tool call.

Supported input kinds are argv, shell, file, web, mcp and unknown. Literal argv and
structured POSIX/Bash analysis reuse the Rust evaluator. Exact-request conditions
bind typed input, all vendor fields, canonical cwd, environment and derived launch
path. Unknown context cannot establish an exact allowance. Broader file/web effect
matching and complete semantic comparison have incomplete coverage and do not
manufacture permission. Powershell/Cmd analysis is unsupported.

`policy_compare` selects two handles and `sample`, `literal_argv` or
`supported_semantic`. Only sample accepts input/context. A sample is one witness;
matching sample decisions cannot prove whole-policy equivalence. Unsupported
normalized comparison regions explicitly return incomplete/unknown evidence.
Sample reports keep the overall relation unknown and identify any decision change
inside the request witness.
Diagnostic Allow has preview disposition and supplies no execution or grant.

## Responses and transport lifetime

### Client startup

Shield exposes project inspection, approved policy changes, and diagnostic
evaluation over stdio using MCP `2025-06-18` and `2025-11-25`. Other clients can
launch `shield mcp serve --project /workspace/acme` explicitly, replacing
the example path with an initialized project. Startup failure exits 2 with a
fixed stderr diagnostic and empty stdout.

A launch may supply an agent and an explicitly mapped host session through
internal registration arguments; tool input cannot appoint either. Client names
are declarations. Shield generates a new connection ID on each launch.

The client initializes with a supported protocol version, sends
`notifications/initialized`, then uses `tools/list` or `tools/call`. The server
supports ping and cancellation and advertises only tools. Every tool publishes
strict input and output schemas, with authority annotations. Shield echoes a
supported requested version. For other version strings it offers `2025-11-25`;
the client decides whether it supports that version.

### Result format and limits

For compact packs, `rule_list` lists canonical rule bodies. Its optional
`shared` array includes each pack selector, invocation groups and command sets.
Bindings are pack-only source fields. For a rule present in the active policy,
a `rule_ref` selects the materialized rule, including its matched leading
arguments and command member, without listing every invocation combination.
Pass the evaluation's `candidate_id` with `rule_ref` to inspect that exact retained
snapshot, including an old active generation or pending proposal. Omitting the
handle selects active policy at lookup time. Lists also accept `candidate_id`;
preserve it with the other arguments when following `next_cursor`. Exact rule
selection accepts the handle but no paging arguments. An unavailable handle
returns `candidate_unavailable` without selecting another snapshot.
Custom-rule inspection includes shared and local rules;
a local rule with a shared rule's ID replaces it in lists and exact lookups.
Inspection uses the same strict source parser and budgets
as policy admission.

Results retain the complete envelope in `structuredContent`. Text block 0 gives
short guidance; text block 1 is bounded JSON with `data`, `error`, `missing` and
`truncated`. Inspection tools include requested data there. Proposal/withdrawal
text includes IDs, status and candidate handles; use `policy_show` to inspect
the complete proposal. Text-only clients parse block 1 rather than block 0. The shared
envelope has schema_version, project_id, nullable policy_revision, data, missing,
truncated and nullable error. Allow/Ask/Deny diagnostics are successful results.
Failures set `isError` and use fixed safe codes/messages with an applicable operator
route. Secrets are redacted from exposed evidence; credentials cannot be embedded
in project rule conditions.

JSON-RPC framing uses newline-delimited UTF-8, with no batches or Content-Length
headers. Duplicate decoded keys, NULs, positional parameters, invalid IDs and
unknown fields refuse. Arguments that do not match a tool's input schema return
an `invalid_input` tool result. Frames are limited to 524,288 bytes plus the
newline; responses to 4 MiB plus newline; pages to 1 MiB. Each connection's snapshot handles
may pin up to 1 GiB of source and compiled bytes, enough for the largest bundle and
plan that policy limits allow; retained cursor views have a 64 MiB budget and at
most 256 continuation cursors. Used cursors remain until disconnection; replaying
one can allocate another continuation. Exhaustion returns `overloaded`; reconnect
and start a new list to release the old views. No capacity limit silently evicts a
live handle. A session accepts at most 1,024 inbound messages; the next one
ends it with `-32000` "Session message limit reached".

One worker processes tools while the dispatcher handles input and ping. At most
eight tool calls may be outstanding, including one waiting for confirmation;
more are refused with `-32000`. Duplicate outstanding IDs close the session.
`--request-timeout-ms` bounds queued work, evaluation and encoding
(default 5,000 ms, range 1–60,000); a call that exceeds it receives `-32001`
"Request deadline exceeded". Cancellation suppresses pending responses.
Policy apply uses that budget for preparation, a separate five-minute form wait,
then a fresh ordinary budget for queued revalidation, commit and encoding.
Checkpoints do not interrupt native blocking I/O; clients also own process/I/O
limits. Lost proposal replies can be recovered by resubmitting the same arguments.

Keep stdin open until you have the replies you need. When it closes, queued
`policy_propose`, `policy_withdraw` and `policy_apply`
calls are refused with `-32800` and never run, a pending confirmation is
withdrawn, and read-only tools still finish.

Closing stdin drains admitted work and joins the worker before exit 0. Framing or
transport failure cancels and joins work and exits 2. Resident clients complete
each call from its correlated envelope; process exit supplies no missing result or
permission. Native blocking stdin readers end at process exit after early failure.

Native macOS tests and pinned official SDK fixtures exercise this interface; live-agent
setup and Linux/Windows runtime coverage remain separate validation work.
