# Navigation store

Navigation holds its state in a store built on Lattice's state management
(`@canonic2/lattice`), which defines stores, selectors, bindings, phases,
transitions and asynchronous work. This page owns the store's design.
[Navigation](navigation.md) owns the behavior it holds; the
[browser UI port](../system/ui-port.md#navigation-store-adoption) owns the order
in which the store is adopted.

**Status:** agreed, not implemented; see
[implementation gaps](#implementation-gaps).

## State

| Field | Meaning |
| --- | --- |
| `id` | Opaque identity of the current navigation |
| `requested` | The destination being navigated to: page, scenario, lens, example, declared artboards and the iteration selected for each declared or implicit artboard |
| `displayed` | The destination and navigation `id` whose content is presented, or `null` |
| `slots` | For the current `id`, each slot of `requested` and its state: `pending`, `ready` with its view, or `failed` with its problem |
| `phase` | `idle`, `pending`, `loading`, `shown` or `failed` |
| `problem` | The destination's own failure message when it failed as a whole |

| Phase | Meaning | Canvas presentation |
| --- | --- | --- |
| `idle` | Nothing requested yet | Empty canvas |
| `pending` | Requested, inside the grace period, a slot still pending | Outgoing view retained and inert |
| `loading` | Requested, grace period elapsed, a slot still pending | **Loading preview…**, no outgoing pixels |
| `shown` | Every slot settled and at least one ready | Ready slots interactive; each failed slot shows its problem in place |
| `failed` | The destination failed as a whole, or every slot failed | The destination's problem, or each slot's problem in place; no outgoing pixels |

Requested and displayed are separate fields, as [navigation feedback](navigation.md#navigation-feedback)
requires. A displayed artboard matches `requested` only when it also shows the
iteration requested for it, or none when none is requested; under a lens other
than design, every artboard matches with no iteration, since only the design
lens renders [iterations](../pages/iterations.md). The canvas accepts input exactly when `phase` is `shown` or `failed`
and no capture is running. One binding writes the canvas's `inert`,
`aria-busy`, visibility and loading message; no other code writes them. The
camera during pan, zoom and reading scroll stays in `<wb-canvas>`; navigation
receives settled values only.

## Events

| Event | Source | Effect |
| --- | --- | --- |
| `picked(destination)` | Sidebar, route, editor | New `id`; `requested` set; `pending` |
| `deadline(id)` | Grace timer | `pending` → `loading` when `id` is current |
| `ready(id, slot, view)` | Renderer | The slot becomes `ready`, when `id` is current |
| `failed(id, slot, problem)` | Renderer | The slot becomes `failed`, when `id` is current |
| `failed(id, problem)` | Activation | The destination fails as a whole: `failed`, when `id` is current |
| `navigated(from, id, destination)` | Renderer, after a link or form inside the preview | Adopted as the current navigation when `from` is current |
| `captureStarted` / `captureEnded` | Review | Input blocked for the capture's duration |

When the last pending slot settles, the navigation leaves `pending` or
`loading`: to `shown`, with `displayed` set to the destination, if any slot is
ready, otherwise to `failed`.

## Identity across documents

The canvas document and each content renderer own separate stores and exchange
messages that carry navigation identity:

- The shell sends every render request with its navigation `id`. The renderer
  tags readiness, failure and metadata reports with the `id` of the request
  they answer, as the [content renderer contract](../system/rendering-flow.md#content-renderer-contract)
  requires.
- When the preview navigates itself, through a link or form the shell hands
  back to it, the renderer mints a new `id` in its own namespace and reports
  `navigated(from, id, destination)`. The shell adopts it only when `from` is
  its current `id`; later reports for the new `id` then match.
- Reports whose `id` is not current change nothing. A late report from a
  superseded navigation, a parked session or a reused warm renderer can never
  mark the current navigation ready or replace what is displayed.
- The shell and each renderer runtime mint identities with distinct prefixes,
  unique per document lifetime.

## Invariants

These hold after every event, in any order:

1. When `phase` is `shown`, `displayed.id` equals `id`, every slot is settled,
   and every ready slot's view matches its slot of `requested`.
2. An event carrying a non-current `id` returns an empty patch.
3. The canvas accepts input if and only if `phase` is `shown` or `failed` and no
   capture is running.
4. In `loading`, `shown` and `failed`, no outgoing content is presented.
5. A navigation leaves `pending` and `loading` exactly when every slot has
   settled or the destination has failed as a whole; a link followed inside the
   preview, once reported, settles the same way as a pick.
6. A failed slot never removes, hides or blocks a ready slot of the same
   destination.

Transition tests drive generated event sequences through these invariants.
Browser regressions through the shell and renderer cover the
[discoveries](navigation.md#discoveries).

## Implementation gaps

- No store exists. The same facts live in closures across the legacy shell,
  the navigation and preview-session controllers, the canvas bootstrap and the
  content renderer, reconciled by explicit calls such as `present()` and
  per-owner sequence counters.
