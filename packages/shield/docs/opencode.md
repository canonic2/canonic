# Connect OpenCode V1

Shield supports the OpenCode V1 plugin interface verified on OpenCode 1.14.27.
OpenCode V2 uses a different plugin and permission API and is unsupported.
Shield does not auto-select OpenCode merely because its executable is present.

From an initialized project, run:

```sh
shield mcp setup --agent opencode
```

Confirm the proposed files and standing Ask fallback, then restart OpenCode.
Setup writes `.opencode/plugins/shield.js` and adds Shield to V1
`opencode.json`. It preserves unrelated plugins/settings and refuses edited
Shield files. Remove it with `shield mcp setup --remove --agent opencode`.

The plugin checks file reads/writes, directory searches and
fetch/search requests through the shared policy. It leaves tool execution with
OpenCode. Unmapped tools, including `apply_patch` and `multiedit`, are withheld;
only todo bookkeeping bypasses inspection. External MCP
policy bindings and Hub- or Box-managed publication are unsupported.

After Allow, Shield installs a frozen copy of the exact evaluated arguments
and locks the host's argument property. A later plugin that changes those
arguments fails the call; unrelated plugin output fields remain editable.

OpenCode V1 cannot ask for approval from this plugin callback. Its standing Ask
fallback defaults to Deny and is shown during setup. Repeated setup preserves
a saved confirmed fallback; there is no separate fallback picker. Policy changes use a fresh terminal review;
ordinary host tool approval does not approve Shield policy changes.

Shell requests retain incomplete context and the standing Ask fallback.
OpenCode's Bash/Zsh launch reads startup files before evaluating the source;
mutable plugin configuration does not prove the eventual shell or cwd. Choosing
Bash in configuration does not establish a clean launch or automatic argv Allow.

If Shield's child process cannot start, times out or returns an invalid reply,
the loaded plugin withholds the tool. If OpenCode skips or cannot load the
plugin itself, the plugin cannot enforce anything. Verify that the running host
loaded it; a written setup file does not establish coverage. A callback after
execution records optional history and cannot undo the operation.

Temporary-root access is available; agent skill libraries, session outputs and
non-temporary scratch receive no automatic allowance from this adapter.
