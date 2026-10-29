# Configure project MCP servers

Shield can configure project MCP connections and evaluate their tool calls in
Claude Code and Codex. Adding a permission pack never enables or launches a
server. Connections and permission rules are reviewed separately.

## Declare and activate a connection

Add `mcp` to `.shield/config.yml`. Each key is a stable project connection ID;
`agents` maps agent names to the server aliases those hosts use:

```yaml
schema_version: 1
packs: []
rules: rules.yml
mcp:
  cloud:
    transport:
      type: http
      url: https://cloud.example.com/mcp
    agents:
      claude: acme_cloud
      codex: cloud
```

Review the source edits with `shield policy review` in your terminal. Then run
`shield mcp setup --dry-run --agent claude --agent codex` to inspect the host
projection, followed by `shield mcp setup --agent claude --agent codex` to review
and apply it. Restart the agents and review their project trust and MCP prompts.
Setup does not install connectors or configure credentials.

Supported host aliases start with a lowercase letter and contain lowercase
letters, digits and underscores, up to 64 characters; repeated `__` is reserved
by the host tool encoding; aliases cannot end in an underscore. Other aliases
refuse instead of guessing host name
normalization. Stable project and pack integration IDs are independent of these
host alias restrictions.

Stdio transports use `type: stdio`, a `command`, an `args` array and an optional
string-valued `env` map. Argument boundaries are preserved. HTTP transports take
a URL without embedded credentials. OAuth headers, secrets, user-wide server
discovery and automatic credential setup are outside this projection.

The connection is independent of packs. A matching manual project entry can be
bound without transferring ownership. A conflicting alias requires resolution;
Shield never replaces it silently. Setup tracks only entries it creates, detects
edits to owned entries, and removes only those entries. Set `enabled: false`,
review the change, then rerun setup to withdraw an owned projection.

## Bind a pack integration

An associated pack contributes MCP rules only when the project connection is
enabled, has an agent binding and matches the pack's transport predicates:

```yaml
mcp:
  cloud:
    transport:
      type: defaults
      pack: acme/cloud
      integration: hosted
    agents: {claude: acme_cloud, codex: cloud}
    packs: {acme/cloud: hosted}
```

An MCP precondition alternative with no enabled association does not invalidate
the pack's other integrations or CLI rules. Alternatives naming only unbound
integrations are dropped. An active gate with no remaining alternatives still
applies its Ask/Deny decision; removing an alternative cannot remove a restriction.

Use the exact selected pack identity. Registry selections prefix the pack ID
with the registry's stable identity, such as `acme-registry/acme/cloud`;
use that same identity in both `transport.pack` and `packs`.
`type: defaults` resolves the integration's defaults from the selected pack.
Alternatively supply an explicit stdio or HTTP transport while retaining the
association. Removing a pack withdraws its association and contributed rules;
the independently configured server remains. Referenced defaults are retained as
explicit connection settings when their pack is removed.

Project rules address the stable ID (`cloud`), while pack rules address their
declared integration (`hosted`). Host aliases route requests to those project
connections. Unbound MCP servers keep their ordinary host behavior. Bound calls
are checked against the current project host entry; drift blocks them pending
terminal review.
Classification and enforcement share one loaded active snapshot per call.
If that snapshot is unavailable, Shield cannot establish that a server is
unbound, so all third-party MCP authorization calls fail closed, including calls
to otherwise external servers. The failure names unavailable MCP bindings and
directs the operator to `shield doctor`. Shield does not infer an unbound server
from a failed policy load.

Shield also checks known competing file scopes read-only. Claude local/user
entries in `.claude.json` and nested `.mcp.json` files cannot reuse a bound alias.
The check honors `CLAUDE_CONFIG_DIR`. Codex user `config.toml` and closer project
`.codex/config.toml` layers cannot reuse it either; `CODEX_HOME` is honored.
Identical entries and normalized-name collisions also require resolution.
Malformed, unreadable or linked scope files refuse verification. These checks
never change the other scopes or bind their servers to the project.

These file checks do not prove which server the running host selected. Session
flags, SDK-supplied servers, dynamically replaced connections and managed
configuration are not fully observable through the native hook. MCP allowances
and launch-setting restrictions are conditional on the host actually using the
reviewed project connection. Shield does not guarantee enforcement against a
server substituted through an unobserved scope. Effective connection identity,
host loading and running-connector identity remain unverified.

## Use local overrides privately

The `mcp` map in ignored `.shield/config.local.yml` replaces whole connections
by ID. Supply the complete effective connection, including its transport and
bindings; empty `args`, `env`, `agents` or `packs` explicitly clear those parts.
You can disable a shared connection or add a local-only one this way.
Local connection changes in proposal views, approval arguments and publication
receipts contain the connection ID, layer and change kind, without before/after
settings. The terminal review shows the full local change from the private
candidates; those details are not copied into the public change record.

Local stdio settings use private indirection. Host files contain a Shield
launcher, project path and opaque random token; the connection ID, transport
digest, command, arguments and environment stay in private review state. Tokens
are reused for unchanged settings and replaced when launch settings change.
The launcher replaces itself with the connector, preserving native stdio and
signal ownership. Changed settings need
fresh setup and an agent restart. Review-state permissions must be private.
Setup verifies that each destination (`.mcp.json`, Claude's local enablement
settings or `.codex/config.toml`) is untracked in its Git repository, before
preparation and again before publication.
Tracked files refuse even when an ignore pattern matches them. An unavailable or
unverifiable repository also refuses. Shield does not change tracking or ignore
rules; keep these local host files out of commits.
Local HTTP overrides refuse before any host write because private HTTP
projection is unsupported. Shield never drops a local override as a fallback.

Connection, association, binding and enablement changes require terminal review.
Permission-only MCP proposals retain the existing `policy_apply` workflow.
Changing a pack variable or release that changes resolved connection settings
also requires terminal review; a mixed proposal is not partially activated.

## Write MCP permission rules

Pack rules use the integration-scoped `mcp` domain; exact-request MCP captures
are project-only, so a pack cannot escape association through a captured request.

Use ordinary profiles, requirements, preconditions and custom overrides. For
example, an explicit region requirement fails when the field is absent, null or
outside the allowed set:

```yaml
id: inspect-cloud
decision: allow
conditions:
  domain: mcp
  server: cloud
  tool: inspect
requirements:
  fields:
    /region: {text: {one_of: [us-east-1, eu-west-1]}}
on_unmet:
  decision: deny
```

This custom document needs `structured-rules-v1` and `requirements-v1`.
Put mandatory presence checks in requirements or a precondition with Deny on
unmet evidence. A Deny condition using `not` does not match an omitted field.

The `connection` predicate uses the same typed field-map grammar over reviewed
facts: `/transport/type`, `/transport/command`, `/transport/args`,
`/transport/env`, `/transport/url` and `/source` (`project` or `local`). Declare
`mcp-connection-facts-v1` when using it. These facts describe launch settings,
not remote credentials, OAuth identity or every region used by a script.
They are derived from the selected connection; tool input cannot supply them.
There is no generic server-default inference or script analysis.

An integration may declare `effective_fields` with `mcp-effective-fields-v1`.
Each named fact supplies an `argument` pointer and a `default` pointer into the
reviewed transport. A present argument wins, including null or a wrong type;
only an omitted argument may use the reviewed setting. Rules read the result
under `/effective/NAME`; `/evidence_sources/NAME` identifies its source and
pointer. Use this declaration only where the verified connector/tool semantics
support that precedence. Unknown defaults stay absent and cannot pass a required
gate. These mappings do not invent tool arguments or modify forwarded calls.

Ask follows the host's supported prompt or its confirmed Ask fallback. In
particular, an Allow fallback may let an Ask proceed; use Deny for mandatory
restrictions. Native third-party server defaults remain `prompt` in generated
Codex configuration. Shield's own tool approval selections remain separate.

## Declare pack defaults and command fields

A pack declares `mcp_integrations` keyed by integration ID. Each definition has
`transport: stdio|http`, typed `fields` over the transport object, and optional
`defaults`. Declare `mcp-integrations-v1`. Defaults may use whole-value typed
references such as `env: {ACME_PROFILE: {var: profile}}`; existing variable
bindings supply the value and layer. Unset optional values make those defaults
unavailable without disabling command-only use of the pack. References are not
string interpolation.
Pack admission checks the resolved sample's transport shape even when variables
are unset: environment entries must resolve to strings and arguments to a string
array. Errors name the integration and referenced variables without their values.
Endpoint syntax and integration predicates are checked against actual resolved
settings, rather than placeholder sample strings.

An optional `command_fields` map associates an exact tool name with a fixed
`executable`, JSON `pointer` and `encoding` (`argv-v1` or
`posix-literal-v1`). Packs declare `mcp-command-fields-v1`; projects can also
declare this map on a connection. Use it only after verifying that tool's
interpretation of the field. An argv field includes the executable as its first
element. Literal strings support quoting and escaping, with no expansions,
shell operators, wrappers or executable substitution.

Shield evaluates the derived command through command rules and gates, then
keeps the stricter outcome of command and outer MCP policy. Unknown remote cwd,
filesystem and environment cannot satisfy local-path requirements. Invalid or
missing mapped fields cannot establish Allow through the mapping. Python or
general script tools need their own execution policy.

Pack decision fixtures can supply an `mcp` connection map alongside `input`.
This exercises explicit association and resolved settings through the same
candidate evaluator as runtime policy; the fixture starts no connector.
