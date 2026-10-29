# Bounded native execution

`shield __internal run` launches a small set of macOS commands under active
policy and records their outcomes. For ordinary Claude or Codex sessions, use
[native project setup](claude-project.md).

The runner uses literal argv, an explicit canonical cwd, and inherited standard
streams. It adds no shell or PATH lookup. Leaf commands receive only `LC_ALL=C`;
Git uses the settings below. Start with the supported commands, then
[run an example](#run-with-active-policy).

## Supported contract

| Executable | Arguments |
| --- | --- |
| `/bin/echo` | Literal arguments, interpreted by the native echo command |
| `/usr/bin/true` | None |
| `/usr/bin/false` | None |
| `/bin/sleep` | One finite nonnegative decimal number, at most 86400 seconds |
| `/Library/Developer/CommandLineTools/usr/bin/git` | `status`, `diff`, `diff --cached`, or `log -n N` with decimal N from 1 to 100 |

Other executables, scripts, wrappers, PATH names, and argument forms block even
if policy allows them. Linux and Windows return unsupported without launching.
The runner neither enables SDK Bash nor installs an agent integration.

The leaf binding requires canonical root-owned executable and dyld files on the same
read-only filesystem as `/`, without group/other write or set-ID mode. The native
Mach-O slice must declare exactly `/usr/lib/dyld` and
`/usr/lib/libSystem.B.dylib`, without rpaths, embedded dyld environment, custom
libraries or unrecognized load commands. File identity, metadata, filesystem
read-only state and cwd identity are rechecked at the locked launch handoff.
History retains executable and loader digests as binding evidence.

Shield trusts the OS and libSystem/shared cache; it does not verify Apple's
boot seal or hash the full transitive cache. Privileged remounts, OS replacement,
and unrestricted external processes are outside its protection. Cwd is passed by
pathname after revalidation, so external directory replacement remains possible.

## Git inspection

Git requires Apple's Command Line Tools Git 2.50.1 (Apple Git-155) at the exact path
in the table and an ordinary `.git` directory at the enrolled project root. Linked
worktrees, bare repositories, the `/usr/bin/git` dispatcher, Xcode/Homebrew Git,
aliases, extra flags and other commands are unsupported.
Repositories with `.git/info/attributes` are unsupported.

Policy and approval match the original executable and arguments. Shield adds
fixed launch arguments: no pager, fsmonitor, untracked cache, hooks, automatic
maintenance/GC or transport protocols. Status uses short output without untracked
files or submodule inspection; diff disables external diff/textconv and submodule
inspection; log uses bounded one-line output without signature verification or
decoration. These controls follow the
[Git diff contract](https://git-scm.com/docs/git-diff).
Git has no inherited environment: LC_ALL=C, global/system configuration disabled,
optional locks and terminal prompts disabled, lazy fetching disabled, and explicit
GIT_DIR/GIT_WORK_TREE fixed to the enrolled repository. System/global attributes
are disabled; repository attributes are read from the empty SHA-1 tree instead
of working/index attributes, preventing clean/process filters. Diff therefore
reports raw working bytes rather than filter-normalized content. These settings
use the [Git attribute-source contract](https://git-scm.com/docs/git/2.46.0).

The root-owned Git executable and its root-owned, non-writable-to-other-users
parent directories are verified. Its native Mach-O contract permits only dyld,
libSystem, libz, libiconv, CoreServices and CoreFoundation; the OS libraries/cache
remain trusted. The native image must contain the observed version marker; that
marker is a compatibility check, not independent publisher authentication.
Git executable bytes are rehashed at approval issuance and launch,
along with ordinary identity checks. Approvals also bind the repository directory
identity, main config and HEAD contents, exact derived launch arguments and settings.
Changing those bindings requires a fresh request.

Approval does not freeze working files, the index, refs, included configuration,
or objects. Git effects are not fully isolated, and privileged executable changes
or external pathname races remain possible.

```sh
shield __internal run --project /path/to/acme -- /Library/Developer/CommandLineTools/usr/bin/git status
shield __internal run --project /path/to/acme -- /Library/Developer/CommandLineTools/usr/bin/git log -n 10
```

## Run with active policy

Initialize the project using [the setup guide](getting-started.md).
Policy matches the absolute system executable and the exact literal arguments,
with the effective canonical cwd. This example permits printing `Acme text`.

The JSON below is one rule, not a complete document. Put it in your selected
custom document's `rules` array with `structured-rules-v1` and
`argument-rules-v1` declared, then review and activate it with `shield policy review`.

```json
{
  "id": "acme-echo",
  "decision": "allow",
  "reason": "Print the Acme fixture",
  "conditions": {
    "domain": "argv",
    "executable": "/bin/echo",
    "arguments": {"kind": "exact", "values": ["Acme text"]}
  }
}
```

```sh
shield __internal run --project /path/to/acme --timeout-ms 5000 -- /bin/echo 'Acme text'
```

`--cwd` selects a directory inside that enrollment and outside `.shield`;
otherwise the project root is used. Accepting a proposal through `shield policy review` publishes its active rule. Ask without a matching native grant exits 3 and deny exits 4
without launching. Ask prints a stored request ID on stderr for operator review.
Text-read approvals cannot authorize this command.
Invalid state/context/binding or launch failures exit 2 and report their cause.
Normal child exits preserve the actual status, including false's exit 1.

## Approve one exact Ask request

After Ask prints a `run-request-…` ID, inspect and approve it in your terminal,
then retry the original command with that ID:

```sh
shield request list --project /path/to/acme
shield request show REQUEST_ID --project /path/to/acme
shield request approve REQUEST_ID --project /path/to/acme
shield __internal run --project /path/to/acme --timeout-ms 5000 --approval-request REQUEST_ID -- /bin/echo 'Acme text'
```

Repeat the original argv, cwd, timeout and supplied `--agent`, `--session-id` and
`--tool-use-id` exactly. The selector supplies no authority by itself. Grants bind
enrollment, activation, revision and candidate digest; literal request; cwd device/
inode; executable/dyld device/inode/size/mtime/digests and verified loader contract;
the fixed environment; timeout duration; declared origin; and the private
`shield-native-run-v1` consumer. The timeout begins afresh with each run attempt;
grant expiry is a separate wall-clock limit on consumption. The fixed consumer
identifies this CLI boundary across retries, not an authenticated process or user.

Approval reopens and verifies the retained native resources against current Ask
policy. The grant also binds the original operator CLI provenance and its minimum
issuance record; an issuance retry keeps that original evidence. It cannot override
Deny or authorize operator/state mutation, unsupported
commands, shell mode or SDK Bash. MCP and the normalized host transport expose
no native grant issuance or consumption. CLI origin fields and operator invocation
remain declared evidence; deployment must protect the operator/state boundary.
Unrestricted same-user access is outside that boundary.

Pending requests expire after ten minutes; approval lasts five minutes. Repeated
approval does not renew it. Use `shield request deny REQUEST_ID --project /path/to/acme`
to reject a pending request or revoke an unused grant.

Expired, revoked, consumed, foreign, changed, or stale references cannot launch.
A retry with an explicit reference cannot fall back to a later Allow policy.
Submit a fresh run without the old selector for fresh review.

The shared writer lock covers revalidation, durable consumption and spawn.
Consumption plus minimum authority is published before launch; a publication
failure withholds launch, but may already have committed consumption. Launch
failure, cancellation, timeout, lost replies and lock-release failure never restore
the grant. Inspect its stored status before retrying. Consumption is not execution
evidence. History opt-out/purge preserves authority. Each enrollment retains at
most 128 native requests. Terminal and expired rows are reclaimed during state
updates; reclaimed request IDs are unavailable for inspection or retry.
The existing 16 MiB state and 1024 authorization-event limits also apply.

## Process ownership and history

The timeout defaults to 5000 ms and accepts 1..3600000 ms, including preparation.
Native filesystem calls have no hard deadline. A running child gets its own process
group. SIGINT, SIGTERM and SIGHUP are forwarded to the exact direct child and
original group; timeout sends TERM. A child still in that group may receive the
signal twice. After a 500 ms
grace, KILL escalates even if the direct child has already exited. On ordinary
child exit, abort and guard cleanup, KILL stops the direct child and remaining
original-group members, even if the direct child moved to another group. The
launcher never signals the new group it joined.
The launcher observes exit without reaping, reserves the original leader's PID
through final group cleanup, and then reaps that child. It never signals the group
after reaping. A separate 500 ms polling window requires direct-child exit and no
remaining live original-group members after KILL; an observation or cleanup failure is reported as an execution
error, rather than successful completion. Native inspection and reaping can block,
so these windows do not provide a hard wall-clock guarantee.
Timeout exits 124; cancellation uses 128 plus the supplied signal. The direct
child's actual exit/signal remains separate from that disposition.

This covers the exact direct child and processes that remain in the original group. It does not contain
descendants that change process groups or create sessions, launch through another
host, or race group inspection with further forks. External processes joining
the group and unrestricted same-user interference are outside this ownership
contract. The supported leaf commands do not spawn descendants; wider commands
remain blocked. Parent SIGKILL or machine failure cannot run cleanup. Descendant
zombies are reaped by their parent or the OS, not by Shield.

The launcher holds the activation lock only through verification and spawn,
then releases it before history writes and waiting. Child ownership survives
post-spawn failures; errors terminate/reap the owned child. Policy changes after
launch govern future requests. Inherited file descriptors and shell redirection
before Shield starts belong to the invoking host, not this launch boundary.

Default optional history records original policy/binding, handoff disposition,
whether spawn occurred, PID and observed exit/signal/completion. CLI attribution
remains caller-declared. Completion links the earlier allowed decision only while
it remains retained. No stdout/stderr is captured; literal request arguments still
follow the existing redaction contract. Opt-out/purge and unavailable history do
not change execution permission or child results; failures are reported separately.
The [project history guide](project-review.md) describes inspection and settings.

Native acceptance covers macOS 15.7.9 with x86_64 approval fixtures. Other tools,
loader configurations, and platforms are unverified. Operator identity is
caller-declared.
