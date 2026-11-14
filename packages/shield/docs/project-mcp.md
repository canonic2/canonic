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
| Approve repeated operations for this conversation | [Session grants](session-grants.md). |
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
| Packs and registries | `pack_list`, `pack_show`, `registry_list`, `registry_add`, `registry_remove`, `registry_default`, `pack_search`, `pack_info`, `pack_add`, `pack_configure`, `pack_update`, `pack_remove` |
| Policy and rules | `policy_show`, `rule_list`, `rule_add`, `rule_edit`, `rule_remove`, `suggestion_list`, `review_defer`, `policy_propose`, `policy_apply`, `policy_withdraw`, `policy_receipt`, `policy_defer`, `policy_draft_list`, `policy_draft_remove`, `policy_reject`, `policy_repair` |
| Approvals and history | `request_list`, `request_show`, `request_approve`, `request_deny`, `history_list`, `history_show`, `history_configure` |
| Session grants | `session_grant_list`, `session_grant_show`, `session_grant_propose`, `session_grant_reconsider`, `session_grant_approve`, `session_grant_revoke` |
| Administration | `enable`, `disable`, `protection_set`, `cache_prepare`, `mcp_setup` |
| Diagnostics | `policy_evaluate`, `policy_compare` |
| Documentation | `docs_list`, `docs_search`, `docs_read` |

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

## Administration and recovery

Administrative tools show the person an exact captured review through the host.
Tool permission and a chat response cannot confirm the change. Stale reviews
refuse; inspect the result before requesting a fresh review.

- `protection_set` changes protected-file access choices. Supply one or both of
  `agent_settings_read` (`ask`/`allow`) and `agent_settings_change`
  (`block`/`allow`). Shield's policy files, review state and cache stay
  protected whatever the choice.
- `disable {}` pauses native interception for this project;
  `enable {}` resumes it. Each requires Shield's human confirmation form,
  including while disabled. Policy and registration are preserved. Disabled
  hooks retain protected-file and operator-command guards, then defer other
  requests to host permissions. MCP remains available. Restart agents when
  requested to refresh generated permission rules.
  See [pause or resume Shield](project-review.md#pause-or-resume-shield).
- `registry_add` accepts `name` and a local directory or HTTPS `location`.
  Omit either to ask the person for the missing details in a form. If scope is
  omitted in that form workflow, the person must choose `project` or `user`.
  Submission returns exact `retry_arguments`; call `registry_add` with those
  arguments for the separate trust review. Input collection does not read the
  source or enroll it, and defaults never supply missing answers.
  It discovers and validates the source, then asks the person to review its
  canonical location, registry identity, signing-key fingerprint and trust basis.
  Discovery grants no trust; acceptance enrolls exactly the reviewed source.
  Relative directories resolve against this project. `registry_remove` and
  `registry_default` accept `name`. Optional `scope` defaults to `project`;
  `user` explicitly changes discovery for other projects too. Installed policy
  is preserved.
  If a pack needs an unenrolled dependency source, use `registry_add` for that
  source and retry the pack request after acceptance. Ask for its location if
  unknown. Registry trust and permission-policy approval are separate reviews.
- `policy_repair` restores managed projections from committed policy.
  `policy_reject` asks the user to reject a pending `proposal_id`.
- `history_configure` accepts `enabled`, retention `limit` (1–10000) and
  `purge`. Clearing history requires confirmation.
- `mcp_setup` accepts registered `agents`, optional `remove` and `dry_run`.
  Dry runs return the captured review without writing. Setup is project scoped;
  restart the affected connection after changing its registration.
  For conflicting or duplicate Shield hooks or a conflicting native Shield MCP
  registration, pass `repair: true` with explicit `claude` or `codex` targets.
  The user reviews the exact replacement before publication. Other hooks,
  including entries in the same group, and other servers survive.
  An absolute `project` may select another enrolled root for explicit agent
  repair only. The review and frozen plan bind that target; the connection's
  policy tools retain their original project. Preview with `dry_run: true`
  before requesting confirmation. Box-managed hooks use Box's `config_sync`
  with the same project, agents and `repair_hooks: true` instead of direct setup.
  If cross-project publication is interrupted, the result uses
  `repair_route: operator_cli`. `operator_recovery` names the target project and
  gives `review` and `cancel` commands as executable/argument records. Ask the
  operator to run the chosen command; unqualified pending recovery on this
  connection would select its original project. A separate prefix-rule failure
  provides a target-project `setup` command instead. Each command requires the
  ordinary operator terminal and confirmation.
  For the connection's own project, interrupted confirmed setup resumes automatically on the next mutating MCP request,
  including `mcp_setup {}`, preserving unrelated settings added since confirmation.
  Status, doctor and documentation calls preserve pending setup and host files.
  If an owned fragment conflicts, use `mcp_setup {"pending":"review"}` for a new human review of
  its replacement, or `mcp_setup {"pending":"cancel"}` to review cancellation.
  Cancellation preserves manual edits and policy, approvals and history.
  Declining or dismissing the confirmation leaves pending work available for
  another decision. Unchanged committed private connections remain usable while other
  setup work is pending.
  Selecting agents updates only those integrations; omitted agents keep their
  settings and registrations, even when their files need attention. Status
  reports each agent separately. Changes to a shared launcher location or the
  saved tool permissions include the affected integrations in a broader review
  with an explanation before anything is published.
- `cache_prepare {}` rebuilds disposable compiled data from committed policy.
  It changes no authority and needs no confirmation.
  If the saved cache location is unusable, supply an absolute `cache_dir` for
  a new private location. This location change requires in-session confirmation;
  it preserves the active policy and does not delete the old location.

`request_list` and `request_show` include native hook requests as well as read
and execution requests. `request_deny` asks the user to deny a pending request
or revoke its unused grant; consumption and revocation are mutually exclusive.

Only the agent that made a native hook request can approve it, and only when
this project has that agent's integration. A Git worktree, clone or copy
whose Shield agent entries match a registration you set up for the same policy
is registered for you automatically. Otherwise, or when an entry was edited,
refusals there do not offer `request_approve`, the tool returns
`agent_not_connected`, and listed requests show `approvable: false` with an
`approval_route`. Run `shield mcp setup` in that folder, restart the agent
session, and repeat the refused call.

Pack installation accepts known typed settings through a `set` object, also
inside each batch selection. Forms collect remaining required or requested
values. `pack_update` collects settings introduced by a release; an unchanged
update returns `unchanged` without asking for approval. Bare `pack_configure`
guides settings and rule overrides; explicit edits support values or larger
configurations that cannot fit the bounded form.

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

- `docs_list`: list page paths, titles and line counts. Optional `offset`
  starts at zero; `limit` is 1–100, default 50. Continue with `next_offset`.
- `docs_search`: find case-insensitive literal `query` text in the guides.
  The query matches within one line: `.*` and `[` match themselves, surrounding
  spaces are significant, and a query must be 1–1024 bytes, not only whitespace
  and without line breaks. Optional `path` selects one listed page. Results
  contain `path`, one-based `line` and the matching line's `text` without its
  line break, in path and line order. Use `offset` and `limit` as for listing;
  continue with `next_offset` and the same query and path.
- `docs_read`: read a listed `path`, optionally starting at `start_line`
  (one-based, default 1). `limit` is 1–400 lines, default 100. The result includes
  `content`, `total_lines` and `next_start_line` for continuation. `content`
  holds the lines exactly as stored, line breaks included, so joining the
  `content` of successive reads reproduces the page.

`limit` is a maximum: a result also ends early before its text passes 512 KiB,
so continue from `next_offset` or `next_start_line` until it is null.
Every result includes `version`, the Shield version whose guides it returns.
Unknown arguments are refused.

An unknown page returns `not_found` and directs the agent to `docs_list`;
these are exact catalog paths, not filesystem paths, so `../README.md` or a
differently cased name is not found. Searches with no matches return an empty
list, and a read past the last line returns empty content. Pagination is stable
for the lifetime of the installed binary.

Before drafting a reusable command rule, search for the relevant matching
grammar and read its explanation and examples. Inspect the current rules with
`rule_list` so reviewed variants can extend an existing custom rule.

### Example: find the argument-matching guide

Call `docs_search` with these arguments:

```json
{"query":"operands"}
```

The result includes matching page paths, line numbers, and text. Choose a
relevant match in `policy-rules.md`. To read that guide from its beginning,
call `docs_read` with:

```json
{"path":"policy-rules.md","start_line":1,"limit":100}
```

The result's `content` contains Markdown. If `next_start_line` is present and
non-null, use that number as the next `start_line` to continue reading. To jump
to a search result instead, use its returned `line` as `start_line`; do not
assume that line numbers are the same in another installed build.

For a new reader, read `introduction.md` or `README.md` first. These tools
expose the same public guides used by people configuring Shield.

## Optional policy-review skill

Shield bundles `policy-review` at `skill://shield/policy-review/SKILL.md` for
user-requested reviews of recurring permission interruptions. A host that supports
MCP skills can discover and activate it. Tools and documentation remain usable
without activation. The skill is guidance; reading it never approves an action
or changes policy. Shield does not install it into local agent skill directories.

Modern clients use MCP `2026-07-28` per-request metadata. For example, send this
newline-delimited request to the registered server:

```json
{"jsonrpc":"2.0","id":1,"method":"skills/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
```

Use the same `_meta` with `skills/get` and
`{"uri":"skill://shield/policy-review/SKILL.md"}`, or with `resources/read`
and that URI. Discovery and reads return private cache scope and zero TTL.
The manifest includes exact byte lengths and SHA-256 digests. Host support for
discovery does not establish support for skill activation or user approval.

To expose only the static bundle without an enrolled project, launch
`shield mcp serve --skills-only`. This mode opens no project and advertises no
policy tools. The installed executable contains all skill bytes and works offline.

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
even if the registry changes. A `rule_id` accepts `view: "rules"` or no view,
without filters or paging. Summaries do not accept filters or paging.
Use `view: "settings"` for complete variable definitions, preconditions and
dependencies, including sections omitted from a large summary. This view also
does not accept filters or paging.

Summary and settings results include `setup`: whether configuration is available,
installation arguments, and a `settings_arguments_template` for requesting
settings with `pack_add.variables`. Replace the placeholder with only declared
names needed for the user's request. Missing required values are collected
automatically; unrequested optional values stay unchanged. Inspect each variable's
type and constraints; known values unsupported by forms can be supplied through
the selection's typed `set` object.
If a release has no settings, installation cannot add a requested
target restriction. Do not offer an unrestricted pack as a scoped installation.
The `local` layer chooses storage on this machine; it does not restrict remote
repositories or accounts.

Tell the agent the restriction you want, for example “install AWS requiring the
acme-read-only profile.” It inspects the pack's declared settings, selects the
matching optional names and opens the
settings form before final approval. A plain install without a requested
restriction keeps the pack's defaults for unrequested optional settings. If the selected
release cannot represent the restriction, the agent should explain that gap.

For a pack declaring an optional `manager` setting, request it with
`{"reference":"acme/settings","variables":["manager"]}`. The user supplies
the value in the form; one final approval activates the installation and setting
together. Use only names declared by the selected release; do not guess fields
or search general rule-authoring docs for undeclared pack settings.

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

After searching and inspecting a pack, call `pack_add` with the
following shape. Replace `PACK_REFERENCE` with the exact reference you reviewed:

```json
{
  "reference": "PACK_REFERENCE",
  "profile": "recommended"
}
```

Answer any required settings form. The response includes the proposal ID and
`apply_arguments`; policy is unchanged.
Explain the scope, then copy `apply_arguments` unchanged into `policy_apply`.
Wait for the host approval and result. A successful application returns a receipt;
declined or unavailable approval leaves the proposal inactive. A stale proposal
needs fresh preparation. See the [review reference](#proposals-and-operator-review).

## Install a pack with guided settings

Ask the agent to inspect the registry pack with `pack_info`, then call `pack_add`:

```json
{"reference":"acme/settings","profile":"recommended","layer":"project"}
```

For a new pack, omitted `profile` selects `recommended`; omitted `layer` selects
`project`. Use `local` for a personal installation. Existing selections keep their
profile unless one is supplied explicitly. Local batches refuse explicit profiles
for packs installed before the batch; requested variable values use local overlays.

Shield resolves and verifies the packs and their dependencies, then asks for required
settings that have neither a value nor a declared default. Fields identify their
pack and variable; field keys such as `setting_0` are private to that form. Answer
every field explicitly. To ask about optional settings during installation, pass
`variables` with the exact names from `pack_info`'s `settings` view. Requested
variables are asked even when they already have a value or declared default;
other optional settings remain unchanged. For example:

```json
{"reference":"acme/settings","variables":["manager"]}
```

Use `packs` to install up to 16 selections together. Each selection accepts
`reference`, optional `profile` and optional `variables`. The top-level `layer`
applies to every selection and its newly added dependencies:

```json
{
  "packs": [
    {"reference":"acme/git","profile":"recommended"},
    {"reference":"acme/settings","variables":["manager"]}
  ],
  "layer":"local"
}
```

Supply either a single `reference` with its `profile` and `variables`, or `packs`;
do not mix those selection fields. Select each installed pack only once, including
when it is also another selection's dependency. Required dependency settings
and requested variables appear in one form, with shared dependencies asked once.
The response contains one proposal for one final approval and atomic activation.
If no required or requested input needs answering, preparation opens no settings
form. Unknown variable names or a failed selection leave no partial proposal.
Installed settings can also be changed later with `pack_configure`.

The form retains the exact acquired releases. A registry change while it is open
does not substitute another release. A changed project policy or review context
requires a new `pack_add` call and fresh answers. Submission saves an inactive
proposal; review its pack versions, profiles, values and scope, then copy its
`apply_arguments` unchanged to `policy_apply` for separate final approval.
Decline, dismissal and unavailable forms create no proposal. If any `pack_add`
reply is lost, inspect `policy_show` before retrying, including when no settings
form opened. A fresh call can create another proposal; do not reconstruct private
form answers.

Guided installation supports the same field types and protocol compatibility as
the settings forms below, with at most 16 required/requested fields in total and 4 MiB of
retained candidate source per interaction. Supply known values through `set`
to avoid unsupported form fields or free-text lists. Split larger guided
interactions into separately reviewed changes within these bounds.

Hosts supporting MCP prompts can also discover `install_pack` through
`prompts/list` and retrieve it with `prompts/get`. Its string arguments are
`reference` (required), `profile` and `layer`. It supplies a conversation starter
for this workflow. Its tools require an enrolled-project MCP connection; a
discovery-only connection can retrieve the prompt but cannot install a pack.
Retrieving it performs no acquisition, proposal creation or approval; the
`pack_add` tool remains available without prompts.

## Update or remove installed packs

`status`, `pack_list` and `pack_show` return cached `data.pack_updates` with
`availability` (`available`, `none`, `partial` or `unknown`), up to eight release
rows, source freshness, excluded pin/dependency counts, total/fresh counts and
truncation. Rows name the installed `selector`, source identity, installed and
advertised versions, observation/expiry times and `compatibility: unchecked`.
The reference form of `pack_list` restricts this inspection to that selection.

A successful `status` or `pack_list` can additionally carry
`data.pack_update_notice`, with up to three rows, a remaining count, message and
agent instruction. Mention it briefly after the current task; prepare an update
only when the user requested updates. The notice creates no proposal or approval
prompt. Delivery is throttled per project and conversation, with connection scope
when a conversation identity is unavailable; reconnecting does not reset the
daily/version limits. Explicit inspection remains available after suppression.

Use `pack_search` with `{"query":"main/"}` to refresh main metadata explicitly,
or substitute the relevant enrolled alias. Inspection makes no update-related
network requests. Old or missing evidence is unknown, rather than a guarantee
that installed packs are current. The operator can launch Shield with
`SHIELD_PACK_UPDATE_CHECK=off` to disable unsolicited notices. See
[cached availability](local-registry.md#check-for-pack-updates) for freshness and
terminal behavior.

The CLI and MCP use the same pack verbs. Call `pack_update` with `{}` to
update all tracking packs, or with `{"reference":"gate"}` to update one
selection and its dependencies. Pinned versions stay fixed and are listed in
`skipped_pinned`. Call `pack_remove` with `{"reference":"gate"}` to remove
one selection using the same dependency checks as `shield pack remove`.

To let the person choose installed packs, call `pack_remove` with `{}`. The form
shows exact installed selectors and accepts 1–16 selections from a list of at
most 64 packs. Submit them together to prepare one removal proposal. Shield
checks the selected set as a whole, so selecting a pack and its dependents does
not depend on display order. A remaining dependent still blocks removal. Shield
refuses a question that cannot fit its bounds rather than omitting choices.

Multiple selection requires MCP `2025-11-25` or modern `2026-07-28`. Older
clients can use explicit single-pack references; Shield never narrows the list
silently. Empty, duplicate or unknown choices refuse. If policy or review
metadata changes while the form is open, request a fresh form and new answers.

These tools prepare an inactive candidate. Inspect the returned changes, then
copy `apply_arguments` unchanged to `policy_apply` for user approval. Preparing
an update or removal does not change active policy. Use `pack_show` with a
`reference` to inspect one installed selection, its source and retained rules.

## Choose installed pack settings

When settings need your input, an agent can call the `pack_configure` tool to
show a form and prepare an inactive proposal from your answers. First inspect
the installed pack with `pack_show` and its `reference` selector to find its
variable definitions. For an installed Acme pack declaring these variables,
the tool arguments look like:

```json
{"reference":"acme/settings","variables":["manager","regions"],"layer":"project"}
```

`variables` selects 1–16 distinct declared names. `layer` is `project` by
default; `local` writes personal configuration when the proposal is approved.
An unknown variable returns `invalid_input`; inspect the installed selection
with `pack_list` to find its declared names before retrying.
Forms support text/path values, booleans, single choices and lists of choices.
Free-text lists use the direct settings arguments described below instead.
Every selected field needs an explicit answer; omitted fields do not inherit
defaults. Pack constraints and credential checks still apply to submitted values.
Do not enter credentials.

Multiple selections require MCP `2025-11-25` or modern `2026-07-28`.
A connection negotiated as `2025-06-18` can collect compatible primitive fields,
but a form containing a list of choices returns `input_unavailable` without
preparing a proposal. In that case, collect the values in conversation and use
the ordinary proposal route. Do not turn the list into one selected value.

Submitting the form only saves a proposal. Review its scope, then let the agent
copy the returned `apply_arguments` unchanged to `policy_apply` for separate
final approval. Claude uses a separate confirmation form; Codex keeps its host
tool approval. Declining or dismissing the settings form creates no proposal
and ends the offer. A changed selection needs a new form and proposal.

If the host cannot display the requested form, ask for the settings in the
conversation and call `pack_configure` with direct `set` arguments.
Final approval still requires the host's confirmation. Never infer missing answers.
A policy change while the form is open makes its answer stale; prepare a fresh
settings review. Modern clients must support the form mode on each request and
return the exact tool arguments, issued `requestState` and the `settings` input
answer on retry. Pending modern forms expire after five minutes, allow at most
eight rounds and are private to the connection. Replaying a consumed form refuses.

## Proposals and operator review

Known settings can be passed directly to `pack_configure` without a form, using
the CLI's operations as JSON fields: `set`, `unset`, `override` and
`clear_override`, with optional `layer`. For example:

```json
{"reference":"acme/settings","set":{"manager":"pnpm"},"override":{"build":"ask"}}
```

To remove those choices, use `{"reference":"acme/settings","unset":["manager"],"clear_override":["build"]}`.
Do not mix direct settings with `variables`, which selects a user-input form.
Both modes return an inactive proposal and require `policy_apply` approval.

`policy_propose` accepts one tagged `change` or a `changes` array of
1–16 changes. A batch applies in order to one candidate; approval activates all
changes together or none. Optional fields include a reason, history and
supersession links, and an ASCII `idempotency_key` of 1–128 characters.

| Change | Purpose |
| --- | --- |
| `project_files_adopt` | Capture and review policy edits on disk; must be proposed alone. |
| `rule_add`, `rule_edit`, `rule_remove` | Add, replace, or remove a custom rule. |
| `pack_add`, `pack_update`, `pack_remove` | Install, update, or remove a pack and its dependencies. |
| `pack_configure` | Set variables or rule overrides on an installed pack. |
| `access_path_add`, `access_path_remove` | Add or remove one absolute read/write directory. |

Every change except `project_files_adopt`, `pack_update` and `pack_remove` accepts `layer: project`
(default) or `local` for uncommitted local configuration. Rules use the project
condition schema.

If a pack or rule change would overwrite unreviewed `.shield/` edits, first call
`policy_propose` with `{"change":{"kind":"project_files_adopt"}}` and submit
its returned `apply_arguments` unchanged to `policy_apply`. Approval presents
the captured policy edits relative to active policy. The semantic comparison
covers supported command decisions; file, web and pack-precondition decisions
are not compared. The prompt reports incomplete comparison coverage, and its
summary can abbreviate large changes; inspect the retained proposal with
`policy_show` for the full structural diff. The files remain inactive until
approval; edits made while the prompt is open make the proposal stale. After
approval, prepare the pack or rule change again. This uses the host's ordinary
Shield approval flow and needs no terminal review. Existing verified pack bytes
are required; new releases still use pack acquisition. Changes to MCP connection
settings use a host confirmation form on both Claude and Codex. The form shows
captured before/after settings, keeping private local values out of public
proposal arguments, comparisons and receipts. Approval also publishes the
owned connection entries for registered agents. Interrupted publication resumes
automatically on a subsequent mutating MCP request or cached launcher invocation; unchanged
connections remain usable. Conflicting owned settings can be reviewed through
`mcp_setup {"pending":"review"}` or cancelled in the same session. New agent
registration uses `mcp_setup`. Host connection reload behavior determines
when an already running connector is replaced.
Missing or linked policy files keep the `shield policy review` repair route;
adoption requires regular files.

The `policy_propose` change with `kind: "pack_configure"` requires an installed `reference` and at least one of `set`,
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
or reference cannot authorize another change. A failed attempt needs fresh
preparation. `policy_show` recovers arguments only for your current live review.

Claude receives the proposal ID, digest, change list, and review summary.
Codex receives readable `change`, optional `details`, and a short `reference`.
Detailed evidence remains available through proposal inspection.

The host confirmation describes pack/profile, rule, setting, or folder changes.
It shows permissions and comparison limits, including before/after matching
scope and removed restrictions. Supplied rule names are labeled as such;
shortened or omitted constraints are identified.

| Host | Approval |
| --- | --- |
| Claude | An MCP confirmation form. Accept applies, decline rejects, and cancel withdraws the attempt. |
| Gemini CLI / Antigravity CLI | MCP form confirmation when the client can present it. Unavailable forms close the attempt and report the missing in-session capability. |
| Codex | The per-call tool prompt for ordinary policy; an additional Shield form for connection settings. If prompts are disabled, enable them with `/permissions`, then prepare fresh in the session. |
| Other or unrecognized host | In-session approval is unsupported. Report that capability gap and withdraw the attempt. |

Claude's form works in `default`, `auto`, and `bypassPermissions` modes and
requires its own Accept response, independently of ordinary tool approval.
The client must advertise form elicitation or apply returns
`approval_unavailable`. Only one form can be open per connection, with a
five-minute response deadline on legacy connections. It waits outside the worker
so other calls can finish. Codex controls its prompt's heading, buttons, and field rendering.

For modern requests, Claude's apply returns `resultType: "input_required"`
with a confirmation form and opaque `requestState`. The host obtains the user's
answer and retries the same tool and arguments with a new RPC ID, echoed state,
`inputResponses: {"confirmation":{"action":"accept"}}` (or `decline`/`cancel`),
and current per-request form capabilities. Agents must never manufacture answers.
State expires after five minutes, belongs to one connection, and is consumed by
one retry. Changed arguments, expired state or missing capabilities cannot apply
policy. At most eight modern confirmations can wait per connection. Disconnect
revokes them. Cancelling a completed interim RPC ID does not revoke a later retry;
the host must discard the state or submit a decline/cancel response.

Shield rechecks arguments and current policy before committing. Stale or changed
candidates need a fresh proposal and approval. Success returns a receipt;
repeating an applied proposal returns that receipt. Agents keep approval inside
the conversation; sending the user to a terminal is a product bug.

Inspection and proposal preparation run without prompts by default; setup
preserves any tool-permission choices you changed. Native hook messages keep
proposal payloads separate from their user-facing explanation.
Native hooks leave Shield's own MCP tools reachable even when the hook's
project state is missing or unreadable. The MCP server still checks its own
project authority and requires approval for changes; ordinary tools remain blocked
when the hook cannot verify their authorization.

### Replay and withdrawal

Without an idempotency key, identical arguments return the same proposal;
changed arguments create another and close the prior live attempt. With a key,
the same input recovers its saved identity or closed outcome; changed input is
refused. Reconnect cannot take over a candidate. Closed attempts return no apply
arguments. To try again while still authorized, prepare with a new
`idempotency_key` or `supersedes_proposal_id` and obtain a fresh decision.

If `policy_propose` reports a deadline error, its inactive proposal may
already be saved. Retry the identical arguments on the same connection to
recover its current outcome. Only a live review owned by that interaction
returns approval arguments. Use `policy_show` to inspect it or `policy_receipt`
to retrieve a known review's outcome. A timeout does not establish that the
proposal was absent or that policy changed. Report unresolved failures; if the
attempt is closed, inspect its outcome before preparing fresh.

Only the original registered proposer can withdraw a proposal. Repeating an
owned withdrawal makes no further change. Unrecognized hosts can inspect and
propose, but advertising form support does not grant activation authority or
install native hooks. A prepared review expires after five minutes even before
apply. Disconnect or process death closes its candidates; Stop hooks close
unapplied reviews associated with that task's session; SessionEnd also closes
them, as does Codex's Interrupt event. Updated hook registration
requires `mcp_setup` through its in-session review. When the host supplies no
session association, the connection lease and expiry still bound its lifetime.

`policy_receipt` returns rejected, withdrawn, stale and failed outcomes as well
as committed receipts. It never reopens them. Recovery evidence is retained
through 24 hours after the review deadline, and the current activation's receipt
is kept. Capacity is 128 live reviews and 1,024 retained outcomes. Older eligible
outcomes are reclaimed when admitting new work; capacity never asks a person to
clear an approval queue.

Only explicit user deferral creates a draft: call `policy_defer` with `intent`,
optional `constraints`, an `idempotency_key` and optionally the owned
`proposal_id` to withdraw. Use `policy_draft_list` to retrieve intent and
`policy_draft_remove` to discard a draft created by the current session. At most
128 drafts are retained, and drafts expire after 30 days. Drafts hold
no candidate, base revision or approval. Resuming requires current inspection,
fresh preparation and immediate approval.

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
`docs_search` and `docs_read` for additional grammar details.
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

`policy_receipt` reads closed outcomes and committed receipts; live proposals return
`operator_review_required`. It cannot apply a change. Stored action requests
are separate: inspect `request_list`, then use `request_approve` or `request_deny`
for in-session confirmation.

`request_approve` supports native-hook requests, SDK reads and checked commands.
It shows the stored action to the person; Accept creates a one-time permission
without executing it or changing policy. Follow the returned `next_action`.
Checked commands retry with `--approval-request REQUEST` and unchanged command,
agent, session and tool attribution. SDK reads retry through the same factory,
session and read window. SDK applications use the explicit
[interactive read session](claude-read-only.md#approve-reads-in-the-sdk-session).
Read and checked-command grants satisfy Ask; native-hook forms can also review
supported Deny overrides. Resource changes, expiry and policy activation require
a fresh request.

Pass `request_ids` to review up to eight distinct pending requests together,
including different kinds from the same identified session. Accept issues all
grants atomically; an invalid member issues none. A request with no recorded
session can be reviewed individually with `request_id` on its matching channel.

## Diagnostic input and snapshots

`status` and `policy_show` expose `active_candidate_id`. Submission or
inspection of a proposal exposes `before_candidate_id` and `after_candidate_id`.
Handles identify immutable bytes; policy activation cannot retarget them.
Pending reviews preserve their exact captures. Under storage pressure, old
inspection handles may expire; `candidate_unavailable` means to request fresh
handles through `status` or `policy_show` on the same connection. Unknown and
foreign handles also refuse. Each connection retains at most 256 snapshots;
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

### Durable preparation tasks

Clients using per-request MCP `2026-07-28` can declare
`capabilities.extensions["io.modelcontextprotocol/tasks"] = {}` in their modern
request metadata. A concrete `policy_propose` call may then return a task handle
instead of waiting for preparation. Legacy calls and calls without that extension
keep their ordinary synchronous result. Guided settings forms and manual file
adoption keep their existing workflows.

Keep the returned `taskId`. Send `tasks/get` with `{"taskId":"RETURNED_ID"}`
to read status and, after completion, the original tool result. Reconnect to the
same enrolled project to continue polling. If the creation reply was lost, repeat
the identical `policy_propose` arguments, including the same idempotency key when
supplied, to recover the original handle. Automatically derived keys include the
current policy base and engine, so the same concrete changes prepare afresh after
the base changes. An explicit key continues identifying its original task; use a
new explicit key when fresh preparation is needed.

Completion captures policy and comparison evidence without creating an applicable
proposal. Copy the completed result's `data.review_arguments` into
`policy_propose`, for example:

```json
{"preparation_task_id":"RETURNED_ID"}
```

This creates a fresh review for the current connection after checking the retained
capture and current policy base. Inspect that review, then pass its unchanged
`apply_arguments` to `policy_apply` for the person's separate confirmation. A
completed task, polling response or task input message cannot approve a change.

Use `tasks/cancel` with the same `taskId` to request cancellation. Completion may
win the race; inspect the subsequent status. Shield's preparation tasks collect
no user input through `tasks/update`; well-formed unissued answers are ignored.
Do not restart cancelled or failed work automatically. If it is still needed,
prepare a new request with a new `idempotency_key` after inspecting the failure.

Tasks retain results for 24 hours from creation. Each project admits at most
32 retained tasks and two running preparations; a full reservation selects the
synchronous path before work starts. Captures are limited to 16 MiB and results
to 1 MiB, within a 640 MiB project task-store bound. Oversized captures fail
without creating a proposal. Execution has its own cooperative 60-second budget.
Closing a connection does not cancel published work, but exiting its process can
stop the worker. Reopening marks an interrupted task failed without replaying it.
Polling does not extend retention or restart work. It settles an unowned working
record as failed even if the polling server was already running when its owner died.

Any connection authorized to that local project enrollment can inspect or cancel
a known handle; client names do not establish identity. Unknown, expired and
foreign handles return not-found. Unusable task storage falls back to synchronous
preparation, including for Tasks-capable calls. Host presentation of tasks depends on the client.

### Operation progress

MCP clients can include `_meta.progressToken` in `tools/call` parameters to
receive `notifications/progress` while Shield acquires registry content or
compares policy. Tokens may be strings or integers. For example:

```json
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"pack_info","arguments":{"reference":"main/canonic/node"},"_meta":{"progressToken":"pack-review-7"}}}
```

Messages identify stages such as reading and verifying the registry catalog,
release history or pack artifact, preparing policy inputs, comparing permissions
and preparing an inactive review. Registry enrollment can report identity/key
discovery before its separate trust confirmation. Stage messages contain no
registry URLs, command text, settings or credentials.

The increasing `progress` value counts stages within that RPC; it is not a
percentage, and Shield omits `total`. At most 64 stages are offered per RPC.
Legacy settings forms retain the count after input; modern input retries are
new RPCs with their own tokens and counts. Calls without a token send no progress.

Progress is best effort: queue pressure or request completion can drop stages.
Notifications never follow the request's final reply. Use the final tool result
and receipt to determine the outcome. Progress cannot approve a proposal, extend
a deadline or revive a cancelled operation. Whether a host displays notifications
depends on that host.

### Client startup

Shield exposes project inspection, approved policy changes, and diagnostic
evaluation over stdio using MCP `2025-06-18`, `2025-11-25`, and per-request
`2026-07-28`. Other clients can
launch `shield mcp serve --project /workspace/acme` explicitly, replacing
the example path with an initialized project. Startup failure exits 1 with a
fixed stderr diagnostic and empty stdout.
Launch from the selected project's directory. Shield serves the nearest
configured project holding the agent's session folder, or else its working
directory; `--project` applies only when no configured project holds that
folder. A moved, copied or worktree folder therefore serves itself.

A launch may supply an agent and an explicitly mapped host session through
internal registration arguments; tool input cannot appoint either. Client names
are declarations. Shield generates a new connection ID on each launch.

The client initializes with a supported protocol version, sends
`notifications/initialized`, then uses `tools/list` or `tools/call`. The server
supports ping and cancellation and advertises tools and bundled resources. Legacy
clients read the skill through `resources/list` and `resources/read`; `skills/*`
requires modern metadata. Every tool publishes
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

### Read a proposal's full reports

Proposal results and `policy_show` include `report_resources` with two addresses:
the captured policy comparison and the retained review evidence. Exact proposal
results also include MCP `resource_link` blocks. Text-only clients can read the
same addresses from text block 1. Use MCP `resources/read` with the returned URI:

```json
{"uri":"COPY_THE_RETURNED_REPORT_URI"}
```

The JSON comparison report preserves coverage, unknowns and before/after
evidence. The evidence report includes attribution, reasons, linked requests,
current disposition and any retained receipt or authorization events. Private
connection settings and secrets are redacted. A report is inspection only;
activate a change through its separate, unchanged `apply_arguments` and host
confirmation.

Report addresses belong to one enrolled project and exact proposal candidate.
They can be read after reconnect while the proposal remains retained. Evidence
reflects its latest retained disposition; a URI never switches to a different
candidate or project. A foreign, unknown or reclaimed report returns a resource
not-found error. Discover the address shape with `resources/templates/list`;
`resources/list` continues to list bundled static resources. Reports exceeding
the response limit return an error rather than partial evidence. Project reports
are unavailable before enrollment consent or on the project-free static server.

### Tool result format

Results retain the complete envelope in `structuredContent`. Text block 0 gives
short guidance; text block 1 is bounded JSON with `data`, `error`, `missing` and
`truncated`. Inspection tools include requested data there. Proposal/withdrawal
text includes IDs, status, candidate handles and report addresses; use the report
links or `policy_show` to inspect the proposal. Text-only clients parse block 1
rather than block 0. The shared
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
most 256 continuations and page receipts. Retrying a retained cursor returns the
same page and successor. Completed traversals release their full lists. Old page
receipts and abandoned continuations are reclaimed automatically; if a cursor
expires, restart that list without `cursor` on the same connection. A view that
cannot fit alongside its replay receipt returns `overloaded`. Pending-review captures
remain protected. A session accepts at most 1,024 inbound messages; the next one
ends it with `-32000` "Session message limit reached".

One worker processes tools while the dispatcher handles input and ping. At most
eight tool calls may be outstanding, including one waiting for confirmation;
more are refused with `-32000`. Duplicate outstanding IDs close the session.
Bundled reads use a separate worker with eight outstanding reads, allowing them
to finish while the policy evaluator is busy.
Task management uses two separate workers with eight outstanding requests, so
status and cancellation remain reachable while preparation runs.
`--request-timeout-ms` bounds queued work, evaluation and encoding
(default 5,000 ms, range 1–60,000); a call that exceeds it receives `-32001`
"Request deadline exceeded". Cancellation suppresses pending responses.
Policy apply uses that budget for preparation, a separate five-minute form wait,
then a fresh ordinary budget for queued revalidation, commit and encoding.
Checkpoints do not interrupt native blocking I/O; clients also own process/I/O
limits. Lost `policy_propose` replies can be recovered by resubmitting the same
arguments. For any lost `pack_add` or `pack_configure` reply, inspect `policy_show`
before retrying, even if no form opened. Consumed form state cannot be replayed
and private answers must not be reconstructed.

Keep stdin open until you have the replies you need. When it closes, queued
`policy_propose`, `pack_add`, `pack_configure`, `pack_update`, `pack_remove`, `policy_withdraw` and `policy_apply`
calls are refused with `-32800` and never run, a pending confirmation is
withdrawn, and read-only tools still finish.

Closing stdin drains admitted work and joins the worker before exit 0. Framing or
transport failure cancels and joins work and exits 1. Resident clients complete
each call from its correlated envelope; process exit supplies no missing result or
permission. Native blocking stdin readers end at process exit after early failure.

Native macOS tests and pinned official SDK fixtures exercise this interface; live-agent
setup and Linux/Windows runtime coverage remain separate validation work.
