# Configure project MCP servers

Shield automatically applies installed pack permissions to matching MCP servers
configured in Claude Code and Codex. Choose a pack/profile, activate it, and its
MCP rules apply without another association or per-tool host approval setup.
Adding a permission pack never enables or launches a server.

## Use an existing server

Keep the server in your agent's existing configuration, including configuration
published by another tool. Shield reads supported project and user scopes and
matches command/arguments or the HTTP endpoint against the pack's integration
criteria. You may choose any server alias the host supports; the name does not
need to match the pack. Detection neither starts the server nor edits its settings.

Claude discovery uses the stable project root's `.mcp.json` and the exact
project entry in the user's `.claude.json`; changing the tool's working directory
does not load descendant MCP or settings files. Project-file servers need an
applicable `enabledMcpjsonServers` entry or `enableAllProjectMcpServers: true`.
User settings can supply approval directly. Repository settings contribute
approval only with recorded workspace trust in the user's project state;
repository content cannot establish its own trust. Disabled lists still win.
Unapproved servers do not supply pack matches or precondition alternatives.

Codex discovery reads project `.codex/config.toml` layers only when its user
configuration records the enrolled root as `trust_level = "trusted"`, with no
explicitly untrusted layer on the path to the current directory. Missing or
untrusted project trust leaves project files out of the observation; supported
user servers remain eligible independently. Discovery never writes trust or
approval settings. Host-only approvals or session overrides that these files
cannot establish remain outside automatic identification.

Once a pack update is reviewed and activated, new rules take effect under its
selected profile. Existing host approval entries for old tool names do not limit
Shield's current rules. A newly configured matching server or an alias rename
needs no pack reinstallation or Shield configuration edit. A tool with no matching
rule follows ordinary policy fallback; a new name does not inherit an old name's
permission by similarity.

Run `shield status` or `shield doctor` to inspect discovered aliases, matched
packs/profiles and unsupported or ambiguous configuration. Shield's native hooks
must be installed and loaded for decisions to reach the agent. Codex uses its
PermissionRequest hook to allow a call that otherwise needs a generic tool prompt;
Claude uses its native permission reply. Shell prefix rules do not decide MCP calls.

Discovery supports ordinary stdio settings and endpoint-only HTTP entries.
Unsupported headers/authentication fields or connection formats cannot establish
a pack allowance. Duplicate aliases in competing scopes refuse rather than guess
which server the host selected. SDK/session overrides whose effective connection
is not observable remain outside automatic identification; matching a file entry
does not attest the running server's identity.

Aliases beginning `plugin_` or `claude_ai_`, and `codex_apps`, belong to host
connector namespaces for this adapter boundary. They cannot establish a pack
match from configuration files. Their native calls retain host permissions;
an explicit Shield connection claiming such an alias is refused.

When an active pack declares MCP integrations, discovery must read the relevant
configuration files before it can classify an alias as external. An unreadable,
malformed or oversized shared file blocks third-party MCP authorization, including
otherwise unrelated servers: Shield cannot exclude a conflicting alias in that
file. The current per-file limit is 1 MiB. Inspect `shield doctor` and repair the
file. A malformed entry for a different, identifiable alias does not block the
requested server; an unparseable whole file cannot provide that isolation.

## Declare and activate a connection

Optionally add `mcp` to `.shield/config.yml` when Shield should publish a connection
or project rules need a stable connection ID. Each key is a project connection ID;
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

In an agent session, ask the agent to prepare
`policy_propose` with `{"change":{"kind":"project_files_adopt"}}` and pass its
returned `apply_arguments` unchanged to `policy_apply`. Shield shows the captured
connection settings in a confirmation form. After approval, the agent calls
`mcp_setup` with the selected `agents` (for example, `["claude","codex"]`) to
review and publish host settings. `dry_run: true` previews without writing.
The connection form masks sensitive values and labels them as added, changed,
removed or unchanged, so a masked edit is still visible for review.
Restart the affected connections after publication. The operator CLI offers the
same reviews through `shield policy review` and `shield mcp setup` when you
choose to work directly in a terminal.
Setup does not install connectors or configure credentials.

Supported host aliases start with a lowercase letter and contain lowercase
letters, digits and underscores, up to 64 characters; repeated `__` is reserved
by the host tool encoding; aliases cannot end in an underscore. Other aliases
refuse instead of guessing host name
normalization. Stable project and pack integration IDs are independent of these
host alias restrictions.

Stdio transports use `type: stdio`, a `command`, an `args` array and an optional
string-valued `env` map. Argument boundaries are preserved. HTTP transports take
a URL without embedded credentials. OAuth headers, secrets, user-wide configuration
writes and automatic credential setup are outside this projection. Automatic
matching can read supported user-level entries without projecting them.

The connection is independent of packs. A matching manual project entry can be
bound without transferring ownership. A conflicting alias requires resolution;
Shield never replaces it silently. Setup tracks only entries it creates, detects
edits to owned entries, and removes only those entries. Set `enabled: false`,
review the change, then rerun setup to withdraw an owned projection.

## Bind a pack integration

A managed connection may explicitly name a pack integration for defaults or an
additional constraint. This association is optional for automatic matching:

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

An MCP precondition alternative with no applicable enabled connection does not invalidate
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
declared integration (`hosted`). Host aliases route requests to configured
connections. Enabled servers matching installed pack criteria receive the current
pack policy automatically. Reliably identified servers with no applicable pack
or project policy retain host permissions. Managed calls are checked against
their reviewed host entry; drift blocks them pending a new `mcp_setup` review
in the session.
Classification and enforcement share one loaded active snapshot per call.
If that snapshot is unavailable, Shield cannot establish that a server is
external, so all third-party MCP authorization calls fail closed, including calls
to otherwise external servers. The failure names unavailable MCP bindings and
directs the operator to `shield doctor`. Shield does not infer an external server
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
settings. The human confirmation form compares the private candidates with
sensitive values masked and hidden differences labelled; those details are not
copied into the public change record.

Existing native entries whose complete transport matches the active local
connection work without setup or host-file changes. Shield checks the command,
arguments and environment, together with competing agent configuration scopes.
Setup also preserves these entries and their existing ownership. Removing Shield
leaves them in place. This verification does not require a Git repository.

When Shield writes local stdio settings, it uses private indirection. Host files
contain a Shield launcher, project path and opaque random token; the connection ID, transport
digest, command, arguments and environment stay in private review state. Tokens
retain the same locator for the same connection, agent and alias; approved
transport changes rotate a separate random host-entry generation. Cached launch
commands resolve the current approved transport. Removal, disabling or renaming
the binding revokes its locator. The launcher replaces itself with the connector,
preserving native stdio and signal ownership. Review-state permissions must be private.
Approval publishes registered connection entries automatically, and interrupted
publication resumes in the same session or from a cached launcher. Reloading an
already running connector depends on the agent host.

Launcher setup works without Git and with tracked host files, including
`.mcp.json` and `.codex/config.toml`. It does not change repository tracking or
ignore rules. Private transport settings remain in private review state; generated
host entries contain only launcher information. The generated executable and
project paths are absolute: they can reveal a local home folder or username and
will not work unchanged on a teammate's machine. Setup shows this warning before
approval. Review the host-file diff before committing or sharing it; Shield does
not inspect Git tracking or alter it. Local HTTP overrides work when
an existing native entry matches exactly; generating a new private HTTP entry is
unsupported and refuses before host writes. Shield never drops a local override
as a fallback.

Managed connection, association, binding and enablement edits use in-session
confirmation forms, including on Codex. Ordinary tool permission alone does not
approve private connection settings.
Read-only automatic matching uses the activated pack's authority without an
additional connection confirmation or configuration write.
Permission-only MCP proposals retain the existing `policy_apply` workflow.
Changing a pack variable or release that changes resolved connection settings
also requires the connection-settings form; a mixed proposal is not partially activated.

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

The `connection` predicate uses the same typed field-map grammar over connection
facts: `/transport/type`, `/transport/command`, `/transport/args`,
`/transport/env`, `/transport/url` and `/source` (`project`, `local`,
`host-project` or `host-user`). Declare
`mcp-connection-facts-v1` when using it. These facts describe launch settings,
not remote credentials, OAuth identity or every region used by a script.
They are derived from the selected connection; tool input cannot supply them.
Automatically discovered settings carry observed provenance, rather than reviewed
connection provenance.
There is no generic server-default inference or script analysis.

An integration may declare `effective_fields` with `mcp-effective-fields-v1`.
Each named fact supplies an `argument` pointer and a `default` pointer into the
configured transport. A present argument wins, including null or a wrong type;
only an omitted argument may use the configured setting. Rules read the result
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
