# Your first Shield project

Set up a project policy, connect Claude Code or Codex, and inspect an agent
request. For an overview of the concepts, read [What is Shield?](introduction.md).

## Before you begin

You need the `shield` command on your path and an interactive terminal. If you
are building from the Canonic checkout, the local installer is:

```sh
make -C packages/shield install-local
```

The installer puts `shield` in `~/.local/bin`; add that directory to your `PATH`.
See [development](development.md) for build details. Native setup uses Unix
command hooks and refuses Windows setup. macOS is the verified native platform.

Open a terminal in your project. Default setup downloads a signed baseline
pack from Shield's main registry, so you need network access.

## 1. Initialize the project

From the project directory, run:

```sh
shield init
```

Shield asks you to choose a profile for the **baseline**, its default pack of
permission rules. Recommended is preselected. A profile selects the pack's
decisions; it does not change your agent's sandbox.

At the agent selection, use Space to mark the agents to configure, then Enter to
continue. Agents whose commands are on your path are preselected. Enter with
none marked skips agent setup; you can add it later.

Review the packs, agent settings, and Shield binary path, then confirm. Shield
saves the active policy and creates `.shield/` for policy files and private
review state. Cancel leaves the project unchanged; a failed baseline download
or verification leaves a new project unenrolled.

For a first setup with a different baseline profile, use
`shield init --profile strict`. To start without adding the default baseline,
use `shield init --no-baseline`. An empty policy returns Ask for unmatched requests.

## 2. Load the agent integration

If you selected an agent during initialization, its registrations are already
installed. To add one later, choose the command for your agent:

```sh
shield mcp setup --agent claude
```

```sh
shield mcp setup --agent codex
```

Each command installs hooks and MCP tools for that agent after review, keeping
any other selected agent registered.

Restart the agent after setup. Trust the project, enable hooks, and approve the
MCP connection as the host requires. In Codex, also review the hook definitions
through `/hooks`.

**Codex:** new setups let valid Ask requests proceed by default, even with the
Strict profile. Saved choices are preserved; Deny and protection failures still
block. Check this fallback in the review. See
[Ask handling](policy-decisions.md#what-happens-after-ask).

Hooks check supported tool requests. MCP lets the agent inspect policy, read
guides, and propose changes. Check that both load in the restarted agent.

## 3. Inspect the result

Run these from the same project:

```sh
shield status
shield doctor
```

Check that `status` names the right project and lists your packs and agents.
`doctor` adds diagnostic checks and recovery commands. Resolve any reported
repair before continuing. Both report installation state; live interception
remains `unverified`.

To inspect the rules themselves, use:

```sh
shield rule list --json
```

JSON output includes full identifiers and structured details.

## 4. Inspect an agent request

Continue your ordinary work in the restarted agent. When Shield records a
request, you can inspect it in the terminal:

```sh
shield history list
```

Copy an entry ID from the list and use it in place of `ENTRY`:

```sh
shield history show ENTRY
```

Find a command the agent submitted. The detail view shows its decision, matching
rules, and any reported completion. A decision alone does not show that execution
finished. If history is empty, follow
[integration troubleshooting](troubleshooting.md#the-agent-does-not-seem-to-use-shield).

## Where to go next

Read [policy decisions](policy-decisions.md) to understand the result of a
request. Then [find a permission pack](local-registry.md#find-and-install-a-pack)
or [write a custom rule](policy-rules.md#your-first-rule) for your project's
needs. Agents can use [Shield's MCP tools](project-mcp.md) to inspect policy and
prepare changes for your review.
