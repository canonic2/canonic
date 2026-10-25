# Navigation

This topic owns how the shell moves from one destination to the next: the
requested destination versus the displayed one, the 200 ms grace period,
pending, loading and failed presentation, settling per slot, group publication,
and capture, handoff and context while navigation is pending. The
[navigation store](navigation-store.md) specifies the state design that holds
these rules. The [rendering flow](../system/rendering-flow.md) owns the
shell/content boundary, [readiness](../preview-pipeline/readiness.md) owns when
content counts as ready, and the
[warm runtime](../preview-pipeline/warm-runtime.md#retained-sessions) owns which
sessions stay alive between destinations.

**Status:** implemented in the current shell controllers. Not implemented:
holding these rules in the [navigation store](navigation-store.md), and a fix
for the two failures recorded under [discoveries](#discoveries); see
[implementation gaps](#implementation-gaps).

## Navigation feedback

Page selection expresses the user's intent immediately, while the canvas keeps
the identity of the content it actually displays. The sidebar marks the picked
page synchronously, before editor messaging, story lookup or rendering finishes.
The breadcrumb may identify the requested destination while its artboard is pending.

| Time or outcome | Canvas presentation |
| --- | --- |
| Pick begins | Keep the outgoing artboard or Docs viewport visible, with its existing label, geometry, camera, reading mode and reference; make it inert |
| Destination settled before 200 ms | Swap to its content and metadata immediately, with no loading message |
| Still pending at 200 ms | Conceal the outgoing view and show **Loading preview…**; it no longer contributes visible artboards to reference/context publication |
| Settled after the deadline | Replace loading feedback with the destination and its metadata |
| Failure | Show the requested destination's failure under its label, with no outgoing pixels: the destination's problem when it failed as a whole, or each failed slot's problem in its own slot beside its ready neighbors |

A destination is settled when every one of its slots is settled. A slot is one
artboard of the destination: the implicit artboard of an ordinary page, or each
artboard a scenario declares. A slot settles when its content is ready or it
has failed. A destination that fails as a whole, such as one whose page,
definition or lens cannot be resolved, settles at once as failed. A failed
replacement in an artboard shows the requested view's named problem with Retry,
in that artboard only.

An ordinary preview is revealed only when its reported page, scenario and lens
match the request and it is ready or has a named failure. The ordinary
single-view canvas keeps its owning renderer across page, scenario and lens
navigation; navigating it does not replace its iframe. Its first paint stays
hidden until the requested content is ready, so the renderer's hosting
interface cannot flash inside the artboard. Concealing the outgoing view does
not detach its renderer or reload its content. The selected renderer's
completion is relayed to the editor only for the requested page, scenario and lens.

A content renderer may prepare a replacement in a spare content iframe. It
must not use a spare Workbench application document. Promotion waits for
matching destination readiness; superseded loads cannot replace a newer selection
or put outgoing pixels beneath the new destination's label.

The 200 ms period starts at each new selection, including a selection whose story
metadata is still being resolved. It delays feedback, never the rendering work or
a ready page. On first open there is no outgoing content to retain; the loader
still observes the grace period. Rapid picks restart the deadline for the latest
destination, preserve the last displayed view while its new grace period applies,
and invalidate older readiness/error callbacks. A view already concealed at a
deadline is not resurrected by another pending pick. Completion, clear, failure
and disposal release pending deadlines and transient outgoing renderers.

### Loading status and labels

One outer canvas status, **Loading preview…**, covers metadata lookup, such as
a Storybook catalog, and rendering. The shell owns application loading status;
renderers report data rather than creating competing application loading UI.
The content's own loading documents and host labels stay hidden during the
transition. Loading indicators honor reduced motion. A matching ready view
clears the status; a lookup failure replaces it with a persistent error and
clears `aria-busy`. Superseded reports cannot overwrite that error. Choosing
another page or retrying the failed destination starts a fresh status; choosing
no available page restores the empty message without a spinner.

Navigation immediately names the destination page; unresolved metadata leaves
no previous scenario name behind. `wb-view-change` relays label metadata
independently of `wb-frame-change` and readiness, so a label update cannot
acknowledge unrendered content. Cold artboards receive their human-readable
`target.label` before mounting, including authored child names.

### Declared-artboard groups

A declared-artboard group is published as the replacement once every slot has
settled, with each failed slot showing its own problem; one failed artboard
never hides its neighbors. Until then the 200 ms rule above applies to the group
as a whole. Creating the expected instances alone does not establish readiness.
Pending renderers must already have a correctly sized layout viewport. The
canvas supplies invisible, inert staging slots independently of the displayed
state. It cannot defer all slot creation or use `display:none` until readiness:
the renderer's paint/readiness callbacks may depend on that layout, producing
a startup deadlock. Promotion preserves the prepared iframe document and its
state; it changes presentation without reloading content.

Existing warm ordinary/Storybook/Docs sessions keep their
[retention semantics](../preview-pipeline/warm-runtime.md#retained-sessions).
Outgoing declared instances may remain parked until the grace period ends or the
replacement commits, then are disposed. This transition does not authorize an
unbounded pool of outgoing runtimes.

### Capture, handoff and context while pending

Screenshots and handoffs remain unavailable throughout pending navigation, even
when outgoing pixels are visible. A published group with failed slots is
captured under the [capture contract](../review/capture.md#what-is-captured):
failed slots become explicit placeholders. The canvas reference and agent context describe
the displayed outgoing view during the grace period, then no visible artboards
while loading feedback is shown, then the committed replacement. Incoming labels,
sizes, automatic camera fitting and reading mode must not be applied to the
outgoing presentation. Queued focus reports cannot route back to the inert
outgoing page.

### Why the grace period exists

Immediate sidebar feedback confirms the pick independently of rendering latency.
Keeping the previous view briefly provides visual continuity through fast page
changes, while suppressing both a blank canvas flash and a loading-message flash.
A page ready at 25 ms appears at once; it does not wait for 200 ms. A slower page
receives explicit loading feedback at the deadline so the old content cannot
linger indefinitely and suggest that the pick was ignored. The threshold is a
product interaction choice, not a claim that rendering completes within 200 ms.

Keeping requested selection separate from displayed identity also prevents a
reference or handoff from describing a new page while showing the old one's
pixels. This improves perceived responsiveness without changing rendering speed,
weakening readiness, or permitting capture of unfinished content.

## Verification

The navigation controller tests cover deadline cancellation, restart, supersession
and group readiness. Preview-session tests cover transient outgoing retention and
cleanup. The host's [browser regression](../../server/node/src/modules/host/navigation.test.ts)
exercises sidebar selection, outgoing pixels/labels/reference, ready commit,
deadline loading, late readiness and failure through the actual shell and renderer.

## Implementation gaps

- Navigation state is not held in the [navigation store](navigation-store.md);
  its rules are spread across the shell, controllers and renderer.
- The [discoveries](#discoveries) have no browser regression. The link-freeze
  is fixed in the canvas bootstrap, which adopts the artboard's destination
  after a link inside a preview; the blank Docs viewport after rapid collection
  switches has not been reproduced or traced.

## Discoveries

- **Link inside a preview froze the canvas (2026-10-23).** Acme demo, App ›
  Dashboard, **Actions** on: clicking **Inbox** inside the preview loaded
  `app/inbox.html` in the artboard while the shell's selection stayed on the
  dashboard. The canvas computed itself as pending, set `inert` on the artboards
  and skipped the step that adopts the renderer's destination as the route, so it
  never left that state. Every click and key on the artboard was ignored until
  another page was picked. Cause: the requested destination had two owners (the
  shell's selection and the renderer's target) reconciled by a call that a later
  change gated on the very state it was meant to resolve. Invariants 3 and 5 and
  the `navigated` event of the [navigation store](navigation-store.md) cover it.
- **Blank Docs page after rapid collection switches (2026-10-23).** In the
  space of component Docs pages, switching from one Docs page to a Docs page in
  another collection and back within about a second left a blank Docs viewport
  that stayed blank. Observed in a screen recording; not yet reproduced or traced.
  Hypothesis: a late report from the superseded visit or the retained Docs
  session was accepted as readiness for the returning navigation. Invariants 1
  and 2 of the [navigation store](navigation-store.md#invariants) cover that
  class of failure.

## Open questions

- Whether a renderer that never reports for its navigation needs a readiness
  timeout that produces `failed`, or whether `loading` may last indefinitely
  for slow compilation.
- How an explicit single-view link and browser Back/Forward behave in a canvas
  of several artboards; the suggestion is in
  [artboard management](artboard-management.md#proposed-edge-case-contracts).
