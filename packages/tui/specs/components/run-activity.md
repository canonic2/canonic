# Run Activity

## Purpose

Represent the host's whole run independently of individual tool calls and transport
connectivity. Keep an editable Composer draft while requesting cancellation.

## Current status

The reusable state/view, Chat composition and twenty deterministic gallery
scenarios are implemented. Engineering checks and human visual acceptance are
recorded independently.

## Behavior and state transitions

A snapshot has a stable, nonempty run ID, monotonically increasing revision,
host elapsed duration, state and explicit cancel/options capabilities. The states
are idle, preparing, running, waiting, completed, cancelled, interrupted and failed.
Waiting names the subject and number of questions. Failure and interruption carry
host reasons. A terminal result cannot return to an active state for the same ID.
A new run uses a new ID and starts without the previous run's pending effects.

Escape requests cancellation once when the host allows it. A repeated Escape
emits nothing and shows “already asked”. The local cancelling state lasts until
the host reports an outcome or acknowledges a failure. Successful submission of
a cancel request alone is not proof of cancellation. A host-confirmed cancelled
snapshot uses a dim `–`; failure alone uses `✕`. If completion arrives first, it
clears the cancel and remains completed. A failure acknowledgement restores the
last authoritative run state, displays the host reason and allows a fresh Escape.
Ctrl+G requests options only when the host supplies that capability; the host
owns the Dialog and allowed actions. Closing options causes no run operation.

Connection is a separate input: online or offline, with an optional host retry
countdown. Offline during an active run shows “disconnected · run state unknown”
and freezes the authoritative snapshot. It does not imply failure, cancellation
or interruption. Offline after a terminal outcome leaves that outcome intact.
The connection marker belongs on the right of the host status row.

The activity row says ready for idle/completed; preparing or working with elapsed
seconds for active states; question count, subject and Ctrl+G for waiting; cancelled
with “work so far kept”; interrupted with its reason; or run failed with its reason.
Cancellation failure takes precedence over the active label. An animation advances
every 140 ms from an injected clock. Reduced motion keeps dots static; elapsed
duration always comes from the host. Below 50 columns optional hints disappear;
Escape remains when cancellation is available. ASCII changes generated symbols
only, independently of color. NO_COLOR preserves Unicode and readable labels.

## Implementation design

`run_activity` owns bounded snapshots, opaque instance tickets, cancellation
deduplication, notices and a caller-owned one-row view. Updates validate atomically.
No terminal, timer, transport, filesystem or executor is acquired. Host text is
sanitized and bounded; ID and revision errors reject the entire update.

## Host boundary

The host dispatches Escape/Ctrl+G only in the run scope after focused overlays have
consumed their keys. It consumes identified intents and reports actual outcomes.
Tickets cannot be acknowledged by another instance or consumed twice. Active
snapshot updates do not invalidate a pending cancellation; a final outcome does.
The host supplies connection changes, durations, retries and allowed options.
Composer text, cursor, focus, queue and Chat anchors are outside this component.

## Acceptance criteria

- **RUN-1:** Revisions, identities and terminal outcomes reject stale or foreign updates.
- **RUN-2:** Repeated Escape emits one cancel; only host confirmation labels cancelled.
- **RUN-3:** Completion wins a pending cancel; late/duplicate acknowledgements are inert.
- **RUN-4:** Cancel failure permits a fresh ticket without erasing the draft or run.
- **RUN-5:** Disconnection never fabricates an outcome; idle outcomes stay historical.
- **RUN-6:** Injected animation, reduced motion, narrow and color/alphabet profiles remain legible.
- **RUN-7:** Options require an explicit capability and have no implicit external effect.

## Remaining gaps

Human visual acceptance and real consumer integration remain separate checkpoints.
Native transport behavior remains consumer-owned.
