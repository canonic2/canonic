# Troubleshooting

Use these checks to identify the installed build, confirm target readiness, and
recover an interrupted operation. For a failure inside the app you're testing,
choose a [diagnostic workflow by symptom](diagnostic-workflows.md).

## The terminal or agent uses the wrong build

Run `testrun --version` and `testrun <command> --help` in the environment that
will run Testrun. If the command cannot be found, check that its installation
directory is on `PATH`. Project-scope MCP registration uses the bare `testrun`
command and user-scope registration uses its recorded absolute path. After
installing a new build, restart the agent or MCP server. Use
[MCP setup](mcp.md#agent-setup) to inspect or repair the registration.

The offline guides from `testrun docs list` belong to the installed build. If
the terminal finds a new binary but the manager still runs the old one, check
`testrun manager status` and follow
[Update an existing installation](distribution.md#update-an-existing-installation).

## The device is missing or unavailable

```sh
testrun target list --compact
testrun doctor --target TARGET_ID
```

Copy the exact physical iPhone or Android target ID for `doctor`. A successful
diagnostic reply can still report `ready: false`. Read the failing check and its
next action. On Android, confirm adb can see the target as `device` and check for
an authorization prompt. On iOS, check pairing, lock state and developer
support. For an absent or stopped simulator, use
[iOS setup](ios.md#setup-and-readiness) to check the runtime, boot state and
simulator set. For a stopped Android emulator, follow
[Start an emulator from an AVD](android.md#start-an-emulator-from-an-avd).

## The browser does not start

Run `testrun browser doctor --engine chrome`, or substitute `firefox` or
`webkit`. Check `hostSupported`, the available launch modes and `missing`
dependencies. A first connection prepares the missing private archives. On
Windows, use the active console session for a headed launch, or use a managed
headless launch from an SSH service session. See [browser setup](browser.md).

If you have a `browser:` target from the connect request, inspect its
`preparationOperation`, `startupOperationId` and `failure` with
`testrun browser status --target BROWSER_TARGET`. Use
`--operation-id STARTUP_ID` to inspect the retained startup result. After a
failed start, disconnect that target before making a new connection. If you
lost the connect reply, list browser connections with `testrun browser status`
to find it before retrying.

## An action timed out or cleanup is pending

- For a device action, use `testrun status --target TARGET_ID --action-id ACTION_ID`
  to inspect the retained outcome. If the outcome is uncertain, observe the
  current screen before deciding what to do next.
- For a browser action, poll its returned ID as shown in
  [Browser targets](cli.md#browser-targets). `WAIT_EXPIRED` with `settled: false`
  means the wait ended while work was still pending. Poll the same ID.
- For `CLEANUP_FAILED` or `RELEASE_PENDING` on a device, inspect
  `testrun manager blockers`, address the reported cause, and use
  `testrun manager retry --target TARGET_ID` to retry that release.
- For `CLEANUP_PENDING` on a browser, the manager retries. Run
  `testrun browser disconnect --target BROWSER_TARGET` to retry that browser's
  cleanup immediately. See [MCP and cleanup](browser.md#mcp-and-cleanup).

## Diagnostic evidence is missing or incomplete

Start with `testrun diagnostics capabilities` and check the selected source's
host, tools and target binding. A partial result can contain usable evidence
and gaps. Compare `phase`, `settled`, and `reason` with the
[capture result example](debugging.md#read-a-capture-result) before collecting again.
Use [Diagnostic access](diagnostic-access.md) to understand scope,
prerequisites and result limits, then choose the source-specific guide from
the [documentation index](README.md#platform-and-framework-diagnostics).
