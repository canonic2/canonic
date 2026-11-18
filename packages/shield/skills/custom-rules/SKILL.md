---
name: custom-rules
description: Write a project custom rule when Shield keeps asking or denying work that will come up again, or when the user wants something allowed, blocked or always asked about; propose it, let the user approve it in the form, and check the result.
---

# Write a custom rule

Use when a request was asked or denied and the same kind of request will come
up again in this task or in later sessions, or when the user asks to allow,
block or always ask about something. Propose the rule as soon as you need it;
do not wait until the task ends. Nothing changes until the user accepts the
`policy_apply` form, so proposing is safe.

Use something else for narrower needs:

- One refused request, once: `request_approve`.
- A batch of commands for this task only: `session_grant_propose`.
- A tool an available pack covers: `pack_search`, `pack_info`, then `pack_add`.
  To change how an installed pack decides, use `pack_configure`.

## 1. Check the current decision

Call `policy_evaluate` with the exact request as it was submitted, and
`rule_list` to find an existing custom rule to edit instead of adding another.
Read what the command runs (scripts, Make targets, package scripts and the
hooks they trigger) before allowing it. An allowance also covers later edits
to those scripts.

## 2. Choose the decision

| Decision | Use for |
| --- | --- |
| `allow` | Everyday work whose effects you checked. |
| `ask` | Work that is fine only sometimes. |
| `deny` | Work that must not happen in this project. Give a `guidance` message with the alternative. |

Custom rules decide before packs, so a custom `allow` can override a pack's
`deny`. Say so in `agent_reason` when that is the intent. Within custom rules,
the more specific match wins; on a tie, `deny` beats `ask` beats `allow`.

## 3. Write the rule

Every rule needs a stable descriptive `id`, a `decision` and `conditions`
with a `domain`; add a short `reason` the user will read in the form. Shield
works out the document's capabilities itself.

- `argv`: an executable and its arguments. Use it for commands.
- `file`: reads and writes the agent makes with its own file tools.
- `web`: searches and fetches, including plain `curl` and `wget` downloads.
- `mcp`: calls to another MCP server's tools.

Keep the scope as small as the need:

- Match arguments with `exact` for one command, or `options` listing only
  the flags and values you checked. Never use `prefix` for a script runner
  (`make`, `npm run`, `cargo`, `pnpm`): a prefix allows any extra arguments.
- Limit the working folder with `cwd`: `{"within":{"root":"project"}}` for
  the project and its subfolders, `{"exact":{"root":"project"}}` for the root.
- Name no absolute or machine-specific paths. Use `project`, `read` or
  `write` roots; Shield refuses rules that name this machine's folders.
- Write the command the way agents run it. A rule for `make -C packages/api
  test` matches those exact arguments from the project root.
- Add `expires_at` with a quoted UTC time, such as `2026-12-31T00:00:00Z`,
  for a temporary need.
- Pass `"layer":"local"` for a personal preference that should not be shared.

When Shield returns a fetch for domain review, open `policy_apply` confirmation
before retrying; do not wait for recurrence or preliminary chat approval. Inspect
the project's instructions, service routing and existing web allowances; edit
an existing allowance when appropriate. Use the site's domain and subdomains
across paths, query strings and ports. Shield's draft uses the Public Suffix
List and groups localhost service names. Keep its redirect boundary. Never add
one rule per endpoint, command spelling or temporary hostname. A draft grants
nothing until confirmed. If the user declined the unchanged saved allowance,
do not offer it again; request exact one-time approval only for work still needed.
Include the hook's `review_ids` unchanged when preparing its domain proposal.
Shield tracks pending and offered reviews durably. Reuse a pending proposal;
do not create another automatic save offer for a site already reviewed in this session.

An exact command in the project:

```json
{"rule":{"id":"api-make-test","decision":"allow",
  "reason":"Run the API package tests.",
  "conditions":{"domain":"argv","executable":"make",
    "arguments":{"kind":"exact","values":["-C","packages/api","test"]},
    "cwd":{"exact":{"root":"project"}}}}}
```

A command with reviewed optional flags and values:

```json
{"rule":{"id":"acme-check-verify","decision":"allow",
  "reason":"Run acme-check verify with its reporting options.",
  "conditions":{"domain":"argv","executable":"acme-check",
    "arguments":{"kind":"options","prefix":["verify"],
      "flags":["--verbose"],"values":["--format"],"operands":false},
    "cwd":{"within":{"root":"project"}}}}}
```

A required option. Requirements decide the result once the conditions match;
without the flag, this rule denies and says how to fix the request:

```json
{"rule":{"id":"acme-db-read-only","decision":"allow",
  "reason":"Allow read-only database sessions.",
  "conditions":{"domain":"argv","executable":"acme-db"},
  "requirements":{"arguments":{"kind":"options","prefix":[],
    "flags":["--read-only"],"values":["--user"],"operands":false,
    "require_all":["--read-only"]}},
  "on_unmet":{"decision":"deny",
    "message":"Add --read-only and submit a revised request."}}}
```

A command that must never run:

```json
{"rule":{"id":"no-acme-deploy","decision":"deny",
  "reason":"Deploys run from CI only.",
  "conditions":{"domain":"argv","executable":"acme-deploy",
    "arguments":{"kind":"any"}},
  "guidance":{"message":"Deploys run from CI; push a branch instead."}}}
```

Reading one project file with the agent's file tools:

```json
{"rule":{"id":"read-release-notes","decision":"allow",
  "reason":"Read the release notes.",
  "conditions":{"domain":"file","action":"read",
    "sources":{"all":{"exact":{"root":"project","path":"NOTES.md"}}}}}}
```

Fetching pages from one documentation host. List each subdomain:

```json
{"rule":{"id":"fetch-docs-example","decision":"allow",
  "reason":"Fetch pages from docs.example.com.",
  "conditions":{"domain":"web","action":"fetch",
    "url":{"schemes":["https"],"hosts":["docs.example.com"]}}}}
```

## 4. Propose and apply

1. Call `rule_add` with the rule (or `rule_edit` to replace one) and an
   `agent_reason` naming what you checked. For several rules, call
   `policy_propose` with up to 16 `rule_add` items in `changes`.
2. Check the proposal: `policy_evaluate` the intended request and one near
   miss, such as an extra flag or another folder, passing the proposal's
   `after_candidate_id` as `candidate_id`. If the scope is wrong, close it
   with `policy_withdraw` and propose a corrected rule.
3. Copy `apply_arguments` unchanged to `policy_apply`. Its form is the
   approval; do not also ask in chat.
4. Retry the original request once it is allowed.

## 5. Outcomes

- Validation error: correct the rule as the message says and propose again.
- Declined: continue without the rule. Do not propose it again or a broader
  one unless the user asks.
- `stale_revision`: the policy changed; check it again and prepare a new
  proposal.
- Abandoned proposal: close it with `policy_withdraw`.

## More detail

These bundled guides cover cases the examples above do not; read them with
`docs_read` when you need them:

- `policy-rules.md`: matching styles, paths and files, wrappers and shell
  commands, web rules and directory options.
- `rule-reference.md`: every rule field, text and argument selector, and
  location predicate.
- `policy-decisions.md`: how custom rules, packs and protection combine.
