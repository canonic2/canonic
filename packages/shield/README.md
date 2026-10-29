# Shield

Project permissions for coding agents.

Shield checks the commands, file reads and web requests that Claude Code and
Codex submit against rules you keep with your project. Each request gets
**Allow**, **Ask** or **Deny**, with the rules that decided it. Agents can
propose new rules, but nothing changes until you review and confirm it.

## Highlights

- **Ready on first run.** `shield init` installs a signed baseline pack and
  connects your agents. You don't have to write any policy to start.
- **Reusable permission packs.** Install packs for the tools you use and pick a
  Loose, Recommended or Strict profile for each one.
- **Precise rules.** Match an exact command, individual arguments and options,
  paths, the working directory or web addresses, in YAML or JSON. Allowing
  `acme-check verify` does not allow `acme-check verify --write`.
- **You confirm every change.** Policy, pack and integration changes show a
  review and need confirmation in your terminal or agent host. `--json` output
  cannot authorize a change.
- **Agents can help.** Agents use Shield's MCP tools to inspect policy, read the
  bundled guides and draft rules for commands they were asked about.
- **History.** See every recorded request, its decision and the rules behind it.
- **One binary.** A Rust CLI that keeps its policy in your project's `.shield/`
  folder.

## Installation

Shield is built from source and has not been published to package managers yet.
You need [Rust](https://rustup.rs/); the checkout pins the toolchain version.

```sh
git clone https://github.com/canonic2/canonic.git
cd canonic
make -C packages/shield install-local
```

This installs `shield` into `~/.local/bin`; add that folder to your `PATH`, or
set `PREFIX=/path/to/prefix` to install somewhere else.

macOS is the verified platform. Native agent setup uses Unix command hooks and
refuses to run on Windows.

## Quick start

In your project's folder:

```sh
shield init
```

Choose a profile for the baseline pack, mark the agents to connect with Space,
then press Enter and confirm the review. Restart the agent so it loads the hooks
and MCP server, and approve them if it asks.

Check the installation, then look at what the agent submits as you work:

```sh
shield status
shield doctor
shield history list
shield history show ENTRY
```

[Your first project](docs/getting-started.md) walks through each step.

## Writing a rule

Run `shield rule add` for a guided form, or write the rule in YAML. This rule
allows `acme-check verify` anywhere inside the project:

```yaml
schema_version: 1
kind: custom
capabilities: [structured-rules-v1, argument-rules-v1, location-v1]
rules:
  - id: verify-project
    decision: allow
    conditions:
      domain: argv
      executable: acme-check
      arguments: {kind: exact, values: [verify]}
      cwd: {within: {root: project}}
```

Activate edits you make by hand with `shield policy review`. When nothing
decides a request, the result is Ask. See [writing rules](docs/policy-rules.md)
and the [rule reference](docs/rule-reference.md).

## Permission packs

```sh
shield pack search node
shield pack add node --profile strict
shield pack update
shield pack remove node
```

Packs come from Shield's built-in `main` registry, whose signing key is pinned
in the binary, or from local folders and signed HTTPS sources that you register.
Installing a pack adds rules only; it does not install the tool the pack
describes. See [permission packs](docs/local-registry.md).

## Agents

```sh
shield mcp setup --agent claude
shield mcp setup --agent codex
shield mcp setup --remove --agent codex
shield mcp setup --dry-run
```

Setup installs hooks, the MCP server and permission settings together, and it
leaves your other hooks, servers and settings alone. Claude Code shows its own
approval prompt for Ask. A new Codex setup lets Ask requests proceed by default;
the review shows this setting, and Deny still blocks. See
[connecting Claude and Codex](docs/claude-project.md).

## What Shield does not do

- It checks requests as they are submitted. It cannot see everything a running
  command does, and it does not replace your agent's sandbox.
- A host that skips, disables or times out a hook bypasses Shield. `status`
  reports installed integrations, not live interception.
- It supports Claude Code and Codex. Linux and Windows have not been verified.

## Documentation

- [What is Shield?](docs/introduction.md)
- [Your first project](docs/getting-started.md)
- [Policy decisions](docs/policy-decisions.md)
- [Policy, approvals and history](docs/project-review.md)
- [Troubleshooting](docs/troubleshooting.md)
- [All guides](docs/README.md)

You can also open the guides from an agent through Shield's MCP tools, which
search the guides built into the binary.

## Development

```sh
make -C packages/shield build
make -C packages/shield test
```

[Developing Shield](docs/development.md) covers the source layout, each check
and debug builds.

## License

Shield is subject to the [Canonic Proprietary License](LICENSE).
Third-party components keep their own licenses.
