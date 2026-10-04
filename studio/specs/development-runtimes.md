# Development runtimes

## Runtime unit

A session has one configured host runtime command: a single server or a supervisor
such as Turbo that starts an Express backend and Vite frontend. Studio supervises
its process group as one unit, without individual child-service controls.

Start and Stop are explicit actions. A per-session queue serializes them, duplicate
starts reuse the live owner, and stop waits for pending startup. Different sessions
can provision and start concurrently. IDE startup has a separate queue.

## Startup sequence

1. Read and validate studio.config.json from the session checkout.
2. Clean up an exited prior runtime record before replacing it.
3. Allocate named ports and construct the session environment.
4. Run optional provisioning in the checkout.
5. Resolve, validate, start, and wait for optional Compose services.
6. Launch the host command as a detached process group in the checkout.
7. Poll the preview origin's readyPath until it proves this session is ready.
8. Return the runtime record and expose running status and Preview.

Provisioning runs on every start, including restarts, with a separate timeout.
It should tolerate repeated runs; destructive seeds/resets require deliberate
project-specific behavior. Opening a project never executes provisioning.
Provisioning owns a separate process group. Its descendants are stopped after
completion, failure, or timeout before application startup can proceed.

## Ports and environment

The recipe lists variables such as FRONTEND_PORT, BACKEND_PORT, and REDIS_PORT.
Studio probes unused loopback ports and ensures distinct values within the
runtime. It passes them as strings to provisioning, Compose, and the host command.
previewPort chooses the browser origin.

Probing closes the temporary listener before real startup. Ports are not reserved
across startup or concurrent sessions. Bind conflicts fail without retry. Servers
must consume assigned ports and fail on collisions rather than silently choosing
another port.

STUDIO_SESSION_ID and STUDIO_CHECKOUT identify the task. XDG_CONFIG_HOME,
XDG_DATA_HOME, and XDG_CACHE_HOME point under its Studio session directory. The
host environment is inherited; tools ignoring XDG or using shared home-directory
state need explicit configuration.

runtime.env cannot override reserved identity, XDG paths, allocated port keys,
or ELECTRON_RUN_AS_NODE. Commands are argument arrays. `$NODE` as the executable
selects STUDIO_NODE or the running Node executable. Arbitrary argument variable
substitution is not implemented; shell operators require an explicit shell.

## Readiness and preview

Readiness requests `http://127.0.0.1:<preview-port><readyPath>` and requires HTTP
success plus JSON sessionId equal to STUDIO_SESSION_ID. A neighboring server on
the port must not satisfy readiness. Defaults are /health and a 30-second deadline.

For frontend/backend stacks, the frontend can proxy /api/health to its own backend.
The endpoint should exercise essential dependencies. Studio verifies identity;
it does not independently prove the application's dependency checks are complete.

Preview uses the configured port, not a shared global localhost URL. Proxy targets,
WebSocket/HMR settings, callbacks, and backend clients must derive their addresses
from assigned values.

## Framework setup

The [full-stack fixture](../test/fixtures/fullstack/studio.config.json) is the
verified reference: Turbo runs persistent development tasks, Express binds
BACKEND_PORT, Vite binds FRONTEND_PORT with strictPort and proxies to that backend,
and Redis uses the session Compose stack. Turbo explicitly forwards identity and
port variables to its tasks; otherwise children may not receive Studio's values.

Laravel/PHP and other tools use the same contract: run in the checkout, consume
assigned ports, scope writable data, and expose readiness identity. Executables
and dependency tools must be available on PATH. These are integration directions;
Express/Vite/Turbo is tested end to end, Laravel is not.

Multiple host processes need a supervisor that retains children and propagates
termination. Do not background daemons outside the owned process group. Commands
started manually through IDE terminals are user-managed and not included in
runtime status or runtime Stop.

## Stop, logs, and failures

Stop sends SIGTERM to the host process group, allows a grace period, then uses
SIGKILL if needed. Descendants must be cleaned even after their supervisor exits.
Compose cleanup is also attempted; one failure must not skip the other cleanup.
Named volumes are retained.

Startup failure cleans partially started resources. Cleanup failures retain
ownership for retry and surface an error. Logs combine bounded host output and
Compose lifecycle output, not a persistent archive or container-log aggregation.
Provisioning output is not streamed into that view.

Status is starting/stopping during operations, otherwise running/stopped based on
the host record. Readiness is a startup gate, not continuous monitoring. An
unexpected host exit can leave Compose running until explicit Stop, restart
cleanup, project close, or shutdown.

## Proposed next work

Port retries need cleanup and a new complete environment/plan for each attempt.
Define health and unexpected-exit behavior before automatic restarts. A service
graph needs dependencies, readiness, logs, and ownership; splitting command text
is insufficient. Add framework fixtures before promising Laravel support.

## Source and acceptance

Owners: [runtime registry](../src/modules/runtime/index.ts),
[supervisor](../src/modules/runtime/supervisor.ts), and
[process adapter](../src/platform/processes.ts).

Acceptance requires concurrent startup, distinct endpoints, correct proxy routing,
independent edits/hot reload and restart, identity readiness, descendant cleanup,
and failed startup without disturbing a healthy neighbor.
