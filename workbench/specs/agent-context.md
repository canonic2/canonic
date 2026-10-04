# Agent context

This spec covers how AI chats in the editor learn which page the user has
open in the workbench canvas. It builds on
[Copy a reference](../docs/canvas.md#copy-a-reference), the
[VS Code extension contract](vscode-extension.md), and the canvas host
messages in [workbench.js](../workbench/workbench.js). Facts about each
agent live in the shared [AI agents research](../../../docs/research/agents/README.md).

## Purpose and scope

When a text file is open in VS Code, chats treat it as context: the user can
ask about "this file" without naming it. The canvas is a webview, and no chat
treats a webview that way ([discoveries](#discoveries)). Without this feature
the user clicks **Copy reference** and pastes.

**Agreed outcome (2026-10-03):** an agent in a VS Code chat can know the
current canvas view, with the same information **Copy reference** copies, without
the user pasting it.

In scope: Claude Code and Codex (extensions and CLIs), GitHub Copilot, and
Cursor. Other MCP clients get the same tool at no extra cost. Annotations and
screenshots keep their own path, the
[handoff](../docs/annotations-and-handoff.md#hand-off-to-an-agent).

## The current view

The support API can publish a version-2 record with canvas ID, selected
artboard ID, complete reference text, and every artboard's space/view/size/status.
The publisher under `src/canvas/` serializes updates to participating spaces
and their shared-root siblings. User activity determines the current canvas;
background readiness and heartbeats only update its report. Oversize reports
fail explicitly without silently truncating artboards. The server validates
version 2 and retains the legacy single-view shape for older clients. Shield
continues to consume `view.text`.

The current canvas UI continues to publish the legacy single-view shape. Its
reference formatter can contribute each instance's reference to a future host;
the singleton publisher is disabled only for an explicit isolated runtime.

**Requirement (agreed):** the current view carries what
[reference.js](../workbench/reference.js) puts in **Copy reference**:

- the page's label and design file (`src`);
- its story (label and id) on a Storybook lens, otherwise its state (label and
  id, with the same fallback to the first or `default` state);
- the lens (label and key) with its upstream implementation address, or
  `Design`.

**Implemented (2026-10-03):**

- [agent-context.js](../workbench/agent-context.js) builds the view with
  `wbReference.text()`, the function behind **Copy reference**, so the two
  can't differ. It adds the ids: `src`, `state` or `story`, and `lens`.
- Every canvas posts its view to `POST /_workbench/view` under a random
  client id: when it settles on a page (each `wb-here`), on `hashchange`
  and `wb-frame-change`, and every 30 seconds as a heartbeat. The VS Code tab
  and a browser canvas both report, embedded or not. A `file://` canvas
  doesn't.
- A view that isn't ready is posted as `null`, in the cases where
  **Copy reference** says to wait.
- A canvas posts `closed` on `pagehide`. One that stops reporting stops
  counting after 3 minutes.
- [agent-view.js](../agent-view.js) keeps the views. `GET /_workbench/view`
  answers `{ root, open, view, changedAt }` for the canvas changed most
  recently. A heartbeat doesn't count as a change.
- With no canvas open, `open` is false and agents are told nothing.

## Finding the server

The server's port changes between runs, and each VS Code window with a
workbench runs its own server.

- In a project with a `.canonic/` folder, the server writes
  `.canonic/.workbench/server.json` (`url`, `pid`) once it listens, and removes
  it on close if the file is still its own. Projects without `.canonic/` get
  no file.
- A killed server leaves its file behind. Readers must check that `root` in
  the response is their project, because another space's server may hold
  that port by then.
- Projects should ignore `.canonic/.workbench/`, as this repository does.

## Delivery to agents

| Surface | Agents | What the user does | What the agent gets | Status |
| --- | --- | --- | --- | --- |
| Prompt hook (`UserPromptSubmit`) | Claude Code, Codex | Allow the hook when the host requires review; no action on each prompt | The current view on every prompt while a page is showing | Implemented |
| MCP server `workbench`: `current_view` tool, `workbench://view/current` resource | Every editor Shield configures | Asks about "this page", or attaches the resource | The current view on request | Implemented |
| Copilot language model tool, `#page` | Copilot | Types `#page`, or nothing in agent mode | The current view on request | Proposed |
| **Ask agent** button in the top bar | Whichever agent is installed | Clicks it | A chat opened with the reference prefilled | Proposed |
| Copilot chat context provider | Copilot | Nothing | The canvas as implicit context, like an open file | Waiting on VS Code |

### Shield sets it up

**Decision (2026-10-03):** Shield writes the hook and MCP configuration when a
project's `.canonic/canonic.yml` has:

```yaml
workbench:
  agent_context: true
```

Shield's [workbench-context.mjs](../../../shield/src/integrations/workbench-context.mjs)
does the work in both roles: `hook` prints the view (or nothing), and `mcp`
serves the tool and resource over stdio. Shield adds the MCP server to every
enabled editor's configuration and the hook to Claude Code's
`.claude/settings.json` and Codex's `.codex/hooks.json`. Its configuration
docs cover the generated entries.

The [Shield hook specification](../../../shield/specs/hooks.md) defines
generation, runtime protocols, approval, and agent coverage. Codex requires
review and trust of new or changed hook definitions before they can run.

- Editors start MCP servers and Codex hooks in the session's directory, which
  may be below the project root or inside a nested Git repository. The
  commands walk up from there to the script. Claude hooks use
  `$CLAUDE_PROJECT_DIR`.
- The hook prints nothing and exits 0 when Workbench isn't running, no canvas
  is open, or the page isn't ready.
- The hook adds the view while the canvas is open even if its tab isn't
  visible, with how long ago it changed. People often ask about the page
  from a chat beside it.

### Proposed next

- **Copilot `#page` tool.** `contributes.languageModelTools` with
  `vscode.lm.registerTool` (VS Code 1.95), registered only when the API exists
  (`engines.vscode` is `^1.123.0`, which already includes it).
- **Copilot and Cursor without Shield.** The extension could register the same
  MCP server itself, through `registerMcpServerDefinitionProvider` (VS Code
  1.101) or `vscode.cursor.mcp.registerServer`, pointing at its own address,
  with no project files.
- **Ask agent button.** Each agent has its own entry point; see
  [Claude Code](../../../docs/research/agents/claude-code.md#commands-and-uris),
  [Codex](../../../docs/research/agents/codex.md#commands-and-uris),
  [Copilot](../../../docs/research/agents/copilot.md#opening-chat-from-an-extension),
  and [Cursor](../../../docs/research/agents/cursor.md#prompts-from-outside).
- **Copilot context provider.** Adopt `registerChatTabContextProvider` once
  it is finalized ([microsoft/vscode#271104](https://github.com/microsoft/vscode/issues/271104)).

## Acceptance criteria

- With the canvas on a page, the server's view text equals what
  **Copy reference** copies. Checked 2026-10-03 in headless Chrome against
  this repository's demo workbench, along with a state change in the same page,
  a navigation to another page, and closing the canvas.
- Changing page, state, story, or lens, or following a link in a live page,
  changes the next answer.
- Closing the canvas stops the context: the hook prints nothing and
  `current_view` says no canvas is open.
- The generated hook and MCP commands find the script from the project root
  and from nested folders, including a nested Git repository. Checked
  2026-10-03 with this repository's generated files.
- Automated: [agent-view.test.js](../agent-view.test.js) (latest canvas,
  heartbeats, closing, staleness, field filtering, the announcement file),
  [server.test.js](../server.test.js) (the route and the file's lifecycle),
  and Shield's
  [integration](../../../shield/src/integrations/workbench-context.test.mjs),
  [output](../../../shield/src/outputs/workbench-context.test.mjs), and
  [loader](../../../shield/src/loaders/workbench.test.mjs) tests.
- Manual, not yet done: the hook and tool inside the Claude Code and Codex
  extensions, with their versions recorded.

## Decisions

- **Same content as Copy reference (2026-10-03).** Agents and people should be
  pointed at a page the same way, and the reference is already the
  supported, documented form.
- **MCP and hooks before Copilot-only APIs (2026-10-03).** They reach every
  agent in scope without proposed APIs.
- **The most recently changed canvas wins (2026-10-03).** With the tab and a
  browser canvas open, the one the user last moved is the one they mean.
- **No screenshots (2026-10-03).** `current_view` reports the page; images
  stay in the handoff, which the user starts deliberately.
- **Shield distributes the configuration (2026-10-03).** Canonic projects
  already generate their agent configuration with Shield; the extension
  writes none.

## Discoveries

What each agent supports, with versions and sources, is recorded in the
shared [AI agents research](../../../docs/research/agents/README.md), checked
on 2026-10-03. The findings that shape this spec:

- **No agent treats a webview as context**, and none lets another extension
  add to what it picks up from the editor. Copilot's proposed
  [chat context provider](../../../docs/research/agents/copilot.md#implicit-context)
  is the only exception. Hence a current view held by the server.
- **MCP tools are the only integration every agent supports**
  ([protocols](../../../docs/research/agents/protocols.md#mcp)). Claude Code and
  Codex ignore VS Code's tool and MCP provider APIs, and Cursor implements
  neither, so each host registers the server its own way.
- **The Copilot context provider may not match a webview panel
  (unverified).** If it doesn't, the canvas would have to become a custom
  editor. Copilot re-reads that context when the webview posts a message, so
  the canvas would post on every view change.
- **Cross-agent IDE protocols don't help.** Each carries only files on disk,
  or belongs to one agent ([protocols](../../../docs/research/agents/protocols.md)).
- **Roo Code, Continue, and Amazon Q are excluded** as archived, unmaintained,
  or reaching end of support ([agents](../../../docs/research/agents/README.md#agents)).

## Open questions

- **Workbench root below the Canonic root.** The server announces itself in
  the folder holding `workbench.yaml`; the script looks in the folder holding
  `.canonic/canonic.yml`. They are the same folder here, but a project with
  `workbench.yaml` in a package gets no context.
- **Viewport.** **Copy reference** doesn't include the viewport. Adding it
  would change the "same as Copy reference" requirement.
- **Unverified agent behavior.** Whether the Codex IDE extension runs hooks,
  and the working directory Codex starts MCP servers in (the commands don't
  depend on it); whether Claude Code's panel accepts MCP resource @-mentions.
