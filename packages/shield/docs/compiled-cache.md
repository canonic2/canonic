# Compiled policy cache

Shield caches a compiled copy of your active policy for faster loading. It
rebuilds missing, corrupt, or incompatible entries before evaluation, without
a background service. Deleting the cache does not change policy or approvals.

## Prepare the cache yourself

To prepare the active policy's cache explicitly, run:

```sh
shield cache prepare
```

Add `--json` for structured output. Preparation does not activate file edits or
proposals. Invalid saved source or an inaccessible cache directory blocks evaluation.
Inside an agent session, `cache_prepare {}` performs the same rebuild without
confirmation. If the saved location is unusable, the agent can supply an absolute
`cache_dir` for a new private location; you approve that change in the session.
Policy stays unchanged and the old cache location is preserved.

## Find the cache

The default cache is outside the project:

| Host | Location |
| --- | --- |
| macOS | `~/Library/Caches/Shield` |
| Linux | `$XDG_CACHE_HOME/shield`, or `~/.cache/shield` if unavailable or relative |
| Windows | `%LOCALAPPDATA%\Shield\Cache` |

`shield status --json` reports the root, namespace, expected size, file
presence, and `compatibility`: `current` when this Shield version can use the
saved plan, or `rebuild-required` when another version or policy built it.
`shield doctor` names a plan that needs rebuilding. On load, Shield verifies the artifact's BLAKE3 digest against the
active policy. Cached regex and glob programs avoid recompiling patterns;
filesystem observations are collected for each request.

Policy changes publish a new plan. CLI commands and hooks load it once per
process; MCP retains evaluators for repeated diagnostics. A cache miss rebuilds
from saved source without changing activation or approvals.

On Unix, directories use mode `0700` and files `0600`. Native hooks protect the
cache and destructive access to its parents. Windows access controls and native
Linux/Windows behavior are unverified.

## Choose another location

Choose a different root during initialization, setup or repair:

```sh
shield init --cache-dir /absolute/path/shield-cache
shield mcp setup --agent codex --cache-dir /absolute/path/shield-cache
shield cache prepare --cache-dir /absolute/path/shield-cache
```

CLI relative paths resolve against the invoking directory. Shield saves the absolute
root for hooks, SDK consumers, and MCP. Changing it through the `cache_prepare`
MCP tool requires an absolute `cache_dir` and human confirmation. Shield
creates a missing root itself. An existing root must belong to you and be
private: Shield refuses one that others can read or enter (for example mode
`0755`) and does not change its permissions. Run `chmod 700` on it, or name a
folder that does not exist yet. A root inside
the project gets an owned `.gitignore`. If preparation fails, Shield reports the
error rather than choosing another location.

Saved policy source remains in `.shield/review/sources/`. Keep it and the
authority checkpoint for recovery; only the compiled cache is disposable.
Cache and source storage are not automatically pruned.
