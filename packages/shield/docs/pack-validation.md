# Validating permission packs

Validate a pack's document, then test its decisions with example requests.
Neither step executes commands or activates policy.

You need `shield` on your path and a directory for two files. No project
initialization or installation of the fictional `acme-check` tool is needed.

## Prepare a small pack

Save this complete document as `pack.yml` in your example directory:

```yaml
schema_version: 1
kind: pack
id: acme/inspect
version: 1.0.0
description: Permit a reviewed inspection command
license: MIT
maintainers: [Acme]
engines: {shield: '^0.8'}
capabilities: [structured-rules-v1, argument-rules-v1]
rules:
  - id: verify
    decision: allow
    conditions:
      domain: argv
      executable: acme-check
      arguments: {kind: exact, values: [verify]}
  - id: publish
    decision: deny
    conditions:
      domain: argv
      executable: acme-check
      arguments: {kind: exact, values: [publish]}
```

This pack allows `acme-check verify`, denies `acme-check publish`, and returns
Ask for other invocations. Scalar decisions apply to all profiles. `engines`
declares Shield 0.8 compatibility; use the range your own pack supports.

## Check a document

From the directory containing `pack.yml`, run:

```sh
shield __internal validate --pack pack.yml
```

A valid document produces JSON and exits 0; invalid input exits 2. You can also
check a project custom-rule file with `shield __internal validate --rules PATH`.

## Test expected decisions

Save this complete fixture document as `cases.json` beside `pack.yml`:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "verify-is-allowed",
      "profile": "recommended",
      "argv": ["acme-check", "verify"],
      "expected": "allow"
    },
    {
      "name": "extra-option-does-not-match",
      "profile": "recommended",
      "argv": ["acme-check", "verify", "--write"],
      "expected": "ask"
    },
    {
      "name": "publish-is-denied",
      "profile": "recommended",
      "argv": ["acme-check", "publish"],
      "expected": "deny"
    }
  ]
}
```

Run the fixture runner from the same directory:

```sh
shield __internal test-pack --pack pack.yml --cases cases.json
```

The JSON result should have `passed: true` and each case's `expected`, `actual`,
and `passed` values. Exit status is 0 for a match, 1 for a mismatch, or 2 for
malformed input.

The second case checks that an extra option misses the exact allowance. Change
its expectation to `allow` to see a failure, then restore `ask`. Add allowed,
denied, and near-miss cases for your own pack.

### Validate your own artifacts

`validate` accepts repeated `--pack` paths and an optional `--rules` document.
It checks each document's structure, capabilities, compatibility, and profile
ordering. It does not check the complete dependency set, cross-document
compatibility, or required variable bindings for activation.

`test-pack` evaluates each fixture under its selected profile. Add
`--with-pack PATH` for companion packs under the same profile, up to 64 distinct
pack identities. `variables` binds the primary pack; companions use defaults.

## Set the fixture context

Fixtures use the production evaluator and fresh filesystem observations. Cwd
and the default project root are the directory where you run the command;
additional access paths are not configured. Add `--details` for matching rules,
requirement checks, branch outcomes, and guidance.

Normalized shell fixtures test compound commands without enrolling a project.
They do not test native hooks, transport replies, or operator activation.
Those checks, including extra-root behavior, need isolated enrolled projects.

### Fixture fields

Use `schema_version: 1` and a nonempty `cases` array with at most 1,024 entries.
Each case has the following fields:

| Field | Meaning |
| --- | --- |
| `name` | Required nonempty case name, unique in this file. |
| `profile` | Required `loose`, `recommended`, or `strict`. |
| `argv` | Nonempty array containing the executable followed by its arguments. Use exactly one of `argv` or `input`. |
| `input` | A normalized diagnostic request, such as the shell example below. Use exactly one of `argv` or `input`. |
| `expected` | Required `allow`, `ask`, or `deny`. |
| `variables` | Optional map binding the primary pack's variable values for this case. |

Omitted variable bindings use declared defaults and are
otherwise unset. Required variables without a binding or default refuse evaluator
preparation.

### Test a shell request

`input` uses the normalized diagnostic schema described in
[project MCP](project-mcp.md#diagnostic-input-and-snapshots), with an empty known
environment and the fixture working directory as context:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "inspection-pipeline",
      "profile": "recommended",
      "input": {
        "kind": "shell",
        "source": "git status | head -20",
        "dialect": "posix",
        "dialect_version": null,
        "original_input": {}
      },
      "expected": "allow"
    }
  ]
}
```

This example needs a pack allowing the complete pipeline; `acme/inspect` above
does not. Include changed options and paths that should Ask or Deny as well.

## Document requirements

Policy documents use `schema_version: 1`, are bounded to 256 MiB, and reject
duplicate or unknown keys. Fixture documents use the same 256 MiB reader and shared parser budgets. Packs declare
the capabilities their rules use, including structured rules, argument selectors,
patterns, locations and requirements. Custom rules belong to the project.
See the [rule guide](policy-rules.md) for authoring, the
[rule reference](rule-reference.md) for fields and capabilities, and
[registry workflows](local-registry.md) for acquisition.

Installed-policy diagnostics use `policy_evaluate` and `policy_compare` through
[project MCP](project-mcp.md). The [project policy guide](project-review.md) covers
confirmed installation, custom rules, approvals and request history.
