# Docker Compose isolation

## One stack per session

Every configured session uses Compose project name
`studio_<project-id>_<session-id>`. Stable IDs preserve data identity across
restarts. Different branches can use the same internal service names and container
ports while their host ports and resources remain separate.

Every invocation supplies explicit project name, project directory, file, working
directory, and session environment. Cleanup never depends on the selected UI
workspace. Studio does not use Docker-wide prune commands.

## Project setup

Declare compose.file in the runtime recipe. Use ordinary project-scoped networks
and named volumes, without fixed container names. Publish only needed host ports,
using allocated variables and 127.0.0.1 bindings. Internal ports can remain fixed.

```yaml
services:
  redis:
    image: redis:7.4.6-alpine
    ports:
      - '127.0.0.1:${REDIS_PORT}:6379'
    volumes:
      - redis-data:/data
    healthcheck:
      test: ['CMD', 'redis-cli', 'ping']
      interval: 1s
      timeout: 1s
      retries: 30
volumes:
  redis-data:
```

The recipe must allocate REDIS_PORT. Healthchecks make up --wait meaningful;
a service without one may be running before it can serve requests. Application
health should also exercise required dependencies. This example shows isolation;
the [fixture](../test/fixtures/fullstack/compose.yaml) configures Redis persistence.

## Validation and launch plan

Studio resolves the Compose file's real path and requires it inside the checkout.
It runs docker compose config --format json using the assigned environment and
validates the resolved model:

| Resource/setting           | Contract                                                       |
| -------------------------- | -------------------------------------------------------------- |
| Named volumes and networks | Resolved names begin with session Compose name plus underscore |
| External resources         | Rejected                                                       |
| container_name             | Rejected                                                       |
| network_mode overrides     | Rejected                                                       |
| privileged services        | Rejected                                                       |
| Published ports            | Loopback host IP and one of the allocated values               |
| Bind mounts                | Resolved source path lexically within the checkout             |

These checks prevent common accidental sharing, not every unsafe Docker capability.
Bind sources are not realpath checked; accepted input is not a security sandbox.

The resolved model is saved as
`<data-root>/sessions/<session-id>/config/runtime/compose.json`, mode 0600.
Resolved dollars are escaped for Compose's second parse. The plan may include
resolved environment values and must remain local state, not committed content
or publicly exposed logs.

Ownership is recorded before up --detach --wait --wait-timeout, so partial startup
still has a cleanup target. Down uses the saved plan rather than the branch's
current YAML. Editing configuration during a run cannot redirect cleanup.

## Data and stop behavior

Normal Stop uses scoped down --timeout 3. Containers and networks are removed;
named volumes remain. Restart uses the same project name and data volumes even
if host ports change. Session A's stop must not affect B's resources or data.

Separate volumes produce separate databases. They do not automatically seed a
branch's data or reconcile schema changes. Migrations, seeds, and resets remain
explicit application decisions. Provisioning must not silently reset a neighbor.

Test teardown can use down --volumes on its owned fixtures. Production has no
reset-data/session-delete action. Anonymous storage should not be assumed to have
the tested named-volume persistence lifecycle.

composeStatus reflects an ownership flag, not continuously queried Docker health.
Manual deletion, daemon loss, and hard crashes need reconciliation. A saved plan
alone is not a durable recovery journal.

## Proposed next work

Data reset needs resource inspection and explicit deletion scope. Recovery needs
Docker labels reconciled with durable ownership before adoption or cleanup.
Images/build caches may be shared; mutable databases and volumes remain scoped
unless a future configuration explicitly defines shared ownership.

Remote engines, shared external services, and broader Compose features need
compatibility/policy decisions rather than bypassing current guards.

## Source and acceptance

Owners: [Compose adapter](../src/modules/runtime/compose.ts) and
[resolved-model policy](../src/modules/runtime/compose-policy.ts).

Acceptance requires two healthy stacks with distinct labels, networks, volumes,
and host ports; independent data; restart persistence; saved-plan cleanup after
source edits; partial-start cleanup; and rejected shared names, external resources,
escaped files, and public bindings. Teardown removes only owned fixture resources.
