---
name: ui-verification
description: Verify a reported mobile or browser UI issue with Testrun when each next step depends on what the screen shows, or when an interrupted action left an uncertain result. Not needed for a single screenshot or a routine tool call.
---

# Verify a UI issue with Testrun

Use this workflow when the user asks you to confirm or reproduce a UI problem
and the steps cannot be planned in advance, or when an earlier action was
interrupted and its effect is unknown. Ordinary discovery, observation and
input do not need it.

This guidance grants no extra authority. It does not widen the user's request,
skip a refusal or replace a step the user must perform on the device.

## 1. Fix the scenario and the target

State the scenario you are checking: the app or page, the starting state, the
steps and the expected result. Use the target the user named. When the target
is not settled, call `target_list` for devices or `browser_list` for browsers
and read the returned capabilities. Ask the user when more than one target
fits. Never pick the first entry, and never switch targets without saying so.

Lists accept `cursor` and `limit` (default 50, maximum 100). Follow the opaque
`nextCursor` with the same filters until null; `invalid_cursor` means restart
without a cursor. `ui_find` cursors belong to the observed tree and expire when
it changes. `ui_snapshot` pages shallow nodes with document paths; its `limit`
counts bytes (default 16384, maximum 65536), so follow every page before treating
a missing node as absent. Browser `events_read` uses `cursor` and `limit`;
`diagnostics_read` places them inside `query`. Both return nullable `nextCursor`
and a ready-to-send `nextRequest` when more observations may remain.

## 2. Connect and read the current state

Use the selected connected target directly. Call `connect` only if needed.
Use its screenshot when it shows the current device state. Call `observe`
when the view is missing or stale. Respect the target's capabilities, control
path, focus scope and page selection. Get coordinates from a current
observation of that target.

## 3. Act in bounded batches

Send one `act` batch for steps you have already decided. When the next step
depends on what the screen will show, end the batch there. Read the resulting
observation, decide, then send the next batch. Do not pack guessed or
conditional steps into one blind batch.

Use `ui_find` or `wait` to confirm that an element or state appeared, rather
than assuming a fixed delay was enough.

## 4. Recover from interruptions without repeating input

When a call times out, is cancelled or returns an unknown outcome, the input
may or may not have reached the target. Call `status`, using the returned
`actionId` for browser work, and wait until the operation has settled. Then
`observe` the current screen before choosing the next action.

Never resend an action whose effect is unknown, and never switch to another
input method as a hidden retry. If the state still cannot be established, stop
and report that to the user.

## 5. Report what the evidence shows

Report:

- what you observed, with the observations or recordings that show it;
- which requested steps completed and which did not run;
- what remains unknown, and why;
- whether the expected result appeared.

An accepted action, a screenshot or a recording does not prove the expected UI
change by itself. Name the observation that shows it.

## 6. Finish without unrequested cleanup

Do what the user asked with the target and any recordings when you are done.
Connected targets are shared with other agents and clients, not owned by this
conversation. Do not `disconnect` a target only because your turn ended. Do not
stop a recording you did not start, reset settings or change the network as
automatic cleanup.
