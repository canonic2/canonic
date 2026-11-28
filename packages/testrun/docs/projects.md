# Prepare an Expo project for device testing

For several applications in one repository, see [project configuration](configuration.md).
This guide describes the version-1 Expo workflow.

Associate your app and Expo server with a build profile to inspect readiness
through Testrun. You can inspect an existing server, run declared preparation
or checks, and start an owned server without constructing HTTP requests or
parsing native app inventory yourself.

Install project dependencies and configure your existing [build profile](builds.md)
first. The example below assumes an Expo development client, a concrete EAS
`development` profile and a project-owned `prepare:mobile` package script that
synchronizes local libraries and generates any required native files. Replace
those project-specific names and commands with your own.

## Associate the app and server

In the app directory's `testrun.yml`:

```yaml
version: 1
builds:
  ios-dev:
    builder: expo
    platform: ios
    target_kind: device
    expo:
      profile: development
    development:
      app_id: com.example.acme.staging
      variant: staging
      expo:
        url: http://127.0.0.1:8081/
        slug: acme
      preflight:
        command: [corepack, pnpm, --version]
        expected_output: "11.27.0"
        timeout_ms: 10000
      prepare:
        command: [corepack, pnpm, run, "prepare:mobile"]
        effects: [ios, tsconfig.json]
      server:
        command: [corepack, pnpm, exec, expo, start, --dev-client, --port, "8081"]
        timeout_ms: 3600000
        effects: [.expo, tsconfig.json]
      checks:
        types:
          command: [corepack, pnpm, exec, tsc, --noEmit]
```

Use your project's required package-manager version for `expected_output`. It
compares the trimmed combined command output exactly; output differences fail
the task rather than disable the project's version checks. A successful outer
version check does not establish the tool version used by a nested build script.

`app_id` identifies the expected installed app and the build artifact. `variant`
is descriptive metadata; supply variant-specific settings through the ordinary
build environment or the selected project command. `slug` checks the Expo
manifest's project identity. When a manifest exposes an application ID, Testrun
also compares it with `app_id`.

All task `cwd` values are relative to the selected project root, defaulting to
`.` independently of the build profile's `cwd`. Put machine-specific endpoint
or connection settings in ignored `testrun.local.yml`. Use an explicit project
path when running from another directory; Testrun does not search parents.

## Inspect before building

Run from the directory containing `testrun.yml`:

```sh
testrun config check
testrun project status ios-dev
testrun --target device:SELECTED-UDID project status ios-dev
```

Status returns the effective configuration with commands/environment redacted,
source provenance, the configured app identity, Expo diagnostics and optional
app inventory for the selected target. Checks distinguish ready, blocked,
unknown and not checked. Runtime status does not execute project commands,
start Expo, install packages or build. Configuration check/show do not contact
the server or a device. Project status does not start an absent manager or renew
a connected device's idle timeout. Without a manager, recent-operation summaries
are empty; retained individual operations remain available through build status.

A successful manifest response establishes host reachability. It does not
establish that the selected phone connected, loaded the current JavaScript or
rendered the app. A loopback connection URL intended for a physical iPhone is
reported blocked. Missing app inventory is unknown; a successful inventory with
no matching app reports the app absent. Missing Metro attachment alone does not
justify rebuilding.

The adapter requests the selected platform's JSON manifest with a five-second
HTTP deadline and a 1 MiB response limit. It does not follow redirects or fetch
advertised bundle URLs. Credential-bearing URLs are refused or omitted, and
bundle URLs containing query parameters are omitted from the ordinary result.
Unsupported protocol responses and project identity mismatches are explicit.

## Run preparation, checks or a build

```sh
testrun project start ios-dev --task preflight --submission-id acme-preflight-1
testrun project start ios-dev --task prepare --submission-id acme-prepare-1
testrun project start ios-dev --task check --check types --submission-id acme-types-1
testrun build start ios-dev --prepare --submission-id acme-build-1
```

Project start returns a retained operation immediately. Inspect it with
`build status OPERATION_ID`. Reusing a submission ID with the same inputs
recovers its outcome rather than executing the command again. Use a new key
for new work. `--prepare` runs the declared preparation and then its optional
preflight before building. Without that flag, a build does not run development
tasks. Existing install/launch/replace flags keep their [build semantics](builds.md).

Each task has exact argv, an explicit environment inherited from the build
profile, a working directory and a deadline. MCP callers supply baseline and
inherited bindings just as they do for builds; CLI captures its documented
client environment. The manager's startup environment is not substituted.
Testrun does not alter the project's package-manager settings or install missing
tools implicitly.

`effects` lists relative project paths to inspect around execution. Results
report observed changes without restoring files or attributing concurrent edits
to the task. Fingerprinting is bounded to 1024 entries and 16 MiB of file data;
symlinks and exceeded bounds report incomplete inspection. Effects are an
inspection list, not a filesystem sandbox for the project command.

## Inspect logs and stop owned work

```sh
testrun build status build-RETURNED_REFERENCE
testrun build log build-RETURNED_REFERENCE --limit 16384
testrun build cancel build-RETURNED_REFERENCE
```

Logs become readable while a process runs. Pass opaque `nextCursor` to `--cursor`
for the next read; finished output returns null, while live output reports
`running` and retains a cursor at its current end. Each response contains at
most 65536 bytes and an operation log
is capped at 8 MiB. Logs are private and may contain anything printed by project
tools. Exceeding the log cap stops the owned command with `OUTPUT_LIMIT`.
Status includes stage, elapsed time, log size and last-output time when
available. An opaque script remains one stage; Testrun cannot infer its internal
compiler progress. Cancellation stops owned execution and retains earlier outcomes.

To start the declared server:

```sh
testrun project start ios-dev --task server --submission-id acme-server-1
testrun project status ios-dev
```

The server is a retained operation with its own deadline, up to one hour. Stop
it with `build cancel` using its operation ID. Server and build leases are
separate so a server can run while building. Active servers count toward the
manager's four-operation limit. Testrun refuses to start when the configured
endpoint already responds or cannot be safely classified. It never stops an
existing server merely because you inspected it.

For a service managed by Box or other tooling, omit `development.server` and set
`development.expo.owner` to its service reference. Use that tool to start, stop
and read service logs. Testrun inspects the endpoint; it does not delegate service
commands automatically.

## Open the development client

An explicit `development.expo.connection_url` overrides the candidate `exp://`
URL derived from the manifest's `hostUri`. Use the URL expected by your installed
development client, including its custom scheme when necessary. A custom client
link can carry one `url` parameter containing a credential-free HTTP(S) URL.
On Unix, physical-device checks with no usable candidate also return active host
interface candidates, without assuming a name such as `en0`. Multiple interfaces
remain ambiguous. These addresses establish neither server binding nor phone
connectivity; Testrun does not change LAN, tunnel or forwarding modes. On other
platforms, configure the connection URL explicitly.
Set optional `connection_mode` to `lan`, `tunnel` or `forwarded` to record your
selected mode. `forwarded` requires an explicit `connection_url` and permits a
loopback candidate for an intentionally forwarded device route; it does not
establish that forwarding is active.

For a connected simulator or Android target:

```sh
testrun --target simulator:SELECTED-UUID project open ios-dev
```

This validates the manifest, queues app launch and URL opening through ordinary
device actions, and returns their outcome. Physical iOS generic URL opening is
unavailable: open the reported URL manually in the intended development client,
then use `observe` to inspect the phone. App launch or a host manifest response
does not certify rendering, native scrolling or accessibility.

MCP equivalents are `project_status({project, profile, targetId?})`,
`project_start({project, profile, task, check?, submissionId?, ...buildInputs})`
and `project_open({project, profile, targetId})`. `task` is `preflight`, `prepare`,
`server`, `check` or `build`; only `check` takes a check name, and only `build`
accepts preparation and device-install flags. All project paths in MCP are absolute.
Use `build_status`, `build_log` and `build_cancel` for retained operations.

Continue with [iOS testing](ios.md) or [Android testing](android.md) to verify
the actual app. Project checks and [browser captures](browser.md) retain their
own scope; native checks require native evidence.
