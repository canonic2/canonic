# Artboard management

This topic collects the design work for managing artboard instances beyond a
scenario's declared group: adding, duplicating and closing instances, a
keyboard-accessible artboard list, **Fit all** and **Fit selected**, free
movement and **Arrange**, restoring or sharing arrangements, and the delivery
plan for that work. [Multiple artboards](multiple-artboards.md) owns the
requirements and acceptance of what is built; where a proposal below restates
behavior already built for declared groups, that page is the owner. The
[artboard system](artboard-system.md) owns the architecture the proposals extend.

**Status:** proposal, not implemented. Only the arrangement tools, free
movement and **Arrange** actions for a later delivery, are agreed; every interaction, edge-case contract and acceptance criterion below
awaits a decision, and persistence and sharing are open. See
[agreed and open](#agreed-and-open) and the [delivery plan](#delivery-plan).

## Agreed and open

Agreed:

- **Arrangement tools, after the first delivery.** Support free positioning
  and row/grid arrangement. The first delivery can use deterministic automatic
  placement; adding and reviewing multiple artboards must not depend on manual
  arrangement tools.

Open:

- **Persistence and sharing.** Remembering an arrangement locally and storing a
  named arrangement in a shared project file are distinct options. Neither
  storage policy has been agreed; see [questions](#questions-for-product-decisions).

## Suggested first-delivery interactions

- Start with the existing single-artboard experience. **Add artboard** opens
  a page/scenario choice; a sidebar action can add a page directly.
  Ordinary sidebar picks replace the selected artboard's view. Add is an
  explicit action so normal navigation does not accumulate artboards.
- **Duplicate artboard** copies page, scenario, lens, and dimensions into a
  fresh preview instance. Runtime form edits, scroll, and application state
  are not cloned. Control overrides and annotations need a decided copy
  policy before implementation; the recommended default is a fresh instance.
- Each artboard instance has its own page, scenario/story, declared artboard,
  iteration, lens, size and dimensions,
  controls, actions, loading/error state, and annotations. Its descriptor
  includes space identity and source/server resolution. Different spaces,
  content types, and repeated instances can coexist on the same canvas.
- Exactly one artboard is selected. Its outline and label identify it; the
  existing top bar, sidebar highlight, source, and reload act on it.
  Copy reference, screenshot, handoff, and agent context describe the entire
  canvas and identify the selected artboard. Provide a keyboard-accessible
  artboard list so selection and closing do not require a pointer. An empty
  canvas has no selection and disables view-dependent actions.
- Place added artboards in a tidy row automatically in the first delivery.
  Avoid rearrangement during preview edits. The arrangement tools add free
  movement by label and explicit row/grid **Arrange** actions; dragging inside the preview keeps
  interacting with its content.
- Zoom and pan apply to the whole arrangement, under the
  [camera contract](interaction.md#camera-and-scrolling). Provide **Fit all** and
  **Fit selected**. Define the distinction between the current
  Fit size and fitting the camera before implementation; freeze Fit
  to explicit dimensions when adding a comparison artboard is one option.
- Interacting with a visible preview selects its artboard and reaches its
  content. Navigation originating inside a preview replaces that artboard's
  view, leaving its neighbors intact. Cross-origin focus and input selection
  require rendered verification rather than assuming parent pointer events.
- A failed or loading artboard does not disable its neighbors. Reload is
  scoped to the selected artboard. Closing disposes that artboard's resources
  and selects a predictable neighbor; closing the last returns to an empty
  canvas.
- Keep annotations local to each artboard. Selection changes preserve them;
  replacing its page, scenario or selected iteration clears them, while lens and size changes follow
  the existing comparison behavior. Undo targets the selected artboard.
- Screenshots capture all artboards, including ones outside the current camera
  viewport, with their labels and annotations. Suggested output: a composite
  image retaining their relative positions, plus per-artboard images in a
  handoff for legibility. Exact output format and large-canvas handling remain open.
- Handoffs include every artboard instance's space, source, scenario/story,
  declared artboard, iteration, lens, size,
  code pointers, and annotations, mapped to the corresponding captured image
  region or image. Keep stable IDs so repeated views remain distinguishable.
- Agent context reports the whole canvas through the existing agent-context
  capability: an artboard list, each artboard's space and view details,
  selected ID, and loading or failure status. One unavailable artboard must
  not make all context disappear. This must reach existing hooks and MCP
  readers, not just the canvas UI.
- If restoration is included, use versioned descriptors carrying every
  artboard's space identity. Restore views and arrangement rather than
  runtime application state. Browser and editor canvases should not silently
  overwrite one another's arrangements. The persistence destination is open.
  Ordinary single-view selection restoration is separately required by
  [persistence](persistence.md); it does not decide arrangement storage or
  authorize serializing live preview state.
- Preserve existing single-view URLs. An explicit page
  link selects that view; define how it interacts with any restored layout.
  Full-layout links and shared named canvases need their own contract if
  included. Single-artboard agent context retains existing view information;
  multi-artboard context extends it with the whole canvas.
- Docs lenses in artboards follow
  [their contract](../pages/docs-pages.md#in-a-multiple-artboard-canvas).

## Proposed edge-case contracts

These proposals do not reopen the limits established by the
[instance model](multiple-artboards.md#instance-model-and-review-limits).

- Duplicate creates a fresh renderer and copies only space/view/size
  descriptors, with default controls and no copied annotations. Independent
  iframe state does not imply independent cookies, storage, backend state,
  or native device/window state. Never clone or reset an external device to
  simulate independence.
- Closing selects the next artboard in order, or the previous one if there
  is no next. Closing the last leaves an empty canvas.
- Resizing can move following artboards to avoid overlap, but does not
  remount them or reset the camera. Fit all operates on bounds; Fit selected
  is unavailable when empty. Exact spacing is a UI design detail.
- Choosing a space in the catalog changes which pages can be browsed; it
  preserves existing artboards. Adding from a different space is explicit;
  ordinary picks replace the selected instance.
- Suggested single-view link behavior: an explicit single-view URL opens a fresh
  one-artboard canvas; it does not merge into or overwrite a restored comparison.
  Browser Back/Forward routes within its owning artboard rather than appending
  artboards. Full-layout links and restoration history await a persistence
  decision. A zero-artboard canvas's context follows
  [agent context](../integrations/agent-context.md#publication-precedence-and-heartbeats).

## Proposed acceptance criteria

- Fit all includes every artboard; Fit selected centers the selected one.
- If layout restoration is included, one missing space or source does not
  discard all valid artboards.
- The first delivery adds artboards with predictable automatic placement. The
  arrangement tools allow free positioning and row/grid arrangement without
  remounting previews.
- Rendered interaction remains usable at the agreed artboard limit, and
  memory and interaction-latency targets are established before user-facing delivery.

## Questions for product decisions

1. **Saving and sharing:** Should Workbench remember an arrangement only on
   the user's machine, or also save named arrangements in a project file that
   teammates can commit and reopen? For example, a shared "Checkout review"
   could name its artboards and views. The destination and portable references
   for a canvas spanning several spaces need decisions if sharing is wanted.
2. **Capture format:** Is one composite canvas image sufficient, or should
   handoffs also include a full-resolution image of each artboard? All artboards
   are included either way. Large arrangements need a defined resolution and
   image-size policy.
3. **Canvas ownership:** Where should a mixed-space handoff be saved, and
   which space owns canvas-level actions such as export and configuration?
   Recommendation: a host space owns the handoff bundle; editing and export
   explicitly identify the space they target. Participant sources retain
   their own space identities regardless of the selected artboard.

## Delivery plan

| Slice | Integration work | Acceptance evidence |
| --- | --- | --- |
| Artboard management UI | Agree the user-facing add/duplicate/select/close interactions, keyboard access, camera Fit operations and empty/error behavior, using a rendered Acme prototype without presenting proposed controls as shipped behavior. Then extend the implemented keyed presentation and owning-space routing with those interactions, preserving declared composition, the existing shell and the shipping one-view path during bounded migration | Accessible repeated-view workflows, keyboard/focus, geometry and cleanup |
| Space and native host acceptance | Verify the implemented owning-space routing and pooled native subscriptions on real hosts | Mixed-space and shared-root recovery, real native capture/input, independent subscriptions |
| Review and context host acceptance | Verify the integrated camera, full-canvas capture, handoff, reference and participating-space publication on supported hosts: participating renderers and spaces, capture races, cleanup and agent readers. Resolve arrangement storage and optional per-artboard image policy before delivery | Complete composite/prompt, hooks and MCP, race/failure cleanup |
| Product completion | Measure memory and interaction latency at the artboard limit in browser and VS Code; resolve arrangement storage/sharing, recovery and host limits; complete the acceptance criteria and public guides. Single-view restoration follows [persistence](persistence.md) | First-delivery acceptance in browser and VS Code |
| Arrangement tools | Extend geometry and interactions for free positioning and row/grid placement while preserving runtime identity and review output | Stable renderer state, capture mapping and accessible movement |

### Documentation delivery

Public guides describe supported behavior. Apply the following changes when
the corresponding implementation is verified; do not publish planned controls,
YAML keys, shared-layout behavior, or capture formats as available features.

| Source | Verified release behavior to document |
| --- | --- |
| `docs/README.md`, package README, website overview | Flexible multi-artboard canvas across supported content/spaces; full-canvas review output |
| `docs/canvas.md` | Add/duplicate/close/select, automatic placement, per-artboard editing, Fit all/selected, navigation and empty/error states; free arrangement only when shipped |
| `docs/annotations-and-handoff.md` | Artboard-owned annotations; all-artboard captures including off-camera content; image labels/dimensions, failures, bundle location, space-qualified paths, clearing annotations and concurrency |
| `docs/spaces.md`, `docs/extension.md` | Browsing versus canvas ownership, space-qualified source/config actions, unavailable-space recovery, context from all participants and shared-root handling |
| `docs/workbench-previews.md`, `docs/preview-data.md`, `docs/storybook.md` | Instance-scoped controls/actions/navigation and independent stories/previews; external storage/backend sharing |
| `docs/ios-simulator.md`, `docs/windows.md` | Concurrent subscriptions, same-source live state, device-specific input, view-only windows and per-artboard prerequisites |
| `docs/configuration.md` | Only any actually adopted/implemented saved-canvas schema; update all shared schema references in the same change |
| `docs/design-system-export.md` | Explicit space target; catalog reference export remains distinct from canvas review |

The public terminology and single-artboard scope can be clarified before the
feature ships. Existing guide routes stay stable, and a new guide joins the
site's navigation and search. Overview screenshots change only when shipped
visible behavior changes.
