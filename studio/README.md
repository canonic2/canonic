# Canonic Studio

Studio is a local desktop workspace for working on several Git projects and
several tasks within each project. Each session has its own worktree, Code OSS
IDE, application processes, ports, and optional Docker Compose stack.

## Run

Requires Node.js 24+ (24.12.0 is pinned in `.nvmrc`), pnpm 10.33.0, Git, and macOS or
Linux on x64/arm64. Docker with Compose is needed for container-based projects.

```sh
cd packages/studio
pnpm install --frozen-lockfile
pnpm run setup
pnpm run doctor
pnpm start
```

Electron and code-server are pinned; setup verifies code-server's release
checksum. Studio opens with an empty project list. Click **Open project** and
choose a Git repository. Project scripts run only when you click **Start runtime**.

Application state lives in ignored `.studio/app/`. `STUDIO_DATA_DIR` changes
that directory; `STUDIO_IDE_RUNTIME` selects an installed code-server executable.
Switching preserves other sessions' running processes and retained IDE views.
Closing a project stops its sessions and preserves its checkouts and settings.

See [using Studio](docs/README.md), [project configuration](docs/configuration.md),
[development and module boundaries](docs/development.md), and [specifications](specs/README.md).

## Validate

```sh
pnpm run check
pnpm run check:full
```

The standard check validates frozen dependencies, formatting, strict types,
typed lint, unused code, module boundaries, recipes, documentation links, core
test coverage and compiled assets. The full check adds desktop and full-stack
integration suites and the production dependency advisory audit. See the
[engineering check contract](specs/quality-checks.md) for commands and prerequisites.

The Node suite tests configuration, real worktrees, state persistence, rollback,
parallel runtime startup, process descendants, and project close/reopen.

Studio uses TypeScript 5.9.3 with NodeNext modules and Node 24 types. Node scripts
and tests run directly from .ts source; the desktop and browser shell compile to
ignored dist/. Start and desktop smoke commands build automatically.

The IDE smoke suite verifies editing, extensions, themes, integrated terminals,
project/session switching, previews, and logs. Full-stack suites launch two real
Turbo/Express/Vite/Redis environments on separate Git branches and Compose
projects, verify hot reload and persistence, and remove only their owned Docker
resources. They require Docker and network access for fixture dependencies.

Fixtures and smoke entry points live under `test/`; production startup imports
neither. Ignored reports and optional captures live in `artifacts/`. A host
compositor error can prevent screenshots while browser assertions still run.

The MVP supports local development; automatic crash recovery, session deletion,
per-service controls, and Workbench/Shield integration remain outside this release.
