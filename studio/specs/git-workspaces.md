# Git workspaces

## Concurrent checkout strategy

Use Git linked worktrees to check out multiple branches of one repository at
once. Each task gets a separate working directory and index while sharing the
repository's object database and refs. This avoids copying the full Git history
and lets normal Git tooling operate inside each task checkout.

The original checkout becomes the unmanaged Main session. Studio never changes
its branch as part of creating a task. A task starts from committed content;
uncommitted edits, untracked files, ignored dependencies, and local environment
files in Main are not copied into the task.

## Creation contract

The application accepts a project, task name, and base ref, defaulting to HEAD.
HEAD resolves in the original repository checkout, not the selected task. Use
an explicit branch or commit to start from another task's committed work.

1. Validate the name and base input and assign a session ID.
2. Resolve the base using
   `git rev-parse --verify --end-of-options <base>^{commit}`.
3. Choose branch `codex/studio/<name-slug>-<session-id>` and checkout
   `<data-root>/worktrees/<project-id>/<session-id>`.
4. Run `git worktree add -b <branch> <checkout> <resolved-commit>`.
5. Register rollback, add the session to copied application state, and persist.
6. Publish the session and selection after persistence succeeds.

The slug uses lowercase alphanumeric characters and hyphens, falling back to
task if empty. The ID distinguishes tasks with the same label. Git commands use
executable/argument arrays through execFile, not shell interpolation. Resolving
the commit first fixes the chosen base if its ref moves during creation.

Studio creates a fresh branch for every managed session. It does not attach an
existing branch or override Git's restriction on checking out the same branch in
multiple worktrees. Branch collisions and invalid refs fail through Git; they
must not produce a successful session record.

## Provisioning and normal use

Dependencies belong to the task checkout. A provisioning command installs them
from its lockfile. A shared writable node_modules directory would let one branch's
installation change another's environment and is not part of this contract.

Developers commit, diff, merge, and push using ordinary IDE/terminal Git tooling.
Studio has no custom merge, push, pull, or branch-sync workflow. Commits update
shared refs; another worktree's files do not change merely because a commit exists.

Hooks, credentials, repository configuration, and refs may be shared. Worktrees
provide trusted development concurrency, not separate permission domains. Tools
that derive paths from Git metadata must handle a .git entry that is a file.

## Failure and retention

If persistence fails after creation, registered rollback removes the newly created
worktree and deletes its new branch. Cleanup is scoped to those resources;
failures are aggregated with the original error. It is not a general cleanup
routine for existing branches.

Runtime stop, project close, and app quit retain task worktrees and branches.
Session deletion and branch cleanup are not implemented. Do not use automatic
git clean, forced removal, or repository-wide pruning to hide lifecycle errors.

There is no durable transaction spanning Git and state.json. A hard crash after
Git creation and before persistence can leave an unregistered worktree. Git
creation can also fail before rollback is registered. Reconciliation is needed
before claiming crash-safe creation.

## Proposed next work

Provide a base picker with an explicit rule for fetching remote refs. Existing
branch import needs ownership and already-checked-out handling. Dirty checkout
copying, if offered, needs explicit ignored-file and secret handling decisions.

Before removal, compare persisted ownership with git worktree list and inspect
dirty/untracked files. Let the developer retain the branch. Recovery should offer
adoption or cleanup of verified orphan worktrees without inferring ownership
solely from a branch prefix.

## Source and acceptance

Owners: [Git adapter](../src/modules/git/index.ts) and
[session creation](../src/modules/sessions/index.ts).

Acceptance requires simultaneous task branches, independent edits and indexes,
an untouched Main checkout, invalid/option-like ref rejection, and rollback after
state write failure. Full-stack testing must run from real worktrees rather than
two copies of one directory.
