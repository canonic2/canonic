# What is Shield?

Shield checks coding-agent requests against permissions you set for a project.
You can allow a test command, ask before a deployment, or deny changes to agent
settings. Its integrations check supported Claude Code and Codex tool calls,
and its history shows which rules decided them.

## When would I use it?

Use Shield to share permissions across a project, add rules for your workflow,
or investigate unexpected agent permissions. Agents can suggest changes, but
you review them before they take effect.

Start with the default baseline pack. You can add packs and custom rules later;
the first setup needs no handwritten policy.

## How a request gets a decision

Suppose an agent submits `acme-check verify`, a fictional inspection command:

1. The integration sends Shield the request and available context.
2. Shield checks the **active policy**: the configuration, rules, and packs you
   last confirmed.
3. Shield returns **Allow**, **Ask**, or **Deny** and explains the matching rules.
4. The agent host handles the result using its permission settings.

An allowance for that exact command does not automatically allow
`acme-check verify --write`. Arguments and working directory can be part of a
rule. A request with no deciding rule returns Ask.

Claude can prompt for Ask. A new Codex setup lets valid Ask requests proceed by
default, even with a Strict pack profile. See
[what happens after Ask](policy-decisions.md#what-happens-after-ask).

## The pieces you configure

| Term | Meaning |
| --- | --- |
| Policy | The rules and settings used for a project. |
| Permission pack | A reusable collection of rules, such as the default baseline. |
| Profile | A pack's Loose, Recommended, or Strict decision choices. |
| Custom rule | A project-specific rule, such as allowing one test command. |
| Registry | A source of packs. Shield includes the `main` registry. |
| Integration | The hooks and MCP connection registered with an agent. |

A pack contains policy data; installing it does not install the tool it describes.
Profiles select pack decisions and do not configure an agent's sandbox.

## You review permission changes

Shield's terminal commands show changes before asking you to confirm. Agents
can prepare proposals through MCP, the interface they use to call Shield's
tools and read these guides. You approve a proposal in the host or terminal.

Shield saves confirmed policy as a snapshot. Manual YAML edits take effect
only after you review and activate them with `shield policy review`.

## Understand the boundary

Shield checks submitted requests. It cannot observe every effect of a running
command or replace your agent's sandbox. A host that skips, disables, or times
out a hook can bypass its checks.

After setup, restart and trust the agent so it loads the hooks. The
[integration guide](claude-project.md) covers supported requests and loading.
The [SDK adapter and macOS runner](README.md#integrate-with-shield) are separate
integration options.

## Start with one project

Follow [your first Shield project](getting-started.md) to initialize policy,
connect your agent, and inspect the result. Then learn
[how policy decisions work](policy-decisions.md), or go directly to
[finding a permission pack](local-registry.md#find-and-install-a-pack).
