# Project configuration

Place `studio.config.json` at the repository root and commit it. Studio reads
each session's copy. The file uses version 1 and is validated against the shipped
[JSON Schema](../config/project.schema.json). Unknown properties are rejected.
A project without this file can use its IDE and terminal.

```json
{
  "version": 1,
  "runtime": {
    "command": ["$NODE", "server.ts"],
    "readyPath": "/health"
  }
}
```

Commands are nonempty argument arrays, not shell text. `$NODE` selects the
application's Node runtime; `STUDIO_NODE` may point to another Node executable.
Other commands must exist on the desktop application's PATH.

Provisioning must finish before the development command starts. Its process
group is cleaned up on completion, failure, and timeout; long-running servers
belong in runtime.command rather than runtime.provision.

The command receives `PORT`, `STUDIO_SESSION_ID`, and `STUDIO_CHECKOUT`.
Its readiness endpoint must return a successful HTTP response and JSON with
`sessionId` equal to `STUDIO_SESSION_ID`.

## Multiple services

A supervisor such as Turbo can run several application processes:

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
  }
}
```

See the [complete fixture](../test/fixtures/fullstack/studio.config.json), including
its Turbo environment forwarding and Vite proxy configuration. Dependencies and
Turbo files belong to each checkout. Express listens on BACKEND_PORT; Vite uses
FRONTEND_PORT with strictPort enabled and proxies to that checkout's backend.

| Runtime property           | Meaning and default                                                        |
| -------------------------- | -------------------------------------------------------------------------- |
| command                    | Required executable and argument array                                     |
| provision                  | Optional command run before Compose and the application, on each start     |
| env                        | Optional string-valued environment variables                               |
| ports                      | Unique uppercase variable names ending in _PORT, or PORT; defaults to PORT |
| previewPort                | Port variable used for Preview; defaults to the first port                 |
| readyPath                  | Readiness URL path; defaults to /health                                    |
| timeoutMs                  | Application readiness timeout: 30000; range 1000–300000                    |
| provisionTimeoutMs         | Provision timeout: 180000; range 1000–600000                               |
| compose.file               | Checkout-relative Compose file                                             |
| compose.waitTimeoutSeconds | Compose health wait: 60; range 1–300                                       |

Reserved Studio identity, XDG directories, allocated ports, and
ELECTRON_RUN_AS_NODE cannot be overridden through runtime.env. Ports are probed
before startup; a later bind conflict fails with logs instead of retrying.

Compose uses a project name derived from the project and session IDs. Use ordinary
project-scoped networks and volumes. Published ports must bind 127.0.0.1 and use
allocated variables. External resources, fixed container names, privileged
services, network mode overrides, and mounts outside the checkout are rejected.
These checks prevent common collisions and are not a security sandbox.

Studio captures the resolved Compose plan before startup. Editing the source
recipe or YAML cannot redirect cleanup. Stop retains volumes. A failed start
cleans up its host process group and containers; cleanup failures remain available
for retry.

## IDE settings

```json
{
  "version": 1,
  "ide": {
    "settings": {
      "editor.tabSize": 2,
      "workbench.colorTheme": "Default Dark Modern"
    }
  }
}
```

IDE settings initialize each session's settings on its first opening. Later edits
through the IDE persist. Workspace trust stays enabled by default. Invalid
project configuration does not prevent opening the IDE to fix that file.

## Check configuration

From packages/studio:

```sh
pnpm run config:check /path/to/project
pnpm run doctor
```

Configuration checking validates JSON without executing its commands. Doctor
reports Node, Git, Docker/Compose, and the installed IDE runtime. Docker is optional
for projects with only host commands.
