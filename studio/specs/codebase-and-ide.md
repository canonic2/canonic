# Codebase and IDE architecture

## Organization

Application wiring, services, validation, platform adapters, transport, and
presentation have distinct responsibilities. Studio uses TypeScript ESM modules
and a small Electron shell.

NodeNext compilation preserves ESM semantics and rewrites relative .ts imports.
The renderer and Electron entry compile to JavaScript; the .cts preload compiles
to CommonJS for the sandbox. Node 24 executes development scripts/tests directly.
Feature-owned types describe persisted state, normalized recipes, runtime owners,
and snapshots. The shared desktop request contract types the preload/renderer
bridge, while incoming IPC payloads still require runtime validation.

```text
renderer -> validated desktop commands -> Studio application facade
  -> projects / sessions / state / git
  -> runtimes / configuration / platform processes
  -> IDEs / configuration / platform processes
desktop workspace views -> session IDE or runtime HTTP origin
```

| Module        | Owns                                                | Delegates                                     |
| ------------- | --------------------------------------------------- | --------------------------------------------- |
| application   | Composition, cross-module close/shutdown, snapshots | Resource operations and persistence           |
| projects      | Registration, tabs, selection                       | Repository resolution to Git; writes to state |
| sessions      | Task identity and workspace creation                | Git operations and persistence                |
| git           | Discovery, ref resolution, worktrees                | UI decisions and runtime lifecycle            |
| state         | Loading, copied atomic updates, queue, rollback     | Resource-specific policy to callers           |
| configuration | Recipe/schema/defaults and validation               | Execution to runtime/IDE                      |
| runtime       | Queues, host/Compose ownership, readiness, cleanup  | Process primitives to platform                |
| ide           | Code OSS processes, profiles, extensions, sockets   | Native views to desktop                       |
| platform      | Paths, environment, queues, process groups          | Product policy to modules                     |
| desktop       | Electron, views, menus, validated IPC               | Application behavior to Studio                |
| renderer      | Controls, selectors, errors, action orchestration   | Files/processes through desktop commands      |

Domain modules do not import Electron or UI code. Projects/sessions do not launch
containers or IDEs. The facade coordinates modules instead of reimplementing them.
Compose policy is separate from subprocess execution. The
[development guide](../docs/development.md) maps actual source paths.

## Concurrency and transport

State writes serialize across projects; runtime and IDE queues are per session.
Slow provisioning must not block another session's startup. State callbacks work
on copies and cannot publish before persistence succeeds.

Desktop commands accept bounded validated payloads and known actions. Studio IPC
is restricted to the trusted shell frame. Its preload is sandboxed, context-isolated,
and has no Node integration. Embedded IDE/Preview views receive no Studio bridge.

Views are retained by session/mode. Selection detaches/attaches views with generation
checks for asynchronous navigation. Restarted preview origins update navigation
policy. External HTTP links use the desktop shell; embedded content cannot create
arbitrary native windows.

## IDE contract

Electron embeds code-server's Code OSS interface for editing, file navigation,
Git, terminals, themes, settings, and compatible extensions. Studio owns project/task
navigation and runtime controls; it does not implement an editing engine or VS Code
extension host.

Each session has its own IDE process, loopback port, user-data directory, extensions
directory, and persistent Electron browser partition. The IDE opens its checkout;
terminals inherit the workspace/session context. Settings and extensions can differ
between tasks.

Defaults use Default Dark Modern, disable telemetry/update checks, and enable workspace
trust. Recipe settings initialize the profile once. Invalid runtime configuration
is reported while the IDE can open with defaults to fix it.

Readiness checks the process's own HTTP-listening output and healthz. Private IPC
uses a short temporary Unix socket path independent of data-root length, because
socket paths have stricter limits. Stop/startup failure removes that directory.

The IDE is authentication-free on loopback for local use, not a remote deployment
contract. Native/proprietary VS Code extensions require compatibility testing in
this environment. Views survive selection changes; restoring every editor tab after
IDE restart is not guaranteed because the origin can change.

## Production and tests

app/main.ts composes production startup. Fixtures, verification extensions, themes,
and runners stay under test and are unreachable from production startup. Tests can
reuse desktop bootstrap with a supplied application; production must not branch on
smoke flags. Package checks enforce key import boundaries.

## Proposed next work

Workbench and Shield should use explicit application services and session context:
project identity, checkout, runtime endpoints, and lifecycle. Their UI belongs in
presentation. There is no generic product plugin API yet; define it from real needs
before committing a public contract.

Alternative shells/IDE packaging should preserve the application ownership model.
Shared extensions/settings and idle suspension require precedence and retention
rules; current profiles are session-specific.

## Source and acceptance

Owners: [facade](../src/application/studio.ts),
[IDE adapter](../src/modules/ide/index.ts),
[bootstrap](../app/desktop/bootstrap.ts),
[commands](../app/desktop/commands.ts), and
[views](../app/desktop/workspaces.ts).

Acceptance requires real extension edit/save, terminal context, theme activation,
profile isolation, retained views, malformed recipe repair, and production/test
separation.
