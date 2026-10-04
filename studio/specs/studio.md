# Studio MVP

This overview defines the release contract. The [specification index](README.md)
links to detailed engineering specs for each subsystem, including setup,
ownership, lifecycle, failure behavior, and proposed next work.

## Product contract

Studio hosts several open Git projects as tabs, with task sessions inside each.
A session owns a linked worktree, retained Code OSS IDE, host process group,
allocated ports, and optional Compose project. Switching projects or sessions
must preserve their other running workspaces.

The MVP provides repository opening, session creation, project/session selection,
runtime start/stop, previews, logs, and project closing/reopening. The interface is
deliberately minimal. Workbench and Shield integration are subsequent work.

## Module boundaries

The application composes feature modules for projects, sessions, Git, state,
configuration, IDEs, and runtimes. Domain modules have no Electron dependency.
Transport and native views belong to app/desktop; presentation belongs to the
renderer. The [development guide](../docs/development.md) maps source ownership.

Feature modules separate application wiring, services, validation, and UI using
plain Node modules.

State writes serialize and replace state.json atomically. Updates work on a copy
and install it only after persistence succeeds. Session creation registers rollback
of its newly created worktree and branch if persistence fails. Reads return copies.
Persisted state validates project/session identity and selected ownership.

Runtime and IDE operations queue independently per session. Starting one project's
dependencies cannot block another session. Duplicate starts reuse the existing
owner. Closing a project prevents new work, waits for its queued operations,
stops its processes and views, and marks the tab closed without deleting files.
Reopening the same repository restores its identity and sessions.

## Configuration and isolation

Each checkout supplies a versioned studio.config.json. The shipped JSON Schema
owns accepted properties, types, and defaults. Configuration validation precedes
provisioning and launching host commands. Reserved identity variables, ambiguous
ports, unknown keys, and invalid timeouts fail with actionable errors.

The runtime optionally provisions the checkout, starts an isolated Compose stack,
waits for container health, and starts the host supervisor. Its primary readiness
endpoint must return the session identity. Turbo is the tested supervisor for
Express and Vite; other tools supply their own commands and assigned-port handling.

Compose resources use studio_<project-id>_<session-id> names. Published ports bind
loopback and use assigned values. Validation rejects external resources, shared
names, fixed container names, privileged services, network mode overrides, and
bind mounts outside the checkout. Studio captures the resolved Compose plan so
configuration edits cannot redirect cleanup. Stop preserves named volumes; test
teardown removes its fixture volumes.

Each session owns IDE settings, extensions, browser storage, and checkout.
Production uses workspace trust and a built-in Code OSS theme. Project IDE settings
initialize defaults once; later user changes persist. Invalid runtime configuration
does not prevent editing it in the IDE.

code-server IPC uses a private, short temporary directory, independently of the
data-directory length. macOS rejects overly long Unix-socket paths; the adapter
removes its socket directory on normal stop and startup failure.

## Acceptance checks

- Node tests cover real branch/worktree isolation, persistence, rollback,
  project close/reopen, parallel startup, independent runtime stop/restart,
  descendant process cleanup, and strict configuration.
- The IDE smoke test verifies a real workspace extension edits and saves a file,
  runs an integrated terminal in that checkout, installs a theme, retains views
  across project/session switching, and serves the edited preview.
- Full-stack tests run two distinct Turbo/Express/Vite environments and healthy
  Redis Compose stacks, with six host ports, separate networks and volumes,
  session-owned proxy routing, counters, live edits, stop/restart persistence,
  configuration edits during runtime, failed-start cleanup, and complete teardown.
- The desktop full-stack test additionally verifies two real IDE views,
  frontend counter clicks, Vite HMR, Express watch restart, and shell navigation.
- Test fixtures, verification extensions, and test runners are under test/ and
  are unreachable from production startup. Screenshots are optional compositor
  evidence; DOM, API, Git, and Docker checks remain required.

## Current limits

The MVP assumes trusted local repositories and commands. Worktrees share Git
objects and refs; they do not provide security isolation. Code OSS through
code-server supplies the IDE; arbitrary VS Code extensions require compatibility
testing. The supported runtime targets are macOS and Linux x64/arm64, with desktop
verification on the current macOS host.

Session deletion, branch cleanup, migrations, per-service controls, continuous
health supervision, automatic crash recovery, and a global unsaved-file quit
flow are not implemented. Ports are probed rather than reserved; bind conflicts
fail instead of retrying. IDE servers and retained views consume resources per
opened session. Runtime configuration remains a trusted executable contract.
