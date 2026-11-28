# Build, install and launch an app

Use `testrun build start` to produce a testable iOS or Android app with Xcode, Gradle,
React Native, Expo, Flutter or a custom command. Add `--install --launch` to run
the complete workflow on a target you select. Each stage has a retained outcome;
polling status never repeats a build or device mutation.

Install the project's existing build dependencies first. iOS native builds need
macOS and Xcode. Android package validation needs SDK Build Tools and
`ANDROID_HOME` or `ANDROID_SDK_ROOT`. Testrun does not install missing build tools
or project dependencies. Build-only use does not connect a device.

An optional `diagnostics` profile block records declared symbols, profileability,
sanitizers, coverage, probes and timing with the effective profile and resulting
artifact. It adds no compiler flags or automatic instrumentation; see
[diagnostic builds](diagnostic-interactive.md#declare-a-diagnostic-build).

## First build

Create `testrun.yml` in your app's project directory. `testrun config init`
creates an empty file and ignores `testrun.local.yml`; it preserves existing files.
For an Xcode simulator app, replace the empty `builds` map with:

```yaml
version: 1
builds:
  ios-dev:
    builder: xcode
    platform: ios
    target_kind: simulator
    cwd: ios
    xcode:
      workspace: Acme.xcworkspace
      scheme: Acme
      configuration: Debug
```

Run from the directory containing that file:

```sh
testrun config check
testrun config show
testrun build start ios-dev
testrun target list --compact
testrun build start ios-dev --install --launch --target simulator:SELECTED-UUID
```

`config show` redacts environment values, build properties and command arguments.
`build start` waits by default and returns the operation record, artifact and completed
stages. `--boot` explicitly permits starting the selected virtual target. Use
`--replace` to update an installed app without uninstalling it. Without that flag,
an already installed app is refused. The selected device stays connected for
testing, shared with every other Testrun client. Run
`testrun disconnect --target simulator:SELECTED-UUID` when finished; a device with
no use for ten minutes is released automatically.

Launch completion establishes native launch transmission/outcome, not that the
app rendered or its backend is healthy. Observe the app and use the normal QA
tools to check behavior. Debug builds may need Metro or Expo. Use the optional
[project workflow](projects.md) to inspect an existing Expo server or explicitly
start its declared command. Builders do not implicitly start persistent servers.

## Project and local settings

`testrun.yml` contains committed shared profiles. Optional `testrun.local.yml`
beside it contains ignored machine-specific settings:

```yaml
version: 1
builds:
  ios-dev:
    xcode:
      development_team: ACME000001
    env:
      API_URL: https://staging.example.com
```

Maps merge by key, lists and scalars replace, and omitted values retain the shared
setting. A local `env` or `launch.env` entry set to `null` removes that configured
binding. Replace `inherit_env` to stop inheriting a key. Profiles cannot change
their builder, platform or target kind through an overlay; create another profile.
A local-only profile must be complete. Both files declare `version: 1`.

Paths are relative to the selected project root; builder project files and
artifact selectors are relative to `cwd`. React Native's adapter enters its
`ios` or `android` directory underneath that cwd. Use `--project /path/to/app`
to choose another root. Testrun does not search parent directories. Duplicate
keys, aliases, unknown fields and unsupported versions are errors.

The files can also live in `.config/` (`.config/testrun.yml` with
`.config/testrun.local.yml`), used when the root has no `testrun.yml`; paths
inside them stay relative to the project root. `--config PATH` selects any other
base file, with its overlay read from beside it. See
[configuration file location](configuration.md#configuration-file-location).

Existing device QA and explicit-path installation work without these files.
They do not load build configuration unless you select a build profile.

## Builder reference

Every profile requires `builder`, `platform` (`ios` or `android`) and
`target_kind` (`simulator`/`device` for iOS, `emulator`/`device` for Android).
Common fields are `cwd` (default `.`), `env`, `inherit_env`, `timeout_ms`
(default 600000, range 1000–3600000), optional `artifact` and optional `launch`.
Include exactly the block named by `builder`.

| Builder | Settings |
| --- | --- |
| `xcode` | Exactly one `workspace` or `project`, plus `scheme` and `configuration`. Optional `sdk`, `build_settings` string map and `development_team`. Uses Xcode build output in a private operation directory. |
| `gradle` | `tasks` list and optional `properties` string map. Uses the project's `gradlew` with `--no-daemon`. Selects an APK below `build/outputs/apk`, or use an explicit artifact selector. |
| `react-native` | iOS: `configuration` and `scheme`, with exactly one native workspace or project. Android: `variant`, such as `stagingRelease`. Builds through the existing native project without running framework install/launch commands. |
| `expo` | `profile` in `eas.json` and project-local `node_modules/.bin/eas`. Uses `eas build --local --non-interactive`. Use a concrete profile without `extends`. Android must produce an APK. Simulator iOS needs `ios.simulator: true`; physical iOS needs an internal-distribution or development-client profile. EAS authentication and signing follow the project's EAS setup. |
| `flutter` | `mode`: `debug`, `profile` or `release`; optional `flavor`, `entry_point` and `dart_defines` string map. iOS simulator requires debug. Builds a native `.app` or standalone APK. |
| `custom` | Nonempty `command` argv list and an `artifact` selector. No shell expansion; explicitly invoke a script or shell if needed. The command must create or refresh its selected output. |

Expo projects with generated native folders can also use `gradle` or `xcode`
profiles. Prepare those folders and dependencies with the project's tooling,
then build them as native projects. EAS local builds require Expo authentication
and a configured Expo project.

An artifact selector is exactly one of `path: dist/acme.apk` or
`glob: app/build/outputs/apk/staging/release/*.apk`. Globs support `*`, `**` and
`?`, and must match exactly one package. Narrow the selector when multiple
flavors or architectures exist. Outputs must stay inside the project or owned
build output directory. Expo controls its own output path and refuses an explicit
`artifact` selector. EAS iOS archives are unpacked into a validated `.app`;
installation receives that app, not the archive. Android AAB/split package sets
are unsupported.

For example:

```yaml
version: 1
builds:
  android-staging:
    builder: gradle
    platform: android
    target_kind: emulator
    cwd: android
    gradle:
      tasks: [assembleStagingRelease]
    artifact:
      glob: app/build/outputs/apk/staging/release/*.apk
  flutter-staging:
    builder: flutter
    platform: android
    target_kind: device
    flutter:
      mode: release
      flavor: staging
      dart_defines:
        API_URL: https://staging.example.com
  custom-android:
    builder: custom
    platform: android
    target_kind: device
    custom:
      command: [pnpm, build:test:android]
    artifact:
      path: dist/acme.apk
```

## Environment and one-run overrides

`env` supplies build-process strings. `inherit_env` explicitly forwards named
variables from the invoking CLI. Missing requested variables fail before build.
Testrun supplies a small host baseline: `PATH`, `HOME`, `LANG`, `LC_ALL`,
`LC_CTYPE`, `TMPDIR`, `TMP`, `TEMP`, `DEVELOPER_DIR`, `SDKROOT`, `JAVA_HOME`,
`ANDROID_HOME`, `ANDROID_SDK_ROOT`, `FLUTTER_ROOT` and `PUB_CACHE`, when present.
The background manager's startup environment does not supply build variables.

```yaml
version: 1
builds:
  ios-dev:
    builder: xcode
    platform: ios
    target_kind: simulator
    xcode:
      project: Acme.xcodeproj
      scheme: Acme
      configuration: Debug
    inherit_env: [ACME_BUILD_TOKEN]
    env:
      API_URL: https://staging.example.com
    launch:
      args: [--show-test-menu]
      env:
        ACME_TEST_MODE: "1"
```

`launch.env`, `launch.inherit_env` and `launch.args` apply separately to the
launched app. They work on iOS; Android refuses generic process env/args. Build
variables do not automatically become launch variables. Flutter defines, Xcode
build settings and Gradle properties are separate from process environment.
Quote numeric or boolean strings. YAML does not interpolate `${...}` or load
`.env` files. Frameworks and custom scripts can apply their own environment rules.

One invocation can override supported settings:

```sh
testrun build start ios-dev --configuration Release --set-env API_URL=https://staging.example.com
testrun build start flutter-staging --mode debug --flavor staging
testrun build start ios-dev --install --launch --target simulator:SELECTED-UUID --launch-env ACME_TEST_MODE=1
```

Other override flags are `--variant`, `--entry-point`, `--timeout-ms` and repeatable
`--launch-arg`. Configuration/variant/flavor/mode overrides must match the builder.
A release build can use staging settings; `production` is a project profile name,
not a build optimization level. Prefer inherited values for secrets; command-line
values can be visible in shell history. Build logs are private but can contain
anything printed by project tools. Compiled-in values can remain in the app.

## Status, cancellation and retained artifacts

```sh
testrun build start ios-dev --no-wait --submission-id acme-ios-check-1
testrun build status build-RETURNED_REFERENCE
testrun build status --submission-id acme-ios-check-1
testrun build status --limit 20
testrun build cancel build-RETURNED_REFERENCE
testrun build log build-RETURNED_REFERENCE --limit 16384
testrun app install --artifact-ref build-RETURNED_REFERENCE --target simulator:SELECTED-UUID
```

Choose a unique submission ID before starting work when you need recovery after
a lost reply. Repeating the same submission with that ID returns its retained
operation, even after YAML changes; changed request inputs refuse. Without an
ID, each CLI invocation starts a new submission. `build status` without a selector
lists operation summaries in ascending build-ID order; use `--cursor` with the returned `nextCursor`
for another page. `build cancel --submission-id ID` also accepts the stable key.

Status and cancel do not start an absent manager. Cancellation stops the owned
build process group and waits for its child; an already dispatched device stage
settles before cancellation prevents
the next stage. Artifact traversal, extraction and copying check cancellation
cooperatively. CLI Ctrl-C requests the same cancellation. Inspect the retained
stage outcomes before repeating work after an interruption.

Each device stage waits its turn in the target's queue behind requests from
other clients. A `disconnect` from any client between stages releases the
target; a later launch stage is then refused with `NOT_CONNECTED` instead of
reconnecting it. Ending the host task does not stop a build or release its target.

Records include a private `logPath`; logs are capped at 8 MiB. Artifacts are
snapshots independent of subsequent project output changes. Records and snapshots
remain in private build storage until explicit operator cleanup. Missing artifact
files require a new build or explicit-path submission. A manager loss before the
terminal record is an unknown outcome, never an automatic retry. Testrun keeps
project files and shared build caches during failure/cancellation.

## MCP and configuration without files

`config_init`, `config_check`, `config_show` and `config_schema` take an absolute
`project` path; `config_schema` accepts `local: true` for the partial overlay.
`build_start` submits asynchronously and accepts an optional stable `submissionId`.
`build_status` and `build_cancel` select either the returned `operationId` or that
`submissionId`. Status without a selector lists history with optional `limit` and
`cursor`. CLI and MCP share the same build/configuration models.

```json
{
  "project": "/path/to/acme",
  "profile": "ios-dev",
  "install": true,
  "launch": true,
  "targetId": "simulator:SELECTED-UUID",
  "baseline": {"PATH": "/usr/bin:/bin"},
  "bindings": {"ACME_BUILD_TOKEN": "VALUE_SUPPLIED_PRIVATELY"}
}
```

MCP callers supply the needed baseline and requested inherited `bindings`
explicitly. There is no fallback to a long-lived MCP server's environment. Supply
`inline` containing a complete profile instead of `profile` to build without YAML.
`env` applies build env overrides; `overrides` accepts `configuration`, `variant`,
`mode`, `flavor`, `entry_point`, `timeout_ms`, `launch_args` and `launch_env`.
`app_install` accepts either `artifactRef` or an explicit absolute `path`.

Continue with [iOS testing](ios.md) or [Android testing](android.md) after launch.
