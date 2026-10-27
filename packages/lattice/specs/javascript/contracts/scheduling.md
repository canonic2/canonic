# Scheduling

## Purpose and status

This contract defines when work may be deferred instead of applied
synchronously, and how deferred work is owned, ordered, cancelled and recovered
from failure. Lattice applies state, bindings and element inputs synchronously
by default; scheduling is the exception, introduced only where an interaction
pattern needs it.

**Status:** agreed contract, not implemented. Lattice exports no scheduling API.
A frame scheduler helper is a candidate under the conditions in
[decisions](#decisions). [State management](state-management.md) owns commits
and product timers; [DOM updates](dom-updates.md) owns what an update writes;
[performance and evidence](performance.md) owns how any scheduling is justified
and how responsiveness is measured; [resource lifecycle](resources.md) owns
shared cleanup and cancellation guarantees.

## Synchronous by default

- State commits, binding assignments and element setters run synchronously, as
  their contracts specify. A click, key press or typed character updates state
  and the affected DOM before the event handler returns, so the change is
  included in the next rendering opportunity the browser takes. Whether that
  opportunity presents a frame promptly depends on the browser and the rest of
  the page; presentation is measured, not assumed.
- No scheduler wraps state changes. A commit is never deferred to a microtask,
  an animation frame or an idle period to batch it with others; an action that
  changes several fields commits once instead.
- Feedback to direct input is never deferred: pressed states, selection, focus,
  typed text and toggles.

## Moments in a frame

Three different things are easy to confuse, and this contract keeps them apart:

| Moment                           | Mechanism                                             | Guarantees                                                                                                                                            | Does not guarantee                                                                                                  |
| -------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Before the next rendering update | `requestAnimationFrame`                               | The callback runs before the browser recalculates style and layout for that frame, so its writes are included in it                                   | That geometry of DOM changed earlier in the frame is available without a forced layout; that the frame is presented |
| Geometry available               | `ResizeObserver` and `IntersectionObserver` callbacks | The observed sizes and intersections were computed by the rendering update's layout, without a forced layout                                          | That further writes in the callback are free: writes there can cause another layout pass in the same frame          |
| Presented on screen              | None; only measurement                                | Event Timing entries and traces show when a frame was presented, as [performance and evidence](performance.md#rendering-and-responsiveness) describes | Anything a callback can wait for                                                                                    |

Lattice offers no "after paint" callback and does not build one from nested
animation frames or timers: such constructions do not prove presentation.

## When scheduling is justified

| Need                                                | Mechanism                                                                     | Example                                                                 |
| --------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Values that change faster than the screen refreshes | Apply the latest result once per frame, before the rendering update           | Pointer position during a drag, pan and zoom, a scroll-linked indicator |
| Geometry of newly changed DOM                       | Observer callbacks, not a frame callback                                      | Sizing a popover after its content changed                              |
| Work with no visible effect soon                    | Background tasks that yield, as [background work](#background-work) describes | Preparing data for a view that is not open                              |

Anything else is applied synchronously, including focusing a node that was just
shown. A new need is added to this table with a workload and evidence, as
[performance and evidence](performance.md) requires.

Product timing, such as a grace period before showing a loading message, is
not scheduling: it is a timer owned by a capability's action, with its result
applied through the capability's transition rules.

## Ownership and cancellation

- Every scheduled callback belongs to an owner with a lifetime: an element's
  connection, a binding or a capability. It is requested with that owner's
  `AbortSignal` and never runs after the signal aborts.
- There is no global scheduler and no shared queue that outlives its owners.
- An owner requests a frame only when it has pending work, and stops requesting
  frames when it has none. Nothing runs a continuous frame loop while idle.
- Aborting an owner cancels its pending frame, observer and background work.
  Cancelling twice is harmless.

## Continuous interactions

A continuous interaction, such as a drag, a pan or a scroll-linked gesture,
receives many input events between frames.

### Values, not dropped deltas

- The owner updates the interaction's authoritative value synchronously on every
  input event: an absolute position is replaced, and a relative movement is
  added to the accumulated value in the order the events arrived. No event's
  contribution is dropped.
- Once per frame, the owner paints the latest result of that value. Painting is
  coalesced; updating the value is not.
- When an API consumes deltas rather than absolute values, the owner
  accumulates the deltas since the last frame, in order, and applies their sum
  once in the frame.
- The painted value is presentation. The authoritative value stays with the
  element that tracks the interaction; the settled value reaches a store when
  the interaction ends, as state management's frequent values rule requires.

### Interaction end and identity

- Each interaction has an identity, such as a sequence number the owner
  increments when an interaction starts.
- When an interaction ends, the owner cancels its pending frame, applies the
  final value synchronously and commits the settled value to the owning
  capability.
- A frame callback applies only if its interaction's identity is still current.
  A frame left over from an ended interaction can never overwrite the value of a
  newer one.

### Requests, failures and rescheduling

- The owner clears its pending-frame marker before applying a frame's work, so
  the marker always reflects whether a frame is actually requested.
- An update requested while a frame's work is being applied belongs to the next
  frame: it requests one new frame and does not rerun the current work.
- If applying a frame's work throws, the error goes to the owner's error policy,
  such as the platform's `reportError` or an injected `report`. Because the
  marker was already cleared, the next update requests a frame normally;
  scheduling can never be left stuck by an exception.
- Several owners requesting frames for the same rendering update is acceptable;
  the browser runs their callbacks together. A shared frame loop across owners
  needs evidence that it saves work.

## Reads and writes

- Reading layout, such as `getBoundingClientRect` or `offsetWidth`, after any
  layout-affecting write since the last rendering update forces a synchronous
  layout. That write may come from any owner, including earlier work in the same
  event loop task; ownership of nodes does not isolate layout.
- An owner keeps its own reads before its own writes within a frame and prefers
  observers for geometry. This reduces the risk of forced layouts; it does not
  guarantee their absence, because another owner may have written first.
- Owners that measurably force layouts on each other in a workload coordinate
  through shared read and write phases for that frame. That coordination is the
  frame scheduler candidate, added only where a measured cross-owner dependency
  justifies its cost.
- Whether an update forces layout is established by measurement in the
  workloads where it matters, not by the ordering rules alone.

## Background work

Background priority is a preference about which waiting task runs next. It does
not reserve idle time, and it does not interrupt a task that is already
running.

- Background work runs as `scheduler.postTask` tasks at background priority,
  with the owner's signal, so cancelling the owner cancels tasks that have not
  started.
- Long work is split into pieces short enough not to delay input. Between
  pieces it yields to the event loop with `scheduler.yield()` or by posting the
  next piece as a new task. Awaiting an already resolved promise is not a yield:
  it continues in the same task, before input and rendering get a turn.
- After every yield or other continuation, the work checks its signal and stops
  if the owner was cancelled.
- A cancelled task or yield rejects with an `AbortError`. The owner's error
  policy treats that as an expected outcome and does not report it; any other
  rejection is reported. No background promise is left without a rejection
  handler.

## Hidden documents

Browsers may suspend or throttle animation frames in hidden documents, such as a
background tab or an editor panel that is not visible. Correctness never
depends on frame delivery:

- State keeps updating while frames are not delivered; only presentation waits.
- The first frame delivered afterwards applies the latest value, not a queue of
  stale ones.
- An interaction that ends while frames are suspended applies its final value
  synchronously, as [interaction end](#interaction-end-and-identity) requires.
- Work that must complete regardless of visibility, such as releasing a lock or
  saving, is not scheduled on frames.

## Testing

Browser tests in Chromium:

The following is a required acceptance matrix for scheduling work. Lattice
has no scheduler implementation or dedicated scheduling suite. Immediate
store/binding behavior is covered by the existing primitives and examples;
those tests do not establish frame, background-task or hidden-document cases.

| Case                            | Assertion                                                                                                            |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Coalescing                      | Many absolute updates between two frames produce one paint with the latest value                                     |
| Deltas                          | Several relative movements between two frames all contribute: the painted result equals their ordered sum            |
| Interaction end                 | Ending an interaction applies the final value synchronously, commits it, and a frame pending from it makes no change |
| Newer interaction               | A frame left from an ended interaction does not overwrite a newer interaction's value                                |
| Idle                            | With no pending work, no frame is requested                                                                          |
| Abort                           | A pending frame, observer callback or background task does not run after its owner's signal aborts                   |
| Request during a frame          | An update requested while a frame is applied requests exactly one next frame                                         |
| Throwing frame                  | After a frame's work throws, the error reaches the owner's policy and the next update is painted in a later frame    |
| Background yield                | An input event dispatched during long background work is handled between pieces                                      |
| Cancellation after continuation | Work cancelled while yielded does no further piece after resuming                                                    |
| Background errors               | A cancellation's `AbortError` is not reported; any other rejection is reported once; no unhandled rejection          |
| Geometry                        | Measuring changed content in a `ResizeObserver` callback causes no forced synchronous layout                         |
| Immediate feedback              | A direct input updates state and DOM before the event handler returns, with no frame in between                      |
| Hidden document                 | State updates while frames are not delivered, and the first frame afterwards applies only the latest value           |

Tests advance frames by awaiting real animation frames in the browser runner,
not by replacing the platform's scheduling functions. Responsiveness of the
interaction workload is measured as [performance and evidence](performance.md)
describes, not asserted from callback timing.

## Decisions

- **No scheduler around state.** Deferring commits would delay feedback and make
  what a person sees depend on timing. Coalescing belongs to the few owners whose
  values change faster than the screen.
- **Owned, cancellable work.** Tying every callback to an owner's signal makes
  cleanup the same single abort as every other resource, and prevents late
  callbacks from touching disconnected DOM.
- **Coalesce painting, never input.** Updating the authoritative value on every
  event and painting once per frame keeps every movement while doing visual work
  once.
- **No promise of presentation.** Frame and observer callbacks describe when
  work runs relative to the rendering update, not when a frame is shown;
  presentation is a measurement.
- **Frame scheduler as a candidate.** A helper that coalesces values and orders
  reads before writes across owners is added when two owners need it and a
  measured cross-owner layout dependency justifies it. It must stop when idle,
  cancel through signals, survive throwing work, add no work for owners that do
  not use it, and beat per-owner frame requests on the workloads where it is
  used.

## Open questions

- Whether background work uses `scheduler.postTask` and `scheduler.yield`
  directly, or whether a thin wrapper that adds signal checks and the
  cancellation policy is worth providing.
- Whether the frame scheduler candidate should be one per document or one per
  owner group, such as a canvas and its overlays.
