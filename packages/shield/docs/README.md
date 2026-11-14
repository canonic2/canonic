# Shield documentation

Shield checks coding-agent requests against your project's permission rules.

Start with [What is Shield?](introduction.md), then set up
[your first project](getting-started.md).

For an existing project, choose a task below or go to
[troubleshooting](troubleshooting.md).

## Get started

- [What is Shield?](introduction.md): why you would use it, its main concepts,
  and the boundary between policy and agent enforcement.
- [Your first project](getting-started.md): initialize policy, connect an agent,
  and check the installation.
- [Understand policy decisions](policy-decisions.md): Allow, Ask, Deny, profiles,
  and the difference between a decision and what the host does next.
- [Use the terminal](operator-experience.md): picker controls, reviews, JSON
  output, and a command overview.

## Work with your project

- [Policy files and settings](policy-files.md): what the `.shield/` files hold,
  how changes take effect, and every setting key.
- [Manage policy, approvals, and history](project-review.md): review changes,
  manage protected files, add access paths, and inspect requests.
- [Approve a batch for one conversation](session-grants.md): reuse a bounded,
  temporary approval while keeping project policy unchanged.
- [Find and manage permission packs](local-registry.md): search, inspect, install,
  configure, update, and remove packs; register your own sources.
- [Check GitHub GraphQL permissions](local-registry.md#github-graphql-reads):
  distinguish engine analysis support from installed pack permissions.
- [Connect Claude and Codex](claude-project.md): load hooks and MCP directly or
  through Box, understand request coverage, and check the limits of native
  interception.
- [Connect Gemini and Antigravity](google-agents.md): set up native integrations,
  understand confirmation and session access, and check verification limits.
- [Connect Copilot CLI](copilot-cli.md): install its native hooks and understand
  prompt-mode Ask, timeout behavior and request coverage.
- [Connect OpenCode V1](opencode.md): install its owned plugin and understand
  standing Ask fallback, V1/V2 boundaries and loading requirements.
- [Configure project MCP servers](mcp-connections.md): bind connections and pack
  permissions, review host projections, and keep local stdio settings private.
- [Troubleshoot Shield](troubleshooting.md): investigate unexpected decisions,
  missing integrations, invalid policy files, and recovery errors.

## Write and look up rules

- [Write your first rule](policy-rules.md): start with one exact command, then
  learn argument matching, path restrictions, web rules, and reusable pack rules.
- [Rule reference](rule-reference.md): look up document fields, rule fields,
  selectors, and required capabilities.
- [Validate permission packs](pack-validation.md): check artifacts and test
  expected decisions without executing the commands.

## Use Shield from an agent

- [Project MCP](project-mcp.md): inspect policy, read these guides, propose a
  change, request approval, and evaluate diagnostic requests.

## Integrate with Shield

For developers embedding Shield in a host application:

- [Claude SDK integration](claude-read-only.md): integrate the restricted
  text-read consumer and its one-time approval flow.
- [Native execution](native-run.md): use the separate bounded macOS runner for
  checked launches and owned process cleanup.
- [Enforcement handoff](enforcement-handoff.md): understand the transport contract
  when embedding enforcement in a trusted host.

## Recovery and development details

- [Compiled policy cache](compiled-cache.md): locate the disposable cache of
  compiled policy files or choose another folder for it.
- [Developing Shield](development.md): build the CLI, find source ownership,
  choose engineering checks, and collect diagnostics.
