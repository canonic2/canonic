# Configuration and setup

## Desktop installation

Studio has its own package.json and pnpm-lock.yaml. Run
pnpm install --frozen-lockfile, pnpm run setup, pnpm run doctor, then pnpm start
from packages/studio. Node 24 or later is
required; .nvmrc pins 24.12.0. Electron and code-server are pinned and setup
checks the IDE download checksum. Docker/Compose is optional for host-only recipes.

pnpm 10.33.0 and TypeScript 5.9.3 are pinned. Native Node 24 runs script/test .ts
files; the desktop build emits JavaScript under ignored dist/. Start builds before
launching Electron. Type-only imports and erasable syntax keep source compatible
with Node's type stripping; the type checker, not Node, validates types.

Production starts empty on first launch and opens user repositories. Its default
profile is packages/studio/.studio/app. STUDIO_DATA_DIR selects another profile,
STUDIO_IDE_RUNTIME selects a code-server executable, and STUDIO_NODE selects the
executable for `$NODE` commands. Tests use separate profiles and fixtures.

State, downloaded runtimes, caches, and artifacts are ignored. Desktop installation
does not modify user repositories or copy fixture code into them. Signed installers
and automatic distribution updates are not implemented.

## Project recipe

Commit studio.config.json at the repository root. Each session reads its own
branch's copy. No recipe is needed for IDE/terminal use; a runtime is required for
Start runtime.

```json
{
  "version": 1,
  "runtime": {
    "provision": ["pnpm", "install", "--frozen-lockfile", "--ignore-scripts"],
    "command": ["$NODE", "node_modules/turbo/bin/turbo", "run", "dev"],
    "ports": ["FRONTEND_PORT", "BACKEND_PORT", "REDIS_PORT"],
    "previewPort": "FRONTEND_PORT",
    "readyPath": "/api/health",
    "compose": { "file": "compose.yaml" }
  },
  "ide": { "settings": { "editor.tabSize": 2 } }
}
```

This needs matching Turbo tasks, server port handling, readiness, and Compose
configuration. JSON alone cannot make an application isolated. The
[configuration guide](../docs/configuration.md) lists fields/defaults; the
[full-stack fixture](../test/fixtures/fullstack/studio.config.json) is executable
reference material.

## Validation contract

The [schema](../config/project.schema.json) owns accepted properties and types.
Version is required and must equal 1. Unknown properties are rejected; ide.settings
intentionally accepts arbitrary editor settings. Validation clones input before
applying defaults.

The configuration module additionally checks relationships:

- Ports are unique uppercase names, either PORT or ending in _PORT.
- Ports default to PORT; previewPort defaults to the first and must name one.
- readyPath begins with one slash and defaults to /health.
- Readiness and provisioning deadlines are bounded integers.
- env values are strings and cannot override identity, XDG paths, allocated ports,
  or ELECTRON_RUN_AS_NODE.
- compose.file is relative without parent traversal; its actual containment is
  checked before Compose up.

Missing configuration yields an IDE-only version 1 configuration. Malformed JSON
errors identify the file. Invalid configuration blocks runtime startup but not
opening the IDE to repair it.

`pnpm run config:check /path/to/project` validates without running commands.
It cannot prove executable availability, successful installation, bind success,
or resolved Compose policy. Doctor reports tool availability separately.

## Changing configuration

Runtime start reads the current recipe each time. Editing it does not reconfigure
running processes or the saved Compose plan. Restart applies changes while
preserving session data identity.

IDE settings seed the user profile only on first opening. Later editor changes
persist; recipe changes do not overwrite the profile. Workspace settings remain
subject to normal Code OSS behavior. Naming an extension theme does not install it.

There is no local override file, automatic .env copying, secret manager, or migration
engine. Commands inherit the desktop environment. Projects must provision ignored
files/credentials without committing secrets or accidentally sharing mutable state.

## Proposed next work

Define precedence and secret handling before local overrides/user defaults. Version
changes need migration/error behavior and compatibility tests. Recipe version and
persisted state version are separate contracts.

Provision caching needs branch/lockfile-aware invalidation and manual rerun. Directory
existence is insufficient to skip installation. Product integrations need documented
configuration in their owning modules rather than ad hoc environment conventions.

## Source and acceptance

Owners: [configuration](../src/modules/configuration/index.ts),
[schema](../config/project.schema.json), [setup](../scripts/setup.ts),
[doctor](../scripts/doctor.ts), and [production entry](../app/main.ts).

Acceptance requires IDE-only projects, immutable-input defaults, strict invalid
key/port/env rejection, file-specific errors, checking without execution, and no
demo state in production startup.
