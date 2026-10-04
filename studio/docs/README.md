# Using Studio

Follow the [setup instructions](../README.md#run). Click **Open project** and
select a Git repository. Its existing checkout appears as Main. Editing Main
changes that repository's files.

## Projects and sessions

Project tabs switch repositories. The Sessions list switches tasks within the
selected project. Each project remembers its selected session. Switching retains
other workspaces, terminals, and running environments.

Click **+** beside Sessions, enter a task name and a starting branch or commit,
then click **Create session**. Studio creates a new `codex/studio/…` branch and
linked Git worktree. `HEAD` refers to the original checkout. Use another
session's branch explicitly to start from its commits. Uncommitted and ignored
files are not copied.

The IDE contains a file explorer, editor, integrated terminal, Git tools, themes,
and extensions through code-server's Open VSX integration. Each session owns its
IDE settings, extension directory, and browser storage. Desktop VS Code extension
compatibility varies; verify required extensions in Studio.

## Run an environment

Add [studio.config.json](configuration.md) to the project's repository and commit
it so new worktrees receive it. Click **Start runtime**. Studio provisions that
checkout when configured, starts its Compose stack, then launches its host
command. Each session receives distinct ports and its own session identity.

**Code**, **Preview**, and **Logs** select the IDE, the primary frontend, and
captured runtime output. **Stop runtime** stops that session's host process group
and Compose containers. Other sessions keep running. Named volumes survive
ordinary stop/restart. Port details are available on the endpoint's tooltip.

Click the **×** on a project tab to stop its sessions and close it. Opening the
same repository restores its sessions and selected task. Closing preserves all
files, branches, volumes, and settings.

## Reopen Studio

Quit stops owned IDE servers and supervised environments. Saved files and
project/session selections persist. Scripts require an explicit start after
reopening; open a workspace to launch its IDE. Save editor changes before closing
a project or quitting: a global unsaved-file confirmation is not available.

The IDE binds loopback without authentication. Studio assumes a trusted local
development machine and trusted project commands. Worktrees share Git objects and
refs; they isolate development state rather than sandboxing project code.

## More guides

- [Configuration](configuration.md): schema, runtime commands, ports, Compose, and IDE defaults.
- [Development](development.md): modules, tests, and source ownership.
