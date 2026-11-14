# Embedded enforcement handoff

Use `shield::enforcement::handoff` to embed Shield in a trusted Rust host.
It revalidates policy evidence and invokes your callback only for an allowed
operation. Your host classifies requests and performs the operation.

For Claude or Codex setup, use [native project setup](claude-project.md).
For restricted SDK reads, use [Claude SDK integration](claude-read-only.md).
The embedded API installs neither integration.

## Integration contract

Load a snapshot of the project's [policy files](policy-files.md), evaluate the
request, and classify it with `enforcement::AgentAccess`:

| Access | Gate behavior |
| --- | --- |
| `Ordinary` | Recheck that the policy files are unchanged; invoke the callback only for allow |
| `Operator` | Block approval, grants, policy changes and protection disabling |
| `AuthorizationStateWrite` | Block direct writes to the policy files, approval state or their storage parents |
| `Unclassified` | Block unsupported or ambiguous requests |

Allow rules cannot override protected or unclassified access. Ask returns
`ApprovalRequired`, Deny returns `Denied`, and state or locking failures return
`Runtime` with the cause. This API cannot issue or consume approval grants.

Classification is the host's responsibility; `AgentAccess` cannot be deserialized
from agent JSON. Intercept every applicable tool path, including indirect writes
to state files, directories, and ancestors. The API does not inspect paths,
resolve aliases, parse vendor tools, or verify executable identity and future
shell effects. Incorrect classification defeats its protection.

## Handoff and failures

The callback receives the original request/evaluation while the writer lock is
held. Keep it short; do not wait for process completion or acquire another review
writer lock. Changed policy files, another project's evidence or corrupt state
prevents ordinary handoff; evaluate the request again. Snapshot checks do not
prove executable or filesystem stability.

The outer `Result` reports protection failures; the inner value is the callback's
result. Panic unwinding releases the lock. A lock-release error can occur after
the callback ran, so check its effects before retrying.

Handoff consumes its evidence through Rust ownership. Callers can evaluate a new
request; consuming evidence is not a one-time approval grant. Unrestricted
processes can still invoke the operator CLI or write state.

## Verified scope

Local macOS tests cover decisions, protected access, stale and corrupt state,
callback errors, and panic cleanup. Doctests check ownership and reject JSON
access claims. Host interception and request classification need their own
verification.

The embedded gate writes no audit history. The
[SDK adapter](claude-read-only.md) and [native runner](native-run.md) use separate
approval gates and record their own observations. None provides OS isolation
from unrestricted same-user processes.
