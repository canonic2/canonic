# Scheduling

`stdio::serve` drives one connection. It owns everything on this page.

## Threads

- One reader thread reads bounded frames and hands them to the dispatcher
  through a bounded buffer.
- The calling thread is the dispatcher. It owns the session and all scheduling
  state and writes every frame, so output is never interleaved. It answers
  `ping`, `tools/list`, initialization and invalid arguments itself.
- Each lane has its own worker threads. Workers run the handler, encode the
  reply within the output limit and return it; a handler panic becomes a
  `-32603` reply and the worker continues.
- The dispatcher sleeps until input, a worker result, progress or the next
  deadline. An idle connection does not wake periodically.

## Admission

- `Handler::admit` chooses a lane, an optional key and an optional deadline
  for each tool call, extension request and extension notification. It runs
  on the dispatcher and must return quickly.
- An unconfigured lane is answered with `-32603`. A lane whose queued, running
  and confirming requests reach its capacity answers with `-32000`.
- The deadline starts at admission, so queue time counts.

## Ordering

- Each lane runs up to its worker count at once.
- Requests with the same key in one lane run one at a time in admission
  order. Requests without a key, or with other keys, may run ahead of a
  blocked key.
- A request keeps its key while it waits for confirmation and while its
  continuation runs, so no other request with that key runs in between.

## Deadlines

- A queued request whose deadline passes is answered with `-32001` "Request
  deadline exceeded before dispatch" and never runs.
- A running request whose deadline passes is answered with `-32001` "Request
  deadline exceeded" and its context is cancelled. Its eventual outcome is
  discarded, including when it returns after the deadline but before the
  dispatcher noticed.
- Workers stop only by checking their context; the driver cannot interrupt a
  handler that does not check.

## Cancellation

A cancelled request is removed from its queue, or its running context is
cancelled and its outcome discarded, or its pending confirmation is withdrawn
with `notifications/cancelled`. No reply is sent.

## Progress

- A handler sends progress through its context only when the call supplied a
  progress token. Notifications go through a bounded buffer; when it is full,
  the newest notification is dropped and the call reports `false`.
- Progress from a request that has been answered, cancelled or timed out is
  dropped, so progress never follows its request's reply.
- Progress notifications that exceed the encoded output limit are dropped
  without sending an error response.

## End of input

When the input ends:

1. The session closes. A pending confirmation is withdrawn with
   `notifications/cancelled` and its request is answered with `-32800`
   "Connection closed before confirmation"; its continuation does not run.
2. Queued mutations are answered with `-32800` "Connection closed before
   dispatch" and never run; queued extension notifications are dropped.
   Queued read-only calls still run.
3. Running mutations keep running for the disconnect grace period, then their
   contexts are cancelled. Running read-only calls continue to their deadlines.
   Work that asks for confirmation from then on gets `-32800` without a form;
   see [confirmation](confirmation.md#driver-rules).
4. Replies are still written while the output accepts them.
5. `serve` returns once nothing is queued, running or confirming, after
   joining its workers.

An input read error follows the same steps and is then returned.

## Connection-ending errors

`serve` writes a final error reply and returns `ServeError::Protocol` for an
oversized or unterminated frame, a duplicate outstanding ID or the session
message limit. It cancels running work, drops queued work and joins its
workers before returning. Output failure returns `ServeError::Output` the same
way. The reader thread may remain blocked on its input until that input ends
or the process exits; blocking reads cannot be interrupted portably.

## Options

| Option | Default |
| --- | --- |
| Lanes | One lane, one worker, capacity eight |
| Request deadline | See [limits](limits.md#time) |
| Confirmation deadline | See [limits](limits.md#time) |
| Disconnect grace | See [limits](limits.md#time) |
| Output limit | See [limits](limits.md#sizes) |
| Frame observer | None |

`serve` refuses options without lanes, with a lane without workers, with a
lane whose capacity is below its worker count, or with message limits too
small for protocol replies. The frame observer receives every frame without
its line break, for diagnostics; it must not block.
