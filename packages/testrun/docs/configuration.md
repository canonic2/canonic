# Configure several projects

Start the project wizard from your repository:

```sh
testrun init
```

The wizard detects likely applications, lets you select them with Space and
Enter, confirms each framework and offers platform profiles. It prints the
complete proposed configuration before asking to create it. Cancellation writes
nothing. Existing configuration in `testrun.yml` or `.config/testrun.yml`, or
their `testrun.local.yml` overlays, is preserved and initialization refuses to
replace it; inspect it with `config show` or `project list`. Use
`--config PATH` to create a different file, as described in
[Configuration file location](#configuration-file-location).

Inspect detection without prompts or writes, or choose explicit applications:

Dry-run discovery also works when configuration already exists; it does not
merge or replace it.

```sh
testrun init --root . --dry-run --json
testrun init --root . --path apps/web --path apps/mobile --dry-run --json
testrun init --root . --path apps/web --kind vue --platform web --yes
testrun init --root . --path tools/device --kind acme/board --platform acme/firmware --yes
```

`--yes` confirms creation without terminal questions. Piped/JSON use requires
`--dry-run` or `--yes`. When automatic discovery finds nothing, use the manual
choice in the wizard or provide `--path`. Explicit paths stay inside the selected
root; `--kind` and repeated `--platform` values apply to those explicit paths.

Detection reads package dependencies, static HTML entry points, Flutter pubspecs, Xcode project/workspace
markers and Gradle/Android markers. It does not evaluate JavaScript config,
execute package scripts, install tools, build apps or contact devices. Generated
and hidden directories and symlinked trees/manifests are skipped. Automatic
scanning is bounded to six directory levels, 4096 entries, 128 candidates,
256 KiB per manifest and 4 MiB of manifest text. Warnings identify unreadable or
invalid manifests; incomplete discovery requires interactive review or narrower
explicit paths before unattended creation.

Framework detection is a suggestion. Platform folders and browser frameworks
provide starter profile suggestions; Xcode markers alone do not establish an
iOS or macOS target. Selected profiles contain platform/target-kind defaults,
which are visible in the review. Custom platforms start with target kind
`process`; adjust that field for the actual adapter. Builds, application IDs,
URLs, drivers and signing settings are configured separately. Configuration is
written as quoted JSON, which is valid YAML, and the local overlay is added to
`.gitignore`.

`init` creates project configuration. `mcp setup` registers agent integrations;
`signing setup` selects host signing preferences. `config init` remains the
noninteractive command for an empty version-2 file.

## Configure projects through MCP

Agents use the same discovery, starter profiles and create-only initializer as
the CLI wizard. No terminal, shell command or running device manager is needed.

Call `project_discover` to inspect candidates and the default configuration:

```json
{"root":"/path/to/acme","paths":["apps/web","apps/mobile"]}
```

Omit `paths` to scan the selected root. Discovery and initialization previews
work even when configuration already exists; creation preserves those files.

Call `project_init_preview` to preview the desired selection. Each entry selects an
existing in-root directory and optionally overrides its detected kind and
platform suggestions. An empty `platforms` array creates no profiles; omission
keeps the detected suggestions. Custom identifiers use the same schema:

```json
{
  "root": "/path/to/acme",
  "projects": [
    {"path":"apps/web","kind":"vue","platforms":["web"]},
    {"path":"apps/mobile","kind":"expo","platforms":["ios","android"]},
    {"path":"tools/device","kind":"acme/board","platforms":["acme/firmware"]}
  ]
}
```

The result includes the configuration, file actions, discovery evidence,
warnings, `canCreate`, any existing-file blocker and a `planDigest` for
`project_init`. The preview tool is read-only. Omit `projects` to preview the
complete detected selection. Both MCP initialization tools reject `dryRun`;
`project_init` requests creation and requires a matching plan digest.

After reviewing the proposal within the agent's authorized task, call
`project_init` with the same root and projects and the
exact returned `planDigest`. Creation verifies that digest against the resolved
configuration and root before writing. Changed selections return
`INIT_PLAN_CHANGED`; obtain a new preview before retrying. The digest binds plan
data and does not grant additional permission. Creation remains subject to the
host's configured approval policy.

Incomplete scans require narrower explicit project paths. Missing digests,
duplicate application paths, outside-root selections and unknown fields refuse
before file publication. Repeating creation reports existing configuration;
inspect it with `config_show` or `project_list` instead of replacing it.

## Configuration file location

Testrun reads configuration from the root you select (the current directory, or
`--root`/`--project`). It never searches parent directories. In that root it
uses the first of:

1. the file passed with `--config PATH` (MCP: absolute `config`);
2. `testrun.yml`;
3. `.config/testrun.yml`.

```sh
testrun config show                                # testrun.yml, else .config/testrun.yml
testrun config show --config qa/acme.yml           # this file, relative to your shell
testrun build start dev --app mobile --config qa/acme.yml
testrun project list --config qa/acme.yml
```

A present `testrun.yml` wins even when it is invalid; Testrun reports its error
instead of falling back to `.config/testrun.yml`. A `--config` file that is
missing or invalid fails the same way. `config show`, `config check`,
`project list` and `project show` report the selected file in `location.file`
and any default file it hides in `location.shadowed`.

The selected file's directory changes only where its overlay is read: the local
file is `testrun.local.yml` beside the selected base, such as
`.config/testrun.local.yml`, whatever the base file is called. Paths inside the
files stay relative to the configuration root, so moving `testrun.yml` into
`.config/` needs no path edits.

`config init`, `init` and `project_init` create root `testrun.yml` by default
and refuse when either default location already holds configuration. Pass
`--config` (MCP `config`) to create another file; the initializer adds that
file's own overlay to `.gitignore`, for example `/.config/testrun.local.yml`.
For a file outside the root, the entry goes into a `.gitignore` beside it.

## Project structure

Put the configuration in the root you want to select explicitly. Version 2 gives
each application its own source root, kind and profiles. Profile names are local
to an application, so several applications can have a profile named `dev`.

```yaml
version: 2
projects:
  website:
    root: apps/website
    kind: vue
    profiles:
      dev:
        platform: web
        target_kind: browser
        driver:
          adapter: firefox
  mobile:
    root: apps/mobile
    kind: react-native
    profiles:
      dev:
        platform: android
        target_kind: emulator
        build:
          builder: react-native
          react-native:
            variant: debug
  desktop:
    root: apps/desktop
    kind: flutter
    profiles:
      dev:
        platform: linux
        target_kind: desktop
        driver:
          adapter: acme/desktop
          options:
            session: development
```

`kind` describes the application's workflow. Recognized core kinds include
`web`, `vite`, `vue`, `react`, `react-native`, `expo`, `flutter`, `xcode`,
`studio`, `android-studio`, `gradle`, `native`, `cli`, `service`, `library`
and `custom`. These are examples, not an enumeration restricting configuration.
Kinds, platforms, target kinds and adapter IDs accept short identifiers or one
namespace prefix, such as `acme/board`. Platform values can include `windows`,
`macos`, `linux`, `ios`, `android`, `web` or a custom platform. Project and
profile names use lowercase letters, digits and hyphens, starting with a letter.

Declaring a platform or driver does not install an adapter or establish runtime
support. Named native builds currently use the existing iOS/Android Xcode,
Gradle, React Native, Expo, Flutter and custom builder paths. Other operations
report unavailable capabilities explicitly.

## Inspect and select

If Testrun reports unresolved Git conflict markers, resolve the conflict in
the named configuration file and rerun the command. The reported location is
the opening marker of the conflict.

```sh
testrun config check --root .
testrun config schema --version 2
testrun project list --root .
testrun project show mobile --root .
testrun project status mobile --profile dev --root .
testrun build start dev --app mobile --root .
```

Build commands resolve `cwd` relative to the selected application's root;
artifact paths remain relative to the build's working directory. Builds retain
the configuration root, project ID, profile and canonical application root.
Two names for the same application directory share its build lease.
Existing installation and launch flags retain their [native build](builds.md)
meaning and require an explicit compatible target.

Listing and showing configuration do not start the manager, discover targets,
launch processes or contact services. Listing reports an invalid application's
diagnostic without preventing inspection of independent applications. Config
check validates all projects; selection validates the chosen project, its shared
base and its references. Config show retains diagnostics for invalid entries.

MCP uses an absolute configuration `root` and a symbolic `project`:

```json
{"root":"/path/to/acme","project":"mobile","profile":"dev"}
```

Pass these selectors to `build_start` or `project_status`. Use `project_list`
with `{root, cursor?, limit?}` and `project_show` with `{root, project}`.
Listing defaults to 50 entries, with a maximum of 100 per page.

## Application identities and declarations

Projects can also declare `applications`, `tasks`, `checks`, `services` and
adapter-owned `options`. A profile can select an application variant, reference
ordered `prepare` and `preflight` tasks, map check aliases, reference services,
and declare opening behavior. A build is optional: browser, CLI, service and
hardware profiles need no mobile package or application identifier.

```yaml
version: 2
resources:
  native-libraries:
    path: libs
services:
  api:
    adapter: http
    execution:
      kind: external
    url: http://127.0.0.1:3000/
projects:
  mobile:
    root: apps/mobile
    kind: expo
    applications:
      staging:
        ios:
          app_id: com.example.acme.staging
    tasks:
      sync:
        command: [corepack, pnpm, run, sync]
        resources: [native-libraries]
        effects: [ios, tsconfig.json]
    profiles:
      dev:
        platform: ios
        target_kind: device
        application: staging
        prepare: [sync]
        services: [workspace/api]
        build:
          builder: expo
          expo:
            profile: development
```

Commands declare exact `command` argv, optional `cwd`, `env`, `inherit_env`,
`timeout_ms`, `effects`, `resources` and `expected_output`. Service execution
declares one owner: `command` with `run`, `external` with an optional reference,
or `box` with workspace/service references. Local service names are project-local;
`workspace/NAME` selects a shared service. Dependencies and resource references
are validated, including dependency cycles.

These declarations are currently available for configuration and inspection.
Version-2 task execution, shared-resource admission, service supervision/Box
delegation, readiness probes and project opening are not implemented. Named
builds refuse `--prepare` and unsupported build extension options rather than
ignoring them. Version-1 [Expo workflows](projects.md) continue to provide their
existing preparation, server, check, readiness and opening operations.

## Local settings and existing files

An ignored `testrun.local.yml` beside the selected base file uses the same version and may specialize
paths, endpoints, environment and command values. Maps merge, lists replace,
and null environment values remove a shared key. Project kind, platform,
target kind and builder identity cannot change through an overlay. Changing a
service execution owner replaces its entire execution block. An overlay cannot
repair an invalid shared project. Unknown core fields are rejected; adapter
extensions belong inside `options`.

Version-1 files and legacy selectors remain supported without rewriting either
file. Named discovery exposes their flat profiles under a synthetic `legacy`
project rooted at `.`. `--project PATH` keeps its historical directory meaning;
new `--root PATH` selects the configuration root. `config schema --version 1`
returns the historical schema. `config init` creates an empty version-2 file;
the top-level `init` wizard creates the reviewed project entries.
