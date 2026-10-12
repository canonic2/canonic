# Status Panels & Notices

## Purpose

Show supplied facts alongside a conversation and keep notices requiring action
reachable without interrupting the draft or a review. The component never
discovers files, probes connections, computes prices or executes actions.

## Current status

Reusable `StatusPanels`, opt-in Chat routing and 27 deterministic gallery
scenarios are implemented. Human visual acceptance and product adapters are
separate checkpoints. See [verification](../verification.md).

## Behavior and state transitions

Sections have host-defined stable IDs, order and semantic kinds. Context, cost,
to-dos, changed files, connections and child activity share five explicit states:
unknown (not reported), loading, ready, empty (zero plus explanation) and error
(reason plus retry only when offered). Context bars contain a supplied count of
filled cells; warnings and numerical summaries are host facts. Loading retains
the previously rendered section height, capped at 64 rows. Compact rendering
does not change saved folds.

Ctrl-X I enters status; Ctrl-X N enters the notice list. Up/Down selects stable
section/row identities, Left folds and Right unfolds, Enter requests an offered
action, R retries an error section when offered, and Esc returns to the retained
caller. Alt-PageUp/PageDown scrolls selected details. Paste, modified effect keys,
release and repeated effect keys cannot issue operations. Disabled actions show
their supplied reason and never request execution.

Notices sort by severity, then newest supplied publication time; ties retain host
order. One current notice occupies the activity row; additional notices show a
count and the list shortcut. Ctrl-G requests its action after existing review/run
attention handling. Pending actions are single-flight per target. Exact host
acknowledgement resolves the notice or shows a retryable failure reason. A failed
action becomes persistent error attention even if the original notice was info.

`advance(Duration)` supplies monotonic time. Backward updates are inert. Info and
success expire at their optional deadlines, except while pending or failed.
Warning/error notices remain until dismissal or authoritative resolution.
D in the list sets actionable errors aside: they leave current attention but
remain in the list and `! N` badge. Revised notices become fresh attention.
Closing a pane retains pending host work; a late response updates facts without
reopening it. A host can invalidate work with a newer material revision or removal.

### Placement and presentation

The gallery uses a 32-column sidebar at 120+ columns, summary and Dialog below
120, and `! N` plus context with a full-width status view below 50. Reviews fold
sections to summaries; insufficient height hides the optional sidebar. The draft
remains visible and retains caret, editing history and caller focus. Hosts own
placement and may render the same view into any supplied rectangle.

Semantic colors support ANSI 16, indexed/RGB, light tokens and attributes-only
NO_COLOR. ASCII replaces generated decorations while preserving supplied Unicode
text. Loading/progress decorations are static, including reduced-motion hosting.
Control characters are rendered as safe text. Long reasons wrap and scroll;
footer space reserves Esc at narrow widths.

## Implementation design

`src/status_panels.rs` owns snapshots, stable focus/folds, notices and exact
requests. `status_panels/model.rs` owns supplied descriptors and validation;
`input.rs` owns navigation/effect routing; `view.rs` owns wrapping, profiles and
derived summaries. Section projection counts wrapped rows and allocates only the
requested viewport, keeping long reasons accessible without storing offscreen rows.
`gallery/status.rs` owns Chat, placement, Dialog and simulated
acknowledgements. Chat emits identified `Intent::Status` only when its adapter is
enabled; `Chrome` supplies presentation without transferring mutable notice state.

Snapshots must increase global revision; changed records must increase their
record revision. Invalid replacement is atomic. Unchanged records preserve
pending work, errors, folds and selection. Material changes invalidate old
requests. Requests include opaque instance-owned tickets, target, record revision
and action ID; foreign, altered, stale and duplicate replies are inert.

Limits: 32 sections, 128 notices, 2,048 total section rows, 128-byte non-control
IDs, 4,096-byte labels, 65,536-byte reasons/details and 4 MiB aggregate descriptor
text. Pending operations cap at 128; retained failure text caps at 4 MiB.
Oversized completion reasons preserve pending work and return an error.

## Host boundary

Hosts own clock advancement, snapshot revisions, authorization, operations,
authentication, persistence and focus composition. An action descriptor is not
authorization. Rendering and input acquire no terminal or OS resources. Returning
does not cancel an already-issued operation. The host retains the caller object
and restores it; the component emits `Returned` rather than reconstructing drafts.

## Acceptance criteria

- **SP-01:** Five states, host facts/order, folds and stable selection survive
  compact rendering, resize and authoritative changes.
- **SP-02:** Actions require deliberate keys and exact acknowledgements; malformed
  data, foreign/stale/duplicate results and disabled actions preserve accepted state.
- **SP-03:** Explicit monotonic expiry, severity ordering, pending/failed actions
  and set-aside errors preserve retry and badge reachability.
- **SP-04:** Responsive Chat/Dialog composition preserves the draft and review
  priority; profiles, clipping, tiny areas and long reasons remain usable.
- **SP-05:** Runnable fixtures, reviewed snapshots and owned PTY input/resize/
  terminal restoration establish local engineering evidence.

Public behavior tests live in `tests/integration/status_panels.rs`, gallery and
snapshots in `status_gallery.rs`, and Unix PTY acceptance in `status_terminal.rs`.

## Remaining gaps

Human visual review, actual product action adapters, native Windows acceptance
and inline terminal hosting remain separate work. The gallery acknowledges
simulated actions; real inspection/authentication decisions stay with consumers.
