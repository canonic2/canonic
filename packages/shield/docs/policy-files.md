# Policy files and settings

Shield enforces exactly what your project's `.shield/` files say. There is no
saved copy to activate or repair: the next request after a file changes is
decided by the changed file.

## The files

| File | Committed | Holds |
| --- | --- | --- |
| `.shield/config.yml` | Yes | Selected packs and profiles, pack settings, connections, shared settings |
| `.shield/rules.yml` | Yes | Your project's custom rules |
| `.shield/shield.lock` | Yes | The exact pack releases the configuration selects |
| `.shield/config.local.yml` | No | Your own settings and pack values, layered over `config.yml` |
| `.shield/rules.local.yml` | No | Your own custom rules |
| `.shield/shield.local.lock` | No | Releases your local layer selects |
| `.shield/.gitignore` | Yes | Keeps the local files and `review/` out of Git |
| `.shield/review/` | No | Runtime state: pending requests, grants, history and proposals |

`shield init` writes these files. A copied, moved or cloned folder needs no
follow-up command: its files are its policy, and a folder without runtime state
gets it on first use.

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
edit that makes them invalid makes every request ask until it is fixed, and
`shield status` names the problem. A command writes on top of your edit rather
than discarding it. If the files change between showing a change and your
confirmation, Shield refuses and asks you to run the command again.

Pack artifacts are fetched by digest from `shield.lock`. When a release is
missing on this machine, `shield status` fetches it.

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
