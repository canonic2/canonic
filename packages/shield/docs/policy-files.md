# Policy files and settings

Shield enforces exactly what your project's policy files say. There is no
saved copy to activate or repair: the next request after a file changes is
decided by the changed file.

## The files

The policy files live together in one folder, the authoring bundle: `.shield/`
by default. The table uses that location.

| File | Committed | Holds |
| --- | --- | --- |
| `.shield/config.yml` | Yes | Selected packs and profiles, pack settings, connections, shared settings |
| `.shield/rules.yml` | Yes | Your project's custom rules |
| `.shield/shield.lock` | Yes | The exact pack releases the configuration selects |
| `.shield/config.local.yml` | Ignored by default | Your own settings and pack values, layered over `config.yml` |
| `.shield/rules.local.yml` | No | Your own custom rules |
| `.shield/shield.local.lock` | No | Releases your local layer selects |
| `.shield/.gitignore` | Yes | Keeps the local files and `state/` out of Git |
| `.shield/state/` | No | Runtime state: pending requests, grants, history and proposals |

When `shield init` or first-run `shield setup` creates a policy bundle, it adds
`config.local.yml` to that bundle's `.gitignore`. Remove that entry if you want
to track the file. Later Shield operations preserve that choice. A copied, moved
or cloned folder gets runtime state on first use.

## Where the configuration lives

Shield uses the first of these that applies:

1. The file you pass with `--config PATH`.
2. `.shield/config.yml`.
3. `.config/shield/config.yml`, for projects that group tool settings under
   `.config/`.

The two default folders are never combined. When `.shield/config.yml` exists,
Shield uses it even if it is invalid, and reports the problem rather than
falling back to `.config/shield/`. A `--config` file that is missing or
unreadable is refused; Shield does not use a default instead.

The rules, locks and local files sit beside the selected `config.yml`, and
paths inside the configuration are relative to that folder. With the grouped
layout the bundle is `.config/shield/`, which gets its own `.gitignore` for the
local files:

```text
.config/shield/
  config.yml
  rules.yml
  shield.lock
  config.local.yml   # ignored by default
  .gitignore
.shield/
  .gitignore
  state/             # runtime state, always here
```

Runtime state stays in `.shield/state/` whichever configuration is selected.
`shield status` shows the selected file and any default file it is not using:

```text
Configuration: .config/shield/config.yml
```

`--config` works with every command, including `shield init`, which creates
the file there. A relative path is resolved from the folder you run the command
in. Without `--project`, the project is the nearest folder above you that
holds the `--config` file or Shield's own files, so a checkout whose only
configuration is that file works from its root or any subfolder. When you set agents up with `--config`, Shield adds the same file, relative
to the project, to the hooks and MCP entries it writes, so agents enforce the
same policy. That file must be inside the project. The Box integration uses a
default location; use `.config/shield/` there instead of `--config`.

Folders that earlier releases set up keep their state in `.shield/review/`;
Shield renames it to `.shield/state/` the first time it opens the folder, and
adds `state/` to `.shield/.gitignore`.

## Change the policy

Change policy with the commands, which show the change, ask you to confirm it
once in the terminal and write the files:

```sh
shield pack add acme/node
shield rule add
shield policy review
```

`shield policy review` also reviews an agent's pending proposal. An agent cannot
confirm its own change: commands that change policy refuse `--json` and
non-interactive use, and native hooks block agents from writing `.shield/`.

You can also edit the files by hand. Shield reads them on the next request; an
edit that makes them invalid prevents policy from authorizing requests, and
`shield status` names the problem. Supported native requests can still use an
exact one-time review through `request_approve`; fixed protected paths stay
closed. For Git conflict recovery, see
[allowing one blocked request](project-review.md#allowing-one-blocked-request).
A command writes on top of your edit rather
than discarding it. If the files change between showing a change and your
confirmation, Shield refuses and asks you to run the command again.

`shield status` and `shield doctor` report missing locked packs without
fetching them. Run `shield pack restore` or call MCP `pack_restore` to fetch
the exact locked releases from their registered sources. Shield verifies each
artifact against its lock digest before storing it on this machine.

## Settings

Settings are keys in `config.yml` or `config.local.yml`. A key set in the local
file wins over the shared one, setting by setting, except `registries`, which
the local file replaces whole. Commands that change a setting write it to
`config.local.yml`; commit a value in `config.yml` to share it.

```yaml
# .shield/config.local.yml
enforcement: enabled
protection:
  agent_settings_read: ask
  agent_settings_change: block
ask_fallback:
  codex: block
history:
  enabled: true
  limit: 5000
```

| Key | Values | Default | Changed by |
| --- | --- | --- | --- |
| `enforcement` | `enabled`, `paused` | `enabled` | `shield enable`, `shield disable` |
| `protection.agent_settings_read` | `ask`, `allow` | `ask` | `shield protection` |
| `protection.agent_settings_change` | `block`, `allow` | `block` | `shield protection` |
| `ask_fallback.<agent>` | `block`, `allow`: what a host that cannot ask does with a request policy asks about | `block` | `shield mcp setup` |
| `history.enabled` | `true`, `false` | `true` | `shield policy review` |
| `history.limit` | 1 to 10000 entries | 5000 | `shield policy review` |
| `registries` | `sources` by alias and an optional `default` alias | None | `shield registry add`, `remove` and `default` |

Settings do not change what a rule decides. An invalid setting does not pause
Shield: native requests ask until it is fixed.

## Compiled plans

Shield compiles the files into a plan stored in your cache folder and reuses it
until a file changes. Deleting the cache changes nothing; the next request
compiles again. See [compiled policy cache](compiled-cache.md).

## Updating Shield

Shield converts older saved state the first time a command, MCP connection or
hook opens the project, writes any settings an earlier version held into
`config.local.yml`, and keeps a private recovery copy. Existing approvals keep
their scope, expiry and single use. `shield status` reports the conversion for
seven days.

For recovery by symptom, use [troubleshooting](troubleshooting.md).
