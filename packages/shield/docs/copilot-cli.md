# Connect Copilot CLI

Copilot is opt-in; onboarding does not preselect it from executable discovery.
From an initialized Shield project, run:

```sh
shield mcp setup --agent copilot
```

Review and confirm setup, then restart Copilot in the project root and accept
its folder-trust prompt. Shield writes `.github/hooks/shield.json` and adds its
server to `.mcp.json`. It preserves unrelated callbacks and servers. Check the
installation with `shield doctor`; remove it with
`shield mcp setup --remove --agent copilot`.

Copilot CLI and VS Code Copilot use separate Shield integrations. These
instructions apply to the CLI. For a plain folder without Git metadata,
starting Copilot in a child directory does not load the parent hook file.
Start in the configured project root.
Copilot also loads hooks from repository Claude settings. If Claude Shield
hooks are installed in the same project or known Copilot user settings, setup
refuses the incompatible combination. Use a separate trusted Copilot checkout
or profile without those imported hooks. Shield leaves the Claude files intact;
removing Copilot remains available. Complete plugin/managed-source coexistence
is not verified.

The integration checks shell commands, absolute file reads/writes and
directory searches against the shared policy. Reusing a `shellId` retains
host permissions because the hook does not establish that shell's starting
directory. Unknown arguments, unsupported patch/fetch tools and ambiguous
MCP tool names are refused. Other unmapped tools are also refused; only
`report_intent` and `task_complete` bookkeeping bypass inspection.
Project policy bindings for external MCP servers
are unsupported; the Shield MCP tools remain available for inspection and
proposal preparation. Policy changes use a fresh terminal review.

Native Ask asks Copilot to confirm the request. In prompt mode, Copilot refuses
Ask because it cannot show a prompt, even with `--allow-all-tools`. Interactive
acceptance and cancellation have not been verified. Copilot blocks a crashed
command hook, but a timed-out hook continues under native permissions. Shield
uses a ten-second limit, so a timeout leaves that call without Shield
enforcement.

The adapter has macOS evidence with Copilot CLI 1.0.105 and an offline scripted
provider. It does not claim Linux/Windows host loading, exact before/after
correlation or Box-managed publication. Temporary folders and selected skill
libraries receive ordinary working-file access; session outputs and other
runtime assets do not.
