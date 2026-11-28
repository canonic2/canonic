# Give a local viewer input control

This handoff supports mobile targets and owned Firefox/WebKit pages. Testrun
provides the private host protocol; a rendered viewer UI is outside its scope.
Preview remains read-only: input uses a separate authenticated channel.

Connect to the target, then call `control_status({})`. Use its
`controlVersion` as `expectedVersion`:

```json
{"owner":"viewer","viewerId":"local-viewer","expectedVersion":1}
```

Pass that object to `control_acquire`. The
trusted native viewer host uses the returned channel metadata to authenticate
privately. Grant tokens do not pass through the agent. A read-only preview
cannot send input or acquire control by itself.

While the viewer controls input, agent input from every Testrun client is
refused with `CONTROL_NOT_OWNED`, and the device is not released for
idle time. Observation and separately owned previews/recordings continue. The viewer must keep its private
connection alive with heartbeats; disconnect or expiry releases input authority.
Input with an uncertain outcome is never replayed.

To resume agent input, read control status again and acquire with
`{"owner":"agent","expectedVersion":N}`; any client can reclaim input this way. To release input without ending
capture, use `control_release({"expectedVersion":N})`. If an active action
became uncertain, inspect the screen/status and disconnect/reconnect before
continuing. A cancelled handoff must be retried explicitly or disconnected.

CLI equivalents after connecting:

```sh
testrun control status
testrun control acquire viewer --viewer-id local-viewer --expected-version 1
testrun control release --expected-version 2
```

For Chrome, Firefox and WebKit, supply the browser `--target` and a complete page reference
with `control acquire viewer --page JSON`. MCP `control_acquire` takes the same
`page` object. The channel is bound to that exact owned page; closing it revokes
input. Omit `page` when acquiring agent control. Control status includes the
page, channel metadata and current `controlVersion`.

On Windows, the host uses native AF_UNIX streams at
the returned private paths. The same grant framing and input messages apply.

Browser hosts send `browser_input` messages with a UUID `commandId`, `page`,
fresh `observation`, and `actions` using the shared `action` discriminator.
Coordinates are CSS viewport pixels from fresh geometry or a mapped viewport
screenshot, or from a live frame that includes a verified `transform` and fresh
`inputObservation`. Handoff invalidates older observations. Held keys/buttons expire after
30 seconds of host time; uncertain releases remain quarantined for owned-page
closure rather than being repeated.

The mobile channel supports complete taps, swipes, keys, text and app actions.
