# Compiled policy cache

Shield compiles your [policy files](policy-files.md) into a plan and keeps it in
a cache folder outside the project, so requests load the plan instead of
compiling the files again. There is nothing to prepare: a request that finds no
usable plan compiles one from the files and saves it. Deleting the cache
changes no policy, approval or setting.

## Find the cache

| Host | Location |
| --- | --- |
| macOS | `~/Library/Caches/Shield` |
| Linux | `$XDG_CACHE_HOME/shield`, or `~/.cache/shield` if unset or relative |
| Windows | `%LOCALAPPDATA%\Shield\Cache` |

Plans are under `plans/`, one per distinct set of policy files. Identical files
in different folders, clones or worktrees share a plan. Changing only a
[setting](policy-files.md#settings) reuses the plan. Pack artifacts are kept
under `packs/` in the default folder, by digest, for every project.

Shield uses a plan only when it was compiled from exactly the current files and
is private to you: on Unix, folders mode `0700` and files `0600` owned by you;
on Windows, private access. A shared, linked, corrupt or mismatched plan is
ignored and compiled again. Native hooks block agents from changing the cache
folder.

Plans are never removed automatically. Delete the folder at any time to
reclaim space.

## Choose another location

For CI, containers or sandboxes, choose the folder when you initialize the
project or set up an agent:

```sh
shield init --cache-dir /absolute/path/shield-cache
shield mcp setup --agent codex --cache-dir /absolute/path/shield-cache
```

`shield mcp setup` needs an absolute path; `shield init` resolves a relative one
against the current folder. The choice is saved in the project's runtime state
for hooks and MCP. Nothing is compiled when you change it; the next request
compiles into the new folder. Shield creates a missing folder privately and
refuses to use a `plans` folder others can access.

If Shield cannot use the folder, requests fail and name `shield doctor`. Fix
the folder's ownership or permissions, or choose another with
`shield mcp setup --cache-dir`.
