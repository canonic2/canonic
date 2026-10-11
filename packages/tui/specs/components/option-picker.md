# Option Picker

## Purpose

Filter, inspect and explicitly confirm host-supplied choices by stable identity,
with safe defaults and separate single/multiple-selection modes.

## Current status

Status: rich picker human accepted in slice 2a. Visual follow-up and renewed
acceptance are recorded in [Presentation](../presentation.md) and
[Human testing](../human-testing.md).

## Behavior and state transitions

### Picker contract

`ChoicePicker` accepts `Choice` values with stable ID, label, detail, group,
disabled reason, danger and current flags. Duplicate IDs reject construction or
refresh atomically. Default selection uses a supplied enabled non-danger ID,
otherwise the first safe row. Filter changes preserve a surviving enabled ID,
otherwise use the first safe ranked match. Refresh retains valid enabled marks
and selection by ID. Host-supplied `Status` distinguishes ready/loading/error;
empty source and no matches have separate messages. No retries or loading run.

`Filter::Auto` enables filtering above seven choices; On/Off override it. The
bounded 4 KiB query uses the editor's grapheme selection, paste and undo. Matching
is a lowercased subsequence, ignoring query whitespace. Label matches rank before
detail matches. Within each field, contiguous prefix matches rank first, then
contiguous matches at a word start, then fuzzy subsequences. A word starts after
a non-alphanumeric character; source order breaks ties within each class.
Whole matched graphemes are bold/underlined. This local algorithm owns no
remote query generation or locale/normalization service.

Arrows/page/Home/End move among enabled rows and keep highlight visible, including
group headers and single-line choice rows. Disabled rows remain explanatory but
cannot be chosen. Danger rows are never implicit defaults or host pre-marks;
explicit Space, digits and Alt-A can mark them. F3 returns `Inspect` only when
the host enables an inspection view with `set_inspect_available`; its hint is
visible only then. Multi-selection is a separate
explicit mode with selected set and final submit; ordinary single-choice Enter
must not change meaning because a consumer supplies many rows. Returned IDs refer
to supplied data, never filtered positions. Compatibility adapters can retain
current original-index results for static choices: the original `Picker`/`Action`
API remains unchanged. Space toggles a mark; Alt-A toggles matching enabled rows.
Hidden marks survive the query and final `Confirmed` IDs follow source order.
An empty marked set can confirm by default when the source contains choices.
With an empty source, Enter is inert and only cancellation is offered.
`set_selection_limits(minimum, maximum)` supplies host-owned constraints;
conflicting bounds reject atomically. A changed maximum retains marks in source
order up to that limit. At the maximum, unmarked rows fade but remain reachable
and cannot gain a mark until one is removed. Below the minimum Enter emits no
intent. Alt-A toggles matching enabled rows, including danger rows, adding only
up to the maximum in ranked display order. Plain Enter press is the
only final confirmation; repeated/modified Enter, paste and disabled shortcuts
are inert. Esc clears a nonempty query before returning `Cancelled`.

## Implementation design

`PickerView` supplies title/source/context (up to three rows), maximum height
(default nine) and presentation. Height derives from source content at opening
and stays stable while filtering, clamped by host area. Details disappear before
labels truncate at grapheme boundaries; group rows count toward paging. Hosts
own placement and execute/recheck returned intents. This supports stage/dock/
full-screen gallery placement; an inline terminal adapter remains slice 12.

### Presentation ownership

The query uses [Editor](editor.md) with `chrome=false` for text-only rendering.
`ChoicePicker` separates matching/state/input from compact rendering. `PickerView`
supplies heading/source/context, maximum height and presentation. The view uses
semantic styles, rules and cell tokens, reserves key/current/mark columns,
aligns details, drops details first and truncates labels by grapheme. Opening
height stays stable while filtering; groups count toward paging. The gallery
owns placement and the independent draft, without another terminal loop.

Multiple-choice marks use ◉/○ in Unicode RGB/indexed profiles and `[x]`/`[ ]`
in ANSI 16, NO_COLOR and ASCII. Disabled rows carry `[-]` in the mark/shortcut
column and faded labels. NO_COLOR retains Unicode rules unless ASCII is selected
separately. Below 50 columns, the footer retains only Esc's current meaning.
The gallery retains a cancelled result as `→ cancelled`; hosts may supply their
own wording without converting cancellation into a choice.

Current picker presentation: labels explicitly reset foreground and
remove inherited dim styling; active enabled labels stay bold. The first blank
cell and final cell have default backgrounds outside the band. Number keys
occupy the middle cell of a three-cell block on every row, followed by one plain
band cell before the label. Detail separation uses its own
two-cell token, preserved before dropping the column. Gallery default placement
docks the measured opening height above a persistent host activity row and
message editor. Muted
unselected keys and bold focused headings are retained; contiguous shortcut sets
with two or more keys use a range, while disabled holes remain explicit. The
host activity prefix uses the warning token, with muted hints. The [visual follow-up review](../human-testing.md#picker-visual-follow-up--2026-10-09)
is independent of Dialog acceptance.

The compatibility index picker bounds search/labels to 4 KiB and strips
terminal control characters. Rich choice query and rendering follow the
contract above.

## Host boundary

Hosts supply choices, status and defaults, own placement/focus, refresh the
source, and recheck/execute returned intents. The picker starts no asynchronous
loading, remote search, filesystem operation or terminal loop. Its query editor
follows the [Editor contract](editor.md).

`notice()` exposes local bound feedback (`mark at least N` / `unmark one first`)
for the host's activity/status row. The title counts marks, host maximum and
hidden marks; neither feedback nor a highlighted row changes the confirmed set.

## Acceptance criteria

- **D-02:** Picker filters/pages variable rows by stable identity; disabled/empty/
  loading/error states cannot confirm; original-index compatibility is verified.

Gallery fixtures cover details, disabled/danger rows, filtering, no matches,
scrolling, multiple selection, groups, loading, error, empty and narrow views.
Hosts choose docked, centered-dialog or full-screen placement.

## Remaining gaps

Native inline hosting remains a [terminal feature](../features/terminal.md).
Visual follow-up has separate [review steps](../human-testing.md#picker-visual-follow-up--2026-10-09);
the earlier functional acceptance does not accept every later styling change.
