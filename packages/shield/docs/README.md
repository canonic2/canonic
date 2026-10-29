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

- [Manage policy, approvals, and history](project-review.md): review changes,
  manage protected files, add access paths, and inspect requests.
- [Find and manage permission packs](local-registry.md): search, inspect, install,
  configure, update, and remove packs; register your own sources.
- [Connect Claude and Codex](claude-project.md): load hooks and MCP, understand
  request coverage, and check the limits of native interception.
- [Configure project MCP servers](mcp-connections.md): bind connections and pack
  permissions, review host projections, and keep local stdio settings private.
- [Troubleshoot Shield](troubleshooting.md): investigate unexpected decisions,
  missing integrations, unapplied edits, and recovery errors.

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

- [Active policy and repair](runtime-activation.md): understand when changes take
  effect and recover interrupted policy-file updates.
- [Compiled policy cache](compiled-cache.md): locate or prepare the disposable
  cache used to load active policy.
- [Developing Shield](development.md): build the CLI, find source ownership,
  choose engineering checks, and collect diagnostics.
