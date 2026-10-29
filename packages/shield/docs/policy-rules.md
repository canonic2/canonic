# Writing policy rules

Custom rules set project-specific permissions. Start with one exact command,
then add optional arguments or requirements as needed. Use the
[rule reference](rule-reference.md) for field definitions and
[policy decisions](policy-decisions.md) for precedence.

Read the first two sections for your first rule, or choose a topic:

| You want to… | Read |
| --- | --- |
| Reject an operation when an option is missing | [Commands and requirements](#match-commands-and-enforce-requirements) |
| Accept optional flags, values, or forwarded script arguments | [Text and argument matching](#match-text-and-arguments) |
| Constrain working directories or affected files | [Path restrictions](#restrict-paths) |
| Understand a compound shell request | [Wrappers and shell commands](#understand-wrappers-and-shell-commands) |
| Permit a documentation host or search | [Web rules](#allow-searches-and-documentation-fetches) |
| Publish configurable, reusable rules | [Pack variables](#pack-variables-preconditions-and-templates) and [shared rule bodies](#share-rule-bodies-in-a-pack) |

## Your first rule

Suppose you have reviewed a tool's `verify` operation and want to allow it.
We use the fictional executable `acme-check`; substitute your own reviewed tool.

In an initialized project, run `shield rule add` and choose **Exact command
arguments** (`argv`). Enter ID `verify-project`, executable `acme-check`,
arguments `["verify"]`, and decision Allow. The arguments array excludes the
executable. Review and confirm to activate the rule for the project cwd.

The default **Complete request** form also binds vendor context and environment.
Choose the argument form here so the rule can cover more than one exact request.

For YAML, use this complete document. Its cwd condition also covers project
subdirectories:

```yaml
schema_version: 1
kind: custom
capabilities: [structured-rules-v1, argument-rules-v1, location-v1]
rules:
  - id: verify-project
    decision: allow
    conditions:
      domain: argv
      executable: acme-check
      arguments: {kind: exact, values: [verify]}
      cwd: {within: {root: project}}
```

Merge the rule and capabilities into the file selected by `.shield/config.yml`.
For a new shared file, save it as `.shield/rules.yml` and set `rules: rules.yml`
in the existing configuration. Run `shield policy review`, choose `manual`, and confirm
the review to activate your edit.

`domain: argv` matches an executable and argument list. Here, the executable
must be `acme-check`, the arguments exactly `[verify]`, and cwd inside the project.
`capabilities` declares the syntax used: structured rules, arguments, and locations.

| Submitted command | Does this rule match? |
| --- | --- |
| `acme-check verify` inside the project | Yes; the rule contributes Allow. |
| `acme-check verify --write` | No; the argument list differs. |
| `acme-check report` | No; the operation differs. |
| `acme-check verify` outside the project | No; the cwd condition fails. |

A miss leaves the decision to other rules, or Ask if none matches. Protected-file
checks and other operations in the same shell request still apply. Inspect the
active rule with `shield rule list --json`, or preview a request through
[MCP diagnostics](project-mcp.md#diagnostic-input-and-snapshots) without executing it.

## Choose the right matching style

Use `exact` when only one complete argument list should match. Use `options`
when you have reviewed optional flags or value-taking options. Use `prefix`
only when every remaining argument may be unrestricted. For example, allowing
the prefix `verify` also allows `verify --write`; it does not infer which flags
are read-only.

For example, if you have also reviewed `acme-check verify --verbose`, replace
only the first rule's `arguments` value with this fragment:

```yaml
arguments:
  kind: options
  prefix: [verify]
  flags: [--verbose]
  values: []
  operands: false
```

Now both `acme-check verify` and `acme-check verify --verbose` match. The flag
is optional. `acme-check verify --write` and `acme-check verify report.txt`
still miss because the grammar declares neither that flag nor operands. Keep
the executable and cwd conditions, then review and activate the edit as before.

Unless marked complete, examples below are fragments for the `rules` array.
Add their [required capabilities](rule-reference.md#capabilities) to the document.

## Match commands and enforce requirements

Suppose the fictional `acme-db` tool must use `--read-only`. Matching that flag
in conditions would leave invocations without it to other rules. Put it in
requirements to decide what happens when it is missing.

Merge this complete example into your configured rule file and activate it
with `shield policy review`, as in [your first rule](#your-first-rule):

```yaml
schema_version: 1
kind: custom
capabilities:
  - structured-rules-v1
  - argument-rules-v1
  - requirements-v1
rules:
  - id: database-readonly
    decision: allow
    conditions: {domain: argv, executable: acme-db}
    requirements:
      arguments:
        kind: options
        prefix: []
        flags: [--read-only]
        values: [--user]
        operands: false
        require_all: [--read-only]
    on_unmet:
      decision: deny
      message: Add --read-only and submit a revised request.
      alternatives: [acme-db --read-only]
```

Conditions match every `acme-db` invocation. Requirements demand `--read-only`,
accept an optional `--user` value, and reject operands. Failure returns the
`on_unmet` decision, Deny:

| Request | This rule's result | Why |
| --- | --- | --- |
| `acme-db --read-only` | Allow | The required flag is present. |
| `acme-db --read-only --user analyst` | Allow | The optional value-taking option is declared. |
| `acme-db --user analyst` | Deny | The required flag is absent. |
| `acme-db --read-only --write` | Deny | The requirement does not accept `--write`. |
| `another-db --read-only` | No match | The executable condition does not match. |

Other matching rules and protected-state checks still affect the
[final decision](policy-decisions.md#how-rules-combine). Verify the real tool's
behavior before treating an option as read-only.

### Require a particular option value

A valued option can also require a specific value. This rule allows
`aws logs filter-log-events` with the `acme-read-only` profile and denies the
same operation with any other profile or none:

```yaml
  - id: logs-read-only-profile
    decision: allow
    conditions:
      domain: argv
      executable: aws
      arguments:
        kind: options
        leading: {flags: [], values: [--profile, --region]}
        prefix: [logs, filter-log-events]
        flags: []
        values: [--profile, --region, --log-group-name, --filter-pattern,
                 --start-time, --end-time, --limit, --output, --query]
        operands: false
    requirements:
      arguments:
        kind: options
        leading:
          flags: []
          values: [{name: --profile, match: acme-read-only}, --region]
        prefix: [logs, filter-log-events]
        flags: []
        values: [{name: --profile, match: acme-read-only}, --region,
                 --log-group-name, --filter-pattern, --start-time, --end-time,
                 --limit, --output, --query]
        operands: false
        require_all: [--profile]
    on_unmet:
      decision: deny
      message: Use --profile acme-read-only for log filtering.
```

The conditions declare the operation's grammar without a profile value, so
every request using those options reaches the rule, whether `--profile` comes
before or after the operation. The requirements repeat the grammar and add the
value: the match applies to every `--profile` occurrence in both phases, and
`require_all` counts both phases together. Another profile, a missing profile
or a second, different `--profile` gets the Deny. A request with an option the
grammar does not declare does not match the rule; other rules decide it.
A pack can replace the literal profile with `{var: profile}` so each project
chooses its own, as described in pack variables below.

### Conditions versus requirements

Conditions determine whether a rule applies. Requirements determine its successful
or unmet outcome. `on_unmet.decision` is Ask or Deny, and may carry a short message
and up to three static alternatives. Alternatives are candidates that must be
evaluated again. They cannot rewrite requests, approve execution or bypass another
rule. Put exclusions in requirements when failure should block with an explanation.
For an unmet requirement, feedback offers an `access_path_add` alternative only
when the same rule and operation report an outside-location failure. Unknown
target evidence, excluded paths and unsupported options do not establish that
adding a folder would help.

## Match text and arguments

Use a **text selector** to match one field, such as an executable name or one
argument. Use an **argument selector** to describe the whole list after the
executable. For `acme-check verify --verbose`, the executable is `acme-check`
and the argument list is `["verify", "--verbose"]`.

Text selectors accept a literal string or one of `exact`, `prefix`, `one_of`,
`glob` or `regex`. Regex matches the entire field, including alternatives and
inline flags. Glob matches individual fields; `*` does not cross a path separator,
and an argv glob never expands into more arguments. Each argument stays separate.
Use recursive stars as `**/name`, `dir/**`, `dir/**/name`, or bare `**` for the
whole field. Other adjacent-star placements, including `a**b`, `***` and bare
`**` inside a brace alternative, are refused. Use `[*][*]` for two literal stars.
Brace alternatives may contain character classes, such as `{[ab],c}`; empty
or nested alternatives are refused.
Argument kinds are `any`, `exact`, `prefix`, `sequence`, `options` and restrictive
`risk_tokens`. Options declare arity, leading global options, value predicates,
presence/forbidden checks, repetition and parsing settings explicitly.
With `prefix-leading-v1`, a `prefix` selector can also declare `leading` options.
It consumes those options and their values, matches the operation immediately
after them, and leaves the suffix unconstrained. This lets an administration
restriction cover unknown suffix options without matching words in option values
or later operands.

For reusable allowances, choose a grammar for the operation rather than copying
one observed argv. `exact` matches only the complete literal argument vector.
`prefix` leaves every later argument unrestricted, so it is unsuitable when only
reviewed optional flags should be allowed. `sequence` fixes initial token
selectors in `prefix` and declares a `rest`: `{kind: none}`, `{kind: any}` or
`{kind: each, match: SELECTOR, min: N, max: N}`. Counts default to 0–256.

An `options` grammar fixes the initial operation in `prefix`, declares boolean
options in `flags`, value-taking options in `values`, and positional arguments in
`operands`. Declared options are optional unless required explicitly; unknown
options fail matching. `operands: false` rejects positional arguments;
`operands: true` accepts them without restriction. A bounded operand object
declares `match`, `min` and `max`. The `--` separator ends option parsing, so flags
forwarded through it must be modeled as operands.

### Example: permit a project test script and its watch option

Suppose your project's `package.json` defines `test`, and you have reviewed both
the script and its `--watch` behavior. Inspect any `pretest` and `posttest`
scripts too, because npm can run them as part of the command. Use this rule
fragment to cover the base command and that optional forwarded argument:

```yaml
id: npm-test
decision: allow
conditions:
  domain: argv
  executable: npm
  cwd: {within: {root: project}}
  arguments:
    kind: options
    prefix: [test]
    flags: []
    values: []
    operands:
      match: {exact: --watch}
      min: 0
      max: 1
```

This covers `npm test` and `npm test -- --watch`, including the empty forwarding
form `npm test --`. Other scripts, flags, extra operands and repeated watch
arguments do not match. Review script definitions, lifecycle hooks and argument
effects before proposing; an allowance permits future edits to the script too.
Read [rule suggestions](project-mcp.md#rule-suggestions) for the MCP workflow.

## Restrict paths

Use path predicates when the same command should be treated differently depending
on the files it affects. A command's working directory and its target files are
different checks: running inside a project does not prove that every operand is
inside it.

Choose the check according to what you need to establish:

| Check | Question it answers |
| --- | --- |
| `cwd` | Where does this command start? |
| `targets` | Are the affected paths that Shield can model inside an allowed location? |
| `argument_paths` | Do the explicit path arguments satisfy the declared restrictions? |

For example, a command started inside the project can still name
`/path/to/another-project/file.txt`. A cwd check alone does not constrain that
operand. Path checks depend on the supported operation model described below;
they do not turn an arbitrary executable into sandboxed code.

Locations use `exact`, `within` or `outside` path predicates with `project`,
`absolute`, `read` or `write` roots. The latter two bind the approved root unions
from [project configuration](project-review.md#additional-access-paths) and require
`access-paths-v1`. `outside` means outside the entire union; an exclusion applies
under every referenced root. `within` can include a root-relative text match.

For a command limited to the project root, use
`cwd: {exact: {root: project}}`. To include subdirectories, use
`cwd: {within: {root: project}}`. The value of `exact` is a path-reference object,
not a path string. Project rules use the portable `project` root rather than a
machine's absolute checkout path; changing `exact` to `within` broadens the rule.

An Allow must cover all affected resource actions. Fixed native rm/rmdir/cp/mv
forms and typed file operations account for source and effective destination
paths. Bounded native `cat`, `head`, `tail`, `wc` and nonrecursive `ls` forms model
read operands; unsupported options and helpers retain incomplete coverage. Bare
`ls` reads cwd; `-` is stdin for the other four commands. Unknown options,
unresolved paths and recursive copies cannot establish
positive containment. Observations are fresh per request. Fixed operations use
direct path facts; bounded search metadata checks are described below.
Linux/Windows native acceptance is not established by macOS tests.

### Check paths in command arguments

`argument_paths` checks paths explicitly supplied to a tool without claiming a
complete resource model for everything that tool executes. It requires
`argument-paths-v1`, `argument-rules-v1` and `location-v1`; read/write root unions
also require `access-paths-v1`. For example, in a rule's requirements:

```yaml
argument_paths:
  grammar:
    kind: options
    prefix: [build]
    flags: [--locked]
    values: [--manifest-path, --target-dir]
    operands: false
  options:
    --manifest-path:
      path: {within: {root: read}}
    --target-dir:
      path:
        within: {root: write}
        exclude: [{root: project, path: .shield}]
      recursive: true
```

The grammar must parse the complete invocation. Every occurrence of each selected
option is checked with physical symlink/ancestor resolution; unknown syntax or
unresolved context fails the requirement. `operands: {path: ...}` beside `options`
checks positional operands. Absent options introduce no paths; use `require_all`
in the grammar when one must be supplied. These checks do not constrain implicit
configuration, build scripts, imports or runtime file access. The rule still
needs its own execution decision and `on_unmet` branch.

### Search files with grep and ripgrep

Fixed `grep` forms model a positional pattern followed by explicit regular files.
Fixed `rg` forms search a pattern in explicit files or
directories, the working directory when none is given, or list them with `--files`.
Listed flags plus `-g`, `-t`, `-T`, context, count and depth values are modeled.
Walks follow no descendant links; explicit operands still follow links and receive
containment/exclusion checks. Content searches can honor ignore files and glob
filters. Their footprint conservatively includes existing hidden descendants;
excluded credential paths still block it. Native callbacks also check protected
state, caches and agent settings, using at most 1,024 metadata entries and 32
directory levels. A large or ambiguous footprint requires a narrower directory.
`--no-ignore` without glob filters supplies a visible-only footprint.
A `--files` listing supports `--hidden` and globs and skips the native
content check; declared policy exclusions still apply. Hidden listings retain
the existing-entry footprint even with `--no-ignore`. Stdin, hidden content
searches, follow overrides, pattern files and helpers remain
unverified. `--no-config` is optional. Shield trusts ambient ripgrep configuration
and evaluates the submitted arguments; flags injected by `RIPGREP_CONFIG_PATH`
are outside these checks. Use `--no-config` when config-free execution is desired.

### Read and transform file content

Fixed `sed`, `awk`, `jq`, `sort`, `uniq`, `cut`, `diff` and `cmp` forms model
explicit regular-file inputs, including contained symlink targets. Sed supports
scripts of `p`, `d`, `s`, `y`, `w`, `W`, `r` and `R` commands separated by `;` or
newlines, each with an optional numeric or `$` address or range; `s` accepts the
`g`, `p`, number, `i`/`I`, `m`/`M` and `w` flags. Files a script writes (`w`,
`W`, the `w` flag) are write targets and files it reads (`r`, `R`) are read
targets; a file name runs to the end of its line. In-place editing (`-i`,
`-iSUFFIX`, `--in-place[=SUFFIX]`) makes each input a write target too, plus
its backup when a suffix is given; when either name is a symbolic link, the
write target is the link itself, which sed replaces. Because GNU sed reads
`-i WORD` as a script and BSD sed reads WORD as a backup suffix followed by the
script, such a form counts the targets of both readings and is modeled only
when both are. This includes `-i ''`: GNU sed reads the empty word as an empty
script and the following script word as another file to edit, so
`sed -i '' 's/a/b/' notes.txt` is also read as editing a file named `s/a/b/`.
When the whole command is that one sed invocation, with literal words and no
redirections or variable assignments, it is checked through an agent hook, and
it keeps no backup and writes no `w` file, sed creates nothing it could then
edit, so a named input that does not exist adds no target. A backup or `w` file could create a later
input, and so could anything else the command runs, so in those cases a missing
input leaves the edit unmodeled. `-iSUFFIX` means the same to both. Rules decide these targets like any
other read or write, so an Allow limited to project reads does not cover an
edit. The `e` command and flag, regex addresses, blocks and GNU's `*` backup
suffixes remain unmodeled; awk supports print statements of
numeric fields, optionally with `-F`; jq supports field/index lookups and pipes
through keys, length, type, sort, unique, values and tostring. Their submitted
syntax does not load program files, run helpers or access the environment, and
loads no input other than the named targets.
Sort output files, uniq's second output operand and arbitrary programs
remain unmodeled. Options precede the program and file operands. A `-` operand
reads standard input. Packs decide whether to allow these forms or ask.

### Create directories and files

Fixed `mkdir`/`touch` forms model writes;
`mkdir -p` supports new paths but operands containing `..` remain unverified.
Touch reference/date options and its special `-` operand remain unverified.
For these four models, options precede operands; a leading `--` admits literal
option-like operands. These models supply resource facts; pack rules choose
permissions and supported option subsets.

## Understand wrappers and shell commands

Allowing a command does not automatically allow every wrapper around it. Shield
looks through supported wrappers to evaluate the inner command, but the wrapper
can still require review because it changes how that command runs.

Launch wrappers and child shells are looked through. `env`, `nice`, `nohup`,
`timeout`, `command`, `builtin`, `exec` and `sudo` with recognized options are
unwrapped, and rules decide the command they launch. An inline script run by
`sh`, `bash`, `dash` or `zsh`, by bare name or from `/bin`, with `-c` is read as
shell source and each command in it is decided. A Deny inside the script blocks
the whole request: with a rule that denies `sudo`,
`nohup zsh -lc 'make docs; sudo true'` is denied. The wrapper or
child shell itself still asks, because it changes how the command runs, so a
wrapped request never reaches Allow.

Unknown options leave a child shell's script unread, and an unread script asks.
zsh accepts only `-c`, `-e`, `-u`, `-v`, `-x`, `-l` and `-f`. Its script is read
with POSIX rules only when it contains none of the syntax that zsh handles
differently: `$`, backticks, `*`, `?`, `[`, `]`, `{`, `}`, `(`, `)`, `~`, `^`,
`#`, `!`, `%`, a word starting with `=`, `&>`, `|&`, `&|`, `<<<`, or `>&` and
`<&` followed by anything but a descriptor number or `-`. These characters
count even inside quotes.

Rules also cover `shell`, `file`, `web`, `mcp` and completely bound
`exact_request` inputs. File rules permit SDK reads, for example:

```yaml
id: notes
decision: allow
conditions:
  domain: file
  action: read
  sources:
    all: {exact: {root: project, path: notes.txt}}
```

This rule needs `location-v1` and `structured-rules-v1`. See the
[capability reference](rule-reference.md#capabilities) for other features.
Pack documents also declare identity and release metadata. Their decisions can
be scalar or complete monotonic Loose/Recommended/Strict maps; custom decisions
are scalar.
A pack rule can set `fallback: true` so it applies only when no other pack
decides the request, such as a broad review rule a tool-specific pack replaces.

## Allow searches and documentation fetches

Use web rules to describe the searches or hosts an agent may access. A fetch
allowance can name one documentation host without allowing every URL.

Web rules decide Claude's WebSearch and WebFetch and Codex's searches and
direct HTTP(S) URL opens. An open is a typed fetch with method GET. Each search
or open in a supported Codex batch is evaluated; Deny blocks the whole call,
and Ask follows the configured host approval behavior. Unsupported forms block
before policy evaluation. Initial-URL permission does not cover unobserved redirects.
This pair allows any search and fetches from one documentation host, and needs
`regex-v1` and `requirements-v1`:

```yaml
- id: search
  decision: allow
  conditions: {domain: web, action: search, query: {regex: "(?s).*"}}
- id: docs
  decision: allow
  conditions: {domain: web, action: fetch}
  requirements:
    url: {schemes: [https], hosts: [docs.example.com]}
  on_unmet: {decision: ask}
```

Hosts match exactly; list each subdomain. A search can carry project text in
its query, so allowing every search is a choice to make deliberately.

Fetch rules also decide plain `curl` and `wget` downloads in shell commands.
With the `docs` rule above and a writable project folder,
`curl -fsS -o guide.html https://docs.example.com/guide` is allowed: Shield
checks the fetch against web rules and the output file against file rules.
This covers GET and HEAD requests with one literal URL, inspection options
such as `-fsSL`, timeouts, retries and `-o` for curl, and `-q`, `--spider` and `-O` for
wget. wget must also pass `--no-hsts`, because otherwise it rewrites
`~/.wget-hsts`; without `-O` or `--spider` it is excluded. curl long-option values
must be separate arguments, such as `--output guide.html`; wget also accepts
`--output-document=guide.html`. Uploads, request
bodies, headers, credentials, cookies, proxies, configuration files (including
an existing `~/.curlrc`, `~/.wgetrc`, or `~/.netrc` for wget without
`--no-netrc`) and other options leave the command to
your argv rules, as do explicit argv rules for curl or wget. Fetches carry
`client` (`agent` for WebFetch and Codex opens, `shell` for curl and wget) and,
for shell clients, `redirects` (`follow` or `none`), so a rule can cover only
one of them:

```yaml
- id: docs-downloads
  decision: allow
  conditions:
    domain: web
    action: fetch
    url: {schemes: [https], hosts: [docs.example.com]}
    request_fields: {/client: {exact: shell}}
```

For local smoke tests, restrict shell fetches to loopback hosts and require
redirects to be disabled. This keeps a localhost URL from granting permission
to follow a redirect to another host:

```yaml
- id: local-smoke-tests
  decision: allow
  conditions:
    domain: web
    action: fetch
    url: {schemes: [http, https], hosts: [localhost, "127.0.0.1", "[::1]"]}
    request_fields:
      /client: {exact: shell}
      /redirects: {exact: none}
```

Ordinary curl GET/HEAD requests match this rule. `curl -L` and wget requests
that follow redirects require another matching rule or approval.
Shield's generated localhost suggestion uses this same scope.

## Directory options

Many tools take an option that only says where to work, such as
`npm --prefix DIR`, `git -C DIR` or `make -C DIR`. A pack or your project's
rules can declare such an option under `directory_options`, with the
`directory-options-v1` capability. When no rule decides a command as written,
Shield then matches it as the same command without the option, run from that
folder. With this declaration and rule, `npm start`, `npm --prefix . start` and
`npm --prefix /path/to/project start` from any folder are all allowed, and
`npm --prefix /somewhere/else start` is not:

```yaml
directory_options:
  - {executable: npm, option: --prefix, position: anywhere, paths: follow}
rules:
  - id: npm-start
    decision: allow
    conditions:
      domain: argv
      executable: npm
      arguments: {kind: exact, values: [start]}
      cwd: {within: {root: project}}
```

`paths` is required: `follow` when the tool resolves other relative paths
against the chosen folder, `unchanged` when it does not, as with
`cargo --manifest-path`; then only a folder equal to the current one is
normalized. An option whose value names a file, such as a manifest, sets
`value: file` and lists the default names in `files` (`files: [Cargo.toml]`);
a value naming any other file leaves the command as written. Other fields set
the value forms, position and how repeats combine:

| Field | Values and default |
| --- | --- |
| `forms` | `separate`, `equals`, `attached`; long options default to separate/equals, short options to separate/attached. Attached values require a single-letter short option. |
| `position` | `leading` (default) or `anywhere` before `--`. |
| `value` | `directory` (default) or `file`; file values require `files`. |
| `repeat` | `refuse` (default), `last`, or `chain` to resolve each relative value against the previous one. |
| `paths` | Required: `follow` or `unchanged`. |

A rule written for the command as typed keeps deciding it. Values with
`~`, `$` or backquotes left after shell analysis, missing values and undeclared
forms leave the command as written.

Unknown keys, source nulls, duplicate keys, and unsupported schemas are rejected.
See the [rule reference](rule-reference.md) for fields and capabilities.
Policy comparisons outside the finite literal model report incomplete.

## Pack variables, preconditions and templates

A pack can declare typed `variables` that each project sets for its own
selection, such as a package manager or a cloud profile. A rule or precondition
uses `{var: NAME}` wherever a text selector is accepted, and `when` tests switch
entries on for particular values. Shield fills in values when it prepares the
policy, so hooks evaluate ordinary literal rules. An entry that refers to a
variable without a value is inactive: unset means no check.

`preconditions` gate the whole pack. When a precondition's conditions match and
none of its `requires_any` objects holds, the pack answers with its `on_unmet`
Ask or Deny, even if a more specific pack rule would allow the request. When one
object holds, the pack's ordinary rules decide as usual. A precondition never
allows anything. In packs that declare `templates-v1`, reasons, guidance and
`on_unmet` text can include `{{NAME}}`, which is replaced by the value.

```yaml
capabilities: [structured-rules-v1, argument-rules-v1, variables-v1, preconditions-v1, templates-v1]
variables:
  package_manager:
    type: choice
    description: Package manager agents use
    values: [npm, pnpm, yarn]
    suggest: [{file: pnpm-lock.yaml, value: pnpm}]
preconditions:
  - id: package-manager
    conditions: {domain: argv, executable: {one_of: [npm, pnpm, yarn]}, arguments: {kind: any}}
    requires_any:
      - executable: {var: package_manager}
    on_unmet:
      decision: deny
      message: This project uses {{package_manager}}.
rules:
  - id: npx-with-pnpm
    when: [{var: package_manager, equals: pnpm}]
    conditions: {domain: argv, executable: npx, arguments: {kind: any}}
    decision: deny
```

Variable types are `choice`, `string`, `list`, `boolean` and `path`. Use `choice`
with `values` for a fixed selection, `string` for text, `list` for several text
values, `boolean` for conditional activation, and `path` for a location.
Each declaration includes a `description`; `required` and `default` control
whether the project must supply a value. See
[pack configuration](local-registry.md#pack-variables-and-rule-overrides) for
setting values without editing the pack.

Variables are also the pack's setup flow: `shield pack add` asks one question per
variable in the order the pack declares them. An optional `setup` object words
the question. `question` replaces the description as the question text,
`unset` labels the answer that leaves an optional variable unset, and `value`
and `placeholder` label the typed row of a string, path or free-text list.
`ask_when` takes `when` tests on variables declared earlier and asks only when
they hold:

```yaml
variables:
  profile:
    type: string
    description: Read-only profile agents must use
    layer: local
    setup:
      question: Allow log filtering only with a specific profile?
      unset: Any profile (no check)
      value: Only this profile
      placeholder: acme-read-only
  region:
    type: string
    description: Region agents must use with that profile
    setup:
      unset: Any region
      value: Only this region
      ask_when: [{var: profile, set: true}]
```

A skipped question keeps the variable's current value. A required variable
without a default cannot use `unset` or `ask_when`.

### Optional checks

A `{var}` reference turns its whole entry off while the variable is unset. To
keep the rule and drop only one check instead, an option grammar accepts two
forms:

- `match: {var: NAME, unset: any}` on a valued option accepts any value while
  `NAME` is unset, and only the bound value once it is set.
- An item of `require_all`, `require_any` or `forbid` can be
  `{name: OPTION, when: [TESTS]}`; the option is listed only while the tests hold.

With these, one rule allows `aws logs filter-log-events` with any profile and
region until the project sets them, then requires each value that is set:

```yaml
  - id: logs-filter
    decision: allow
    agents:
      codex:
        prefixes:
          - [aws, logs, filter-log-events]
    conditions:
      domain: argv
      executable: aws
      arguments:
        kind: options
        leading: {flags: [], values: [--profile, --region]}
        prefix: [logs, filter-log-events]
        flags: []
        values: [--profile, --region, --log-group-name, --filter-pattern]
        operands: false
    requirements:
      arguments:
        kind: options
        leading:
          flags: []
          values:
            - {name: --profile, match: {var: profile, unset: any}}
            - {name: --region, match: {var: region, unset: any}}
        prefix: [logs, filter-log-events]
        flags: []
        values:
          - {name: --profile, match: {var: profile, unset: any}}
          - {name: --region, match: {var: region, unset: any}}
          - --log-group-name
          - --filter-pattern
        operands: false
        require_all:
          - {name: --profile, when: [{var: profile, set: true}]}
          - {name: --region, when: [{var: region, set: true}]}
    on_unmet:
      decision: deny
```

The rule has no `message`, because a template naming an unset variable would
turn the rule off. Agents are told what failed instead, for example
`--profile must be acme-read-only.` or `Missing required option: --profile.`

Shield checks a pack's rules with every conditional item listed, whatever its
`when` tests. An option that one item requires and another forbids is refused
even when their tests can never hold together.

### Codex prefix rules

`agents.codex.prefixes`, which needs the `agent-outputs-v1` capability, lists
the command prefixes Codex may run outside its sandbox. That matters for
commands such as `aws` that need the network. While the rule is active and
allows, Shield writes each prefix to `.codex/rules/shield.rules`. A position can
list alternatives:

```yaml
    agents:
      codex:
        prefixes:
          - [aws, logs, filter-log-events]
          - [aws, logs, [tail, get-log-events]]
```

Each prefix must start with the rule's own executable and argument prefix, and
may add tokens after it. A rule matching `aws logs` can list
`[aws, logs, tail]` but not `[aws]` or `[aws, s3]`. The rule's executable and
argument prefix must be literals or a `one_of` of literals.

Shield's hooks still decide every command. When policy asks about a command
whose every part matches one of these prefixes, such as
`aws logs tail --follow`, the hook blocks it even under the Allow fallback,
because Codex would otherwise run it without asking. A command behind a wrapper
such as `env` or inside `bash -c` keeps Codex's sandbox and follows the
fallback. Turning the rule off, changing its
decision or failing its `when` removes the prefix at the next activation.
See [project setup](claude-project.md) for when the file is written.
`shield rule list --json` reports, for each entry that depends on variables or is
overridden, the values it used and the resolved rule, or why it is inactive.
Project custom rules do not accept variables, `when` or templates.

## Share rule bodies in a pack

Packs can share one rule body across commands. Project custom rules use
independent rules.

Declare `invocations` and `command_sets` on the pack, then reference them from
a rule. They require `invocation-groups-v1` and `command-sets-v1` respectively.
This pack fragment shares an inspection body between two commands and two
leading argument forms:

```yaml
invocations:
  globals: {id_prefix: v, segments: [[[], [--plain]]]}
command_sets:
  inspection: {status: base-status, diff: base-diff}
rules:
  - id: inspection
    invocation: globals
    command_set: inspection
    conditions: {domain: argv, executable: acme, arguments: {kind: any}}
    decision: allow
```

The `acme --plain status` match has ID `v1-base-status`. Requirements inspect the
complete original argv. Groups declare finite exact vectors; they do not guess
global options or strip unknown arguments. Each command set belongs to one rule
body. `shield rule list --json` includes shared definitions alongside canonical bodies;
an exact matched `rule_ref` returns its complete argument condition. Human pack
summaries count stored bodies, invocation groups and command members separately.

## Next steps

Use the [rule reference](rule-reference.md) for field lookup,
[pack validation](pack-validation.md) to check candidate documents, and
[Project MCP](project-mcp.md#rule-suggestions) to prepare a change with an agent.
If a rule behaves unexpectedly, inspect the actual request with the
[troubleshooting guide](troubleshooting.md#the-request-was-allowed-or-blocked-unexpectedly).
