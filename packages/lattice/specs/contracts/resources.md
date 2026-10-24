# Resource lifecycle

## Purpose and scope

These agreed guarantees apply to resources owned by capabilities, bindings,
elements and scheduled work. They define observable cleanup behavior rather
than a required base class, resource registry or helper. No resource API is
implemented by the scaffold.

Factories and connection operations document what they create, who owns it and
how it ends. Listeners, subscriptions, observers, timers, workers and pending
asynchronous operations belong to a documented lifetime. Expose explicit
cleanup or accept an owning `AbortSignal`; resources must not outlive that owner.

## Ending a lifetime

Cleanup is idempotent, releases owned resources and prevents later callbacks
from delivering stale state or touching disconnected DOM. Handle already-aborted
signals before acquiring resources. Clean up partial initialization when setup
throws or asynchronous work fails. Observe rejected promises; cancellation does
not replace operation-identity checks when work can complete after supersession.
Document whether disposal is synchronous or asynchronous and how outstanding
operations settle. Reconnection creates a fresh lifetime without duplicate work.

## Cleanup failures

A release step can itself throw. Cleanup attempts all remaining releases and
preserves the original failure:

- Cleanup attempts every release step, in reverse order of acquisition, even
  when an earlier step throws. One failing step never leaves later resources
  held.
- When cleanup runs because setup or an action failed, that original failure is
  what propagates to the caller. Errors from the cleanup steps go to the owner's
  error policy, such as an injected `report` or the platform's `reportError`;
  they never replace or hide the original failure.
- When cleanup runs on its own, such as a `dispose()` call or an abort with no
  earlier failure, it completes every step and then throws the step's error, or
  an `AggregateError` holding all of them when several steps failed. Cleanup
  triggered by an `AbortSignal` cannot throw to anyone; its step errors go to the
  owner's error policy.
- A lifetime is marked ended before its first release step runs. Disposing or
  aborting it again, including after a cleanup that partly failed, does
  nothing: failed steps are not retried, and their errors were already reported
  or thrown once.

## Ownership and acceptance

Keep platform resources outside serializable state snapshots. Lifecycle tests
exercise the real owned boundary where practical, including disposal, repeated
cleanup, abort, partial failure, late completion, a failing release step that
leaves later steps still released, an original failure preserved over cleanup
errors, and repeated disposal after a partly failed cleanup. Browser-only
behavior uses real browser tests; pure state behavior remains testable in Node.

The specializing contracts own additional behavior:

- [State management](state-management.md): subscriptions and capability operations.
- [Bindings](bindings.md): connection setup, rollback and action reporting.
- [Element lifecycle](element-lifecycle.md): connection, moves and reconnection.
- [Scheduling](scheduling.md): cancellation and scheduled callback completion.
