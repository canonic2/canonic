# Agent context

This spec covers how AI chats in the editor learn which page the user has
open in the workbench canvas. It builds on
[Copy a reference](../../docs/canvas.md#copy-a-reference), the
[VS Code extension contract](vscode-extension.md), and the canvas host
messages in [workbench.js](../../ui/legacy/workbench.js).

## Purpose and scope

An agent should understand which Workbench view the user means by “this
page” without requiring the user to paste a reference. The context carries
exactly the information exposed by Copy reference. Hooks and MCP provide
explicit delivery; host-native implicit context is a proposed integration,
subject to each editor's supported capabilities.

In scope: Claude Code and Codex (extensions and CLIs), GitHub Copilot, and
Cursor. Other MCP clients get the same tool at no extra cost. Annotations and
screenshots keep their own path, the
[handoff](../../docs/annotations-and-handoff.md#hand-off-to-an-agent).

## The current view

The support API can publish a version-2 record with canvas ID, selected
artboard ID, complete reference text, and every artboard's space/view/size/status.
The publisher under `ui/src/modules/canvas/` serializes updates to participating spaces
and their shared-root siblings. User activity determines the current canvas;
background readiness and heartbeats only update its report. Oversize reports
fail explicitly without silently truncating artboards. The server validates
version 2 and the single-view report shape. Agent
integrations consume `view.text`.

The canvas composition publishes this complete report through its controller-owned
publisher. The legacy singleton publisher remains for legacy hosts and is disabled
for an isolated renderer. A content renderer reports instance-local metadata to
the outer canvas; it does not publish another application-level current view.
See the [rendering flow](../system/rendering-flow.md).

**Requirement (agreed):** the current view carries what
[reference.js](../../ui/legacy/reference.js) puts in **Copy reference**:

- the page's label and design file (`src`);
- its story (label and id) on a Storybook lens, otherwise its scenario (label and
  id): the declared scenario matching the view, else the page's first listed
  scenario; when the page lists none, or a lens has no scenario map,
  `{ id: 'default', label: 'Default' }`;
- the iteration the artboard shows, if any, as `` - Iteration: <Label> — `<id>` ``,
  followed by `` - Iteration source: `<path>` `` when the iteration has a source.
  The reference does not name the declared artboard;
- the lens (label and key) with its upstream implementation address, or the
  resolved authored lens label.

The composed canvas's reference additionally names every artboard's space,
dimensions and status through [review/reference.js](../../ui/src/modules/review/reference.js).
Each version-2 artboard record also carries `artboard` (the declared artboard
ID) and `iteration` when they apply.
The legacy singleton reference omits dimensions; that does not limit the
complete canvas record.

**Legacy singleton publication behavior:**

- [agent-context.js](../../ui/legacy/agent-context.js) builds the view with
  `wbReference.text()`, the function behind **Copy reference**, so the two
  can't differ. It adds the ids: `src`, `scenario` or `story`, `lens`, and
  `iteration` when the view shows one. The server keeps the same fields.
- Every canvas posts its view to `POST /_workbench/view` under a random
  client id: when it settles on a page (each `wb-here`), on `hashchange`
  and `wb-frame-change`, and every 30 seconds as a heartbeat. The VS Code tab
  and a browser canvas both report, embedded or not. A `file://` canvas
  doesn't.
- A view that isn't ready is posted as `null`, in the cases where
  **Copy reference** says to wait.
- A canvas posts `closed` on `pagehide`. One that stops reporting stops
  counting after 3 minutes.
- [agent_view.rs](../../server/src/agent_view.rs) keeps the views. `GET /_workbench/view`
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
- Projects should ignore `.canonic/.workbench/`.

## Delivery to agents

| Surface | Agents | What the user does | What the agent gets | Status |
| --- | --- | --- | --- | --- |
| Prompt hook (`UserPromptSubmit`) | Claude Code, Codex | Allow the hook when the host requires review; no action on each prompt | The current view on every prompt while a page is showing | Implemented |
| MCP server `workbench`: `current_view` tool, `workbench://view/current` resource | Every editor the agent configuration covers | Asks about "this page", or attaches the resource | The current view on request | Implemented |
| Copilot language model tool, `#page` | Copilot | Types `#page`, or nothing in agent mode | The current view on request | Proposed |
| **Ask agent** button in the top bar | Whichever agent is installed | Clicks it | A chat opened with the reference prefilled | Proposed |
| Copilot chat context provider | Copilot | Nothing | The canvas as implicit context, like an open file | Proposed; host capability review required |

### The agent configuration sets it up

The project's agent configuration writes the hook
and MCP configuration when Workbench agent context is enabled for the project.

One context reader does the work in both roles: `hook` prints the view (or
nothing), and `mcp` serves the tool and resource over stdio. The
configuration adds the MCP server to every enabled editor's configuration and
the hook to Claude Code's `.claude/settings.json` and Codex's
`.codex/hooks.json`. Codex requires review and trust of new or changed hook
definitions before they can run.

- Editors start MCP servers and Codex hooks in the session's directory, which
  may be below the project root or inside a nested Git repository. The
  commands walk up from there to the script. Claude hooks use
  `$CLAUDE_PROJECT_DIR`.
- The hook prints nothing and exits 0 when Workbench isn't running, no canvas
  is open, or the page isn't ready.
- The hook adds the view while the canvas is open even if its tab isn't
  visible, with how long ago it changed. People often ask about the page
  from a chat beside it.

### Proposed integrations

- A Copilot page-reference tool, callable explicitly or by the agent.
- Extension-owned MCP registration for hosts that support it, without requiring
  generated project configuration. Keep the same reference and discovery rules.
- An Ask agent action that opens the installed agent with the reference.
- Implicit canvas context where an editor exposes a supported provider API.

Each integration requires a review of the editor's documented capabilities
before implementation. No proposed editor API or specific host version is
assumed available by this contract.

## Acceptance criteria

- With the canvas on a page, the server's view text equals what
  **Copy reference** copies, including after scenario/lens changes and navigation.
- Changing page, scenario, story, or lens, or following a link in a live page,
  changes the next answer.
- Closing the canvas stops the context: the hook prints nothing and
  `current_view` says no canvas is open.
- The generated hook and MCP commands find the script from the project root
  and from nested folders, including a nested Git repository.
- Automated: [agent_view tests](../../server/src/agent_view/tests.rs) (latest canvas,
  heartbeats, closing, staleness, field filtering, the announcement file),
  [server.test.js](../../server/node/server.test.js) (the route and the file's lifecycle),
  and the context reader's, output's and loader's own tests in the package
  that generates the agent configuration.
- Manual, not yet done: the hook and tool inside the Claude Code and Codex
  extensions, with their versions recorded.

## Integration constraints

- People and agents receive the same resolved reference text. Single-view and
  canvas reports preserve their own complete identities; selected focus does
  not hide other supplied artboards.
- User activity determines the current canvas. Background readiness updates
  and heartbeats cannot steal precedence from the canvas last used.
- Context contains references and status. Screenshot sharing remains a
  deliberate handoff action.
- Agent configuration owns generated project integration. Editor-specific
  registration must not introduce a second context model or conflicting files.

## Open questions

- **Workbench root below the Canonic root.** The server announces itself in
  the folder holding `workbench.yaml`; the script looks in the folder holding
  `.canonic/canonic.yml`. They are the same folder here, but a project with
  `workbench.yaml` in a package gets no context.
- **Host acceptance.** Confirm hook invocation, tool/resource access and
  script discovery inside each supported editor, with host versions recorded
  in the verification evidence.
