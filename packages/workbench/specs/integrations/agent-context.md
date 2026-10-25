# Agent context

This spec covers how AI chats in the editor learn which page the user has
open in the workbench canvas. It builds on
[Copy a reference](../../docs/canvas.md#copy-a-reference), the
[VS Code extension contract](vscode-extension.md), and the canvas host
messages in [workbench.js](../../ui/legacy/workbench.js). It owns the canvas
context record, its publication, precedence and heartbeats, and its delivery
to agents; [multiple artboards](../canvas/multiple-artboards.md) owns the
artboard instances the record describes.

**Status:** the version-2 canvas context record, reference parity with Copy
reference, publication with precedence and heartbeats, server discovery, the
prompt hook and the `workbench` MCP server are implemented. The editor
integrations under [proposals](#proposals) are not agreed. Context for a
`workbench.yaml` below the Canonic root and acceptance inside each editor are
open; see [open questions](#open-questions).

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

## Requirement: the canvas context record

### The record

The canvas publishes one version-2 record
([review/reference.js](../../ui/src/modules/review/reference.js) `report`):
canvas ID, selected artboard ID, artboard order, and for every artboard its
space, view (page `src`, scenario, lens), size, status, and the declared
artboard ID (`artboard`) and `iteration` when they apply, plus the complete
readable reference as `text`. The server validates version 2 and the
single-view report shape. Agent integrations consume `text`.

### Reference parity

**Requirement:** people and agents receive the same resolved
reference text. On the composed canvas, **Copy reference** in the top bar
copies the canvas reference, which is exactly the record's `text`: the
artboard count and selected artboard, then for each artboard its ID and
space, its size in CSS pixels, its single-view reference and its status.

Each artboard's single-view reference carries what
[reference.js](../../ui/legacy/reference.js) puts in a one-page reference:

- the page's label and design file (`src`);
- its story (label and id) on a Storybook lens, otherwise its scenario (label and
  id): the declared scenario matching the view, else the page's first listed
  scenario; when the page lists none, or a lens has no scenario map,
  `{ id: 'default', label: 'Default' }`;
- the iteration the artboard shows, if any, as `` - Iteration: <Label> — `<id>` ``,
  followed by `` - Iteration source: `<path>` `` when the iteration has a source.
  The reference does not name the declared artboard; the record's
  `artboard` field does;
- the lens (label and key) with its upstream implementation address, or the
  resolved authored lens label.

Single-view and canvas reports preserve their own complete identities;
selected focus does not hide other supplied artboards. Keep the selected-view
compatibility fields for existing consumers, but hooks and MCP consume the
complete canvas `text` rather than only those fields.

### Publication, precedence and heartbeats

- The canvas composition publishes the record through its controller-owned
  publisher ([publication.js](../../ui/src/modules/canvas/publication.js)). A
  content renderer reports instance-local metadata to the outer canvas; it
  does not publish another application-level current view. See the
  [rendering flow](../system/rendering-flow.md).
- Publish on artboard add and remove, view, status and size changes, and
  selection changes, and every 30 seconds as a heartbeat.
- The publisher sends the full record to each participating space's context
  service, and to the spaces sharing a participating space's root, under the
  same canvas ID, serializing updates. Each service keeps its own root for
  the context reader's validation. Shared-root announcements are handled
  explicitly rather than letting space selection decide which context
  survives.
- Closing the canvas or removing a space's participation withdraws the record
  there. A canvas that stops reporting stops counting after 3 minutes, so
  heartbeat expiry handles vanished canvases.
- User activity (selection and user changes) determines the current canvas.
  Automatic loads, background readiness updates and heartbeats only update its
  report; they never take precedence from a different canvas the user has
  used since.
- [agent_view.rs](../../server/src/agent_view.rs) keeps the views.
  `GET /_workbench/view` answers `{ root, open, view, changedAt }` for the
  canvas changed most recently. A heartbeat doesn't count as a change. With no
  canvas open, `open` is false and agents are told nothing.
- Context stays available during partial loading and failure: a loading or
  failed artboard is represented without hiding usable neighbors. It makes no
  claim that an agent has access to every referenced root.
- A canvas with no artboards reports open with an empty list and no selected
  artboard; closing the canvas reports closed.
- Complete context is bounded at 32 artboards and 100,000 text characters.
  An oversize report fails explicitly; slicing text or silently dropping later
  artboards violates the complete-context requirement. UI delivery must expose
  that error without hiding other available review operations.
- Context contains references and status. Screenshot sharing remains a
  deliberate handoff action.

## Delivery and discovery

### Finding the server

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

### Delivery to agents

| Surface | Agents | What the user does | What the agent gets |
| --- | --- | --- | --- |
| Prompt hook (`UserPromptSubmit`) | Claude Code, Codex | Allow the hook when the host requires review; no action on each prompt | The current view on every prompt while a page is showing |
| MCP server `workbench`: `current_view` tool, `workbench://view/current` resource | Every editor the agent configuration covers | Asks about "this page", or attaches the resource | The current view on request |

### The agent configuration sets it up

The project's agent configuration writes the hook
and MCP configuration when Workbench agent context is enabled for the project.
Agent configuration owns generated project integration. Editor-specific
registration must not introduce a second context model or conflicting files.

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

## Proposals

These integrations are proposals.

| Surface | Agents | What the user does | What the agent gets |
| --- | --- | --- | --- |
| Copilot language model tool, `#page` | Copilot | Types `#page`, or nothing in agent mode | The current view on request |
| **Ask agent** button in the top bar | Whichever agent is installed | Clicks it | A chat opened with the reference prefilled |
| Copilot chat context provider | Copilot | Nothing | The canvas as implicit context, like an open file; host capability review required |

- A Copilot page-reference tool, callable explicitly or by the agent.
- Extension-owned MCP registration for hosts that support it, without requiring
  generated project configuration. Keep the same reference and discovery rules.
- An Ask agent action that opens the installed agent with the reference.
- Implicit canvas context where an editor exposes a supported provider API.

Each integration requires a review of the editor's documented capabilities
before implementation. No proposed editor API or specific host version is
assumed available by this contract.

## Implementation note: the legacy single-reference publisher

The legacy singleton publisher remains for legacy hosts and is disabled for an
isolated renderer. Its reference omits dimensions; that does not limit the
complete canvas record.

- [agent-context.js](../../ui/legacy/agent-context.js) builds the view with
  `wbReference.text()`, the function behind the one-page reference, so the two
  can't differ. It adds the ids: `src`, `scenario` or `story`, `lens`, and
  `iteration` when the view shows one. The server keeps the same fields.
- Every canvas posts its view to `POST /_workbench/view` under a random
  client id: when it settles on a page (each `wb-here`), on `hashchange`
  and `wb-frame-change`, and every 30 seconds as a heartbeat. The VS Code tab
  and a browser canvas both report, embedded or not. A `file://` canvas
  doesn't.
- A view that isn't ready is posted as `null`, in the cases where
  **Copy reference** says to wait.
- A canvas posts `closed` on `pagehide`.

## Acceptance criteria

- With the canvas on a page, the server's view text equals what
  **Copy reference** copies, including after scenario/lens changes and navigation.
- Changing page, scenario, story, or lens, or following a link in a live page,
  changes the next answer.
- Closing the canvas stops the context: the hook prints nothing and
  `current_view` says no canvas is open.
- Agent context includes every artboard, its space and view, and the selected
  ID through both hooks and MCP. A loading or failed artboard is represented
  without hiding usable neighbors.
- Complete context reaches agents working in every participating configured
  space; shared-root spaces and simultaneous editor/browser canvases do not
  overwrite one another's identities. Heartbeats and background readiness do
  not change which canvas the user last used.
- The generated hook and MCP commands find the script from the project root
  and from nested folders, including a nested Git repository.
- Automated: [agent_view tests](../../server/src/agent_view/tests.rs) (latest canvas,
  heartbeats, closing, staleness, field filtering, the announcement file),
  [server.test.js](../../server/node/server.test.js) (the route and the file's lifecycle),
  and the context reader's, output's and loader's own tests in the package
  that generates the agent configuration.
- Manual (no evidence yet): the hook and tool inside the Claude Code and Codex
  extensions, with their versions recorded.

## Open questions

- **Workbench root below the Canonic root.** The server announces itself in
  the folder holding `workbench.yaml`; the script looks in the folder holding
  `.canonic/canonic.yml`. They are the same folder here, but a project with
  `workbench.yaml` in a package gets no context.
- **Host acceptance.** Confirm hook invocation, tool/resource access and
  script discovery inside each supported editor, with host versions recorded
  in the verification evidence.
