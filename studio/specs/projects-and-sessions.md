# Projects and sessions

## Model and identity

A project is a registered Git repository, identified by its canonical absolute
repository path. It owns a display name, project ID, open/closed tab state,
sessions, and its last selected session ID. Opening a folder inside a repository
resolves to the repository root. Opening it again selects or reopens the existing
project record.

A session is a task workspace inside exactly one project. It has an ID, name,
recorded branch, absolute checkout path, and managed flag. IDs currently use eight
characters from a UUID and remain stable in persisted state. Resource names use
IDs rather than display names, so labels do not determine Docker identity or
filesystem ownership.

Every new project begins with **Main**, an unmanaged session using the original
checkout. Additional sessions are managed worktrees. Main is a session name,
not a promise that the repository's branch is named main.

## Ownership

| Resource                                      | Owner                    | Lifetime                                             |
| --------------------------------------------- | ------------------------ | ---------------------------------------------------- |
| Repository registration and tab               | Project                  | Retained when the tab closes                         |
| Original checkout                             | User; referenced by Main | Never removed by Studio                              |
| Managed checkout and new branch               | Task session             | Retained across stop, close, and quit                |
| IDE server and native workspace view          | Session                  | Started on demand; retained across selection changes |
| IDE settings, extensions, browser storage     | Session                  | Retained across IDE restarts                         |
| Host runtime process group and assigned ports | Session runtime          | One running start/stop cycle                         |
| Compose containers and networks               | Session runtime          | Created on start, removed on normal stop             |
| Compose named volumes                         | Session                  | Retained on normal stop                              |

Git objects and refs, the host environment, and the Docker daemon are shared.
A session must scope mutable application data, temporary files, databases, and
caches appropriately; Studio cannot infer every path used by arbitrary programs.

## Selection and lifecycle

The application has one active project. Each project separately remembers its
active session. Selecting a session validates that it belongs to that project.
Selection changes presentation; it does not stop runtimes, delete files, or
recreate retained IDE views.

Creating a session validates a trimmed name of 1–80 characters, creates the Git
workspace, persists its record, and selects it. It does not automatically start
the application runtime. The desktop opens the selected IDE on demand.

Runtime and IDE lifecycles are independent. Stopping a runtime leaves the IDE
available. Opening an IDE does not execute the runtime recipe. IDE terminals
can launch user commands independently; the runtime manager tracks only the
configured command and its descendants.

Closing a project blocks new application operations for that project, waits for
pending state changes, and attempts to stop every session's runtime and IDE. A
cleanup failure prevents marking the project closed and is reported. Successful
closing removes the tab and workspace views. Another open project becomes active
if needed, or the application returns to its empty state.

Reopening restores project/session IDs and the selected session, using retained
checkouts and settings. Runtimes must be started explicitly. App shutdown stops
Studio-owned processes without marking all project tabs closed. Save editor
changes before closing; a global unsaved-file flow is not implemented.

## Persisted state

`<data-root>/state.json` stores version 1 metadata: projects, sessions, selection,
and tab state. It does not store live PIDs, readiness, runtime ports, or a durable
resource ownership journal. Runtime status is derived from in-memory records.

State updates serialize through one queue and operate on a copy. The store writes
state.json.tmp with mode 0600 and renames it over state.json before installing the
new in-memory state. Failed persistence triggers registered rollback callbacks.
Loading checks identities, paths, and selected ownership; malformed or unsupported
state produces an error rather than silently resetting the project list.

Typical data layout:

```text
<data-root>/
  state.json
  worktrees/<project-id>/<session-id>/
  sessions/<session-id>/
    ide/User/settings.json
    extensions/
    config/runtime/compose.json
    data/
    cache/
  desktop/
  logs/
```

The IDE profile is distinct from application data in the checkout. Directories
are created as needed rather than all being populated during registration.

## Proposed next work

Define archive, remove, and reopen as separate operations before adding session
deletion. Removal needs dirty/untracked-file checks, explicit branch retention
choices, and a separate choice for deleting container data. Main must remain
protected from managed-worktree removal.

Add reconciliation for missing checkouts and externally changed branches. The
recorded branch is metadata, not continuously refreshed Git status. Define idle
IDE suspension and resource limits without making tab switching implicitly stop
task runtimes. Unsaved-editor handling needs IDE cooperation.

## Source and acceptance

Owners: [projects](../src/modules/projects/index.ts),
[sessions](../src/modules/sessions/index.ts),
[state](../src/modules/state/index.ts), and
[application coordination](../src/application/studio.ts).

Acceptance requires canonical-path deduplication, independent project selection,
copied snapshots, close/reopen with the same IDs and files, cleanup scoped to the
closed project, and persistence failure without publishing partial session state.
