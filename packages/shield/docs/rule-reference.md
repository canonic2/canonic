# Rule reference

This reference covers document fields, rule fields, and selectors. For your
first rule, start with the [rule-writing guide](policy-rules.md).

| You need to look up… | Section |
| --- | --- |
| The surrounding YAML/JSON document | [Document format](#document-format) |
| A rule's decision, conditions, or unmet branch | [Rule fields](#rule-fields) |
| Exact text, alternatives, patterns | [Text selectors](#text-selectors) |
| Complete argument lists and optional flags | [Argument selectors](#argument-selectors) |
| Working directories and affected paths | [Locations and domains](#locations-and-domains) |
| Which syntax features to declare | [Capabilities](#capabilities) |

## Document format

Shield accepts UTF-8 JSON or a single YAML document using `schema_version: 1`.
Keys must be strings. Unknown fields, duplicate keys, unsupported capabilities,
and YAML anchors and aliases are rejected. Omit optional source fields instead
of setting them to null; exact-request payloads can retain their supplied null
values. Documents support no environment interpolation, command substitution,
or includes.

| Field | Project custom rules | Permission pack |
| --- | --- | --- |
| `schema_version` | Required integer `1`. | Required integer `1`. |
| `kind` | Required `custom`. | Required `pack`. |
| `capabilities` | Required list of exactly the capabilities used. | Same. |
| `rules` | Required array with unique rule IDs; may be empty. | Same. |
| `directory_options` | Optional directory option declarations. | Same. |
| `id`, `version` | Not accepted. | Required publisher/name and semantic release version. |
| `description`, `license`, `maintainers` | Not accepted. | Required nonempty description, license text, and nonempty maintainer list. |
| `engines` | Not accepted. | Required object containing only `shield`, with its supported semantic-version requirement. |
| `dependencies` | Not accepted. | Optional dependency declarations; see [packs](local-registry.md#pack-dependencies). |
| `variables`, `preconditions` | Not accepted. | Optional declarations; see [pack variables](policy-rules.md#pack-variables-preconditions-and-templates). |
| `invocations`, `command_sets` | Not accepted. | Optional shared bodies; see [rule reuse](policy-rules.md#share-rule-bodies-in-a-pack). |

Project configuration selects the rule file. Documents cannot select a project,
operator identity, or cache root. Activate file edits with `shield policy review`.

## Rule fields

| Field | Meaning |
| --- | --- |
| `id` | Required stable identifier, unique within the document: 1–128 ASCII bytes, beginning with a letter or digit, then letters, digits, `.`, `_`, `:`, or `-`. |
| `decision` | Required `allow`, `ask`, or `deny`. Pack rules may instead map all three profiles: `loose`, `recommended`, `strict`. |
| `conditions` | Required object with a `domain` and the predicates that make this rule apply. |
| `requirements` | Optional nonempty object of additional predicates, all of which must be verified for the successful decision. |
| `on_unmet` | Required exactly when `requirements` is present; an `ask` or `deny` decision with optional `message` and `alternatives`. |
| `reason` | Optional nonempty explanation, at most 4,096 UTF-8 bytes. |
| `guidance` | Optional successful-branch `message` and/or `alternatives`. |
| `expires_at` | Optional quoted UTC timestamp for custom rules; an expired rule contributes nothing. |
| `fallback` | Pack-only `true`; lets this rule yield to regular decisions from other packs. |
| `invocation`, `command_set` | Pack-only references to shared definitions; `command_set` requires `invocation`. |
| `when` | Pack-only activation tests on declared variables. |
| `agents` | Agent-specific outputs; see [Codex prefix rules](policy-rules.md#codex-prefix-rules). |

Guidance messages are at most 512 UTF-8 bytes. Alternatives contain one to three
nonempty strings, each at most 256 bytes. They suggest new requests to evaluate;
they do not authorize or rewrite the current request.

Rule order does not determine precedence. See
[how rules combine](policy-decisions.md#how-rules-combine).

## Text selectors

| Shape | Matches |
| --- | --- |
| `verify` or `{exact: verify}` | The complete literal field. |
| `{prefix: verify}` | Text starting with `verify`. |
| `{one_of: [verify, report]}` | One of the listed literal values. |
| `{glob: '*.txt'}` | A glob against one field; `*` does not cross a path separator. |
| `{regex: 'get|describe'}` | A regex against the entire field, not a substring search. |
| `{not: {one_of: [prod, admin]}}` | A present value the inner selector does not match. |

Selectors do not expand an argument into additional arguments. The shell command
`acme-check "two words"` has one argument containing a space, not two arguments.
Glob and regex selectors require their respective capabilities; `not` requires
`negated-selectors-v1`. A `not` selector never matches a missing value, so pair
it with `require_all` when the option must be present.

## Argument selectors

The `arguments` condition describes the arguments after the executable.

| `kind` | Fields | Use |
| --- | --- | --- |
| `any` | No additional fields. | Accept any argument list. |
| `exact` | `values`: literal string array. | Match the complete argument list, including an empty list. |
| `prefix` | `values`: selector array; optional `leading` grammar. | Match the start and leave the suffix unrestricted. Leading options require `prefix-leading-v1`. |
| `sequence` | `prefix`: selector array; `rest`: `none`, `any`, or `each`. | Match initial tokens and constrain the rest. |
| `options` | `prefix`, `flags`, `values`, `operands`; optional parsing controls. | Accept a declared operation and reviewed options. |
| `risk_tokens` | `prefix`, `any_of`. | Detect restrictions conservatively; cannot grant Allow. |

For a sequence, `rest: {kind: none}` rejects remaining arguments;
`rest: {kind: any}` accepts them. Use
`rest: {kind: each, match: SELECTOR, min: N, max: N}` to constrain each remaining
argument. Counts default to 0–256.

An options grammar distinguishes flags from options that consume a value:

```yaml
kind: options
prefix: [verify]
flags: [--verbose]
values: [--format]
operands: false
```

This fragment accepts `verify`, `verify --verbose`, and `verify --format json`.
It rejects `verify --format` and `verify report.txt`. It does not restrict the
format value to JSON; use `{name: --format, match: json}` in `values` for that.

Declared options are optional unless required. Unknown options fail matching.
Use `require_all`, `require_any`, and `forbid` to check declared option names.
`leading: {flags: [...], values: [...]}` declares global options before the
operation. `operands` is `false`, `true`, or `{match: SELECTOR, min: N, max: N}`.
The `--` separator ends option parsing; subsequent tokens are operands.

Some tools read global options anywhere, among options a rule cannot list.
`unknown_options: skip` passes over undeclared options so a grammar can still
check the declared ones; it needs `operands: true`. `long_abbreviations: true`
accepts any unique prefix of a declared long option, as argparse-based tools
do, so declare the tool's complete global option set when you use it. This
requirement accepts any AWS command that names the `acme-ro` profile, written
in full or abbreviated, anywhere in the command:

```yaml
kind: options
prefix: []
flags: [--debug, --no-paginate, --no-verify-ssl, --no-sign-request]
values: [{name: --profile, match: acme-ro}, --region, --output, --query, --endpoint-url]
operands: true
unknown_options: skip
long_abbreviations: true
require_all: [--profile]
```

It allows `aws logs tail g --since 1h --prof acme-ro` and refuses
`aws s3 ls --profile acme-admin`. The settings require `unknown-options-v1` and
`option-abbreviations-v1`. Skipping says nothing about what the skipped options
do, so allowances for specific operations keep their declared grammars.

With abbreviations disabled, skip mode refuses prefixes of declared long names
such as `--prof` or `--prof=value` for `--profile`. Unrelated undeclared options
are still skipped.

Skip mode also refuses short-option tokens containing a declared option behind
an undeclared letter. For example, a requirement on `-X` refuses `-kXPOST` when
`-k` is undeclared, since skipping it could hide an override of `-X`. Declare
`-k` as a flag to parse that cluster normally. A declared valued option still
consumes its attached suffix as a value.

For a worked example of forwarding `--watch` to a project script, see
[argument matching](policy-rules.md#match-text-and-arguments). Use requirements
with an `on_unmet` branch when an option violation should contribute a rejection
instead of making the entire rule miss.

## Locations and domains

An `argv` rule can match the executable, arguments, and cwd. File rules match
typed file operations. Shell rules match shell input; web rules match searches
and fetches; MCP rules match MCP requests. `exact_request` binds the complete
normalized input and context rather than just the command arguments. The guided
CLI defaults to that exact-request form; explicitly choosing `argv` broadens the
vendor/environment binding.

Path predicates use `exact`, `within`, or `outside`. References use `project`,
`absolute`, `read`, or `write` roots. Read/write roots require
`access-paths-v1`; their configured directories are described in
[additional access paths](project-review.md#additional-access-paths).

Use a cwd predicate to constrain where a command starts. Use target predicates
to constrain its observed file effects. Use `argument_paths` to check explicit
path operands without claiming a complete model of the tool's effects.
See [path restrictions and supported operations](policy-rules.md#restrict-paths)
and [web rules](policy-rules.md#allow-searches-and-documentation-fetches).

## Capabilities

Declare exactly the capabilities used anywhere in the document, once each.

| Capability | Required for |
| --- | --- |
| `structured-rules-v1` | Every document. |
| `argument-rules-v1` | Argument selectors in argv rules. |
| `argument-paths-v1` | Explicit argument-path checks. |
| `glob-v1`, `regex-v1` | Their respective text selectors. |
| `location-v1` | Path references/predicates, cwd, file domains, or targets. |
| `access-paths-v1` | `read` or `write` root unions. |
| `requirements-v1` | Requirements and their unmet branches. |
| `possible-effects-v1` | Possible-effect predicates. |
| `fallback-rules-v1` | Pack fallback rules. |
| `dependencies-v1` | A dependencies array, including an empty one. |
| `invocation-groups-v1`, `command-sets-v1` | References to their respective shared pack definitions. |
| `prefix-leading-v1` | Leading options on a prefix argument selector. |
| `variables-v1` | Pack variables, variable selectors, or activation tests. |
| `preconditions-v1` | Pack preconditions. |
| `directory-options-v1` | Directory option declarations. |
| `templates-v1` | Variable templates in pack explanation text. |
| `agent-outputs-v1` | Agent-specific rule outputs. |
| `mcp-integrations-v1` | Pack MCP integration definitions and defaults. |
| `mcp-connection-facts-v1` | Typed `connection` predicates over reviewed MCP facts. |
| `mcp-effective-fields-v1` | Pack mappings from request fields and reviewed defaults to effective facts. |
| `mcp-command-fields-v1` | Declared MCP fields evaluated as literal fixed-executable commands. |
| `unknown-options-v1` | `unknown_options: skip` in an options grammar. |
| `option-abbreviations-v1` | `long_abbreviations: true` in an option phase. |
| `negated-selectors-v1` | `not` text selectors. |

An argv rule with an argument selector needs `structured-rules-v1` and
`argument-rules-v1`. Add `location-v1` for a cwd predicate and `requirements-v1`
for requirements and their unmet branch. Plain string selectors need neither
`regex-v1` nor `glob-v1`.

Use [pack validation](pack-validation.md) to check documents without activating
them. Validation checks syntax and policy structure; review the tool's behavior
before granting permission.
