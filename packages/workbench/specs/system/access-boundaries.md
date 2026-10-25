# Local access and execution boundaries

This topic records the access boundaries and the limits of their assurances.
It is not an authentication scheme or a claim that project code is sandboxed.
[Server](server.md) owns transport/process lifecycle;
[VS Code](../integrations/vscode-extension.md#workspace-trust) owns workspace trust.

**Status:** implemented: the boundaries below as described. Undecided policy:
uniform origin/Host validation of mutation routes, symlinks and dotfiles in
static serving, and authentication for a remote or shared server; see
[verification and unresolved policy](#verification-and-unresolved-policy).

## Execution and origins

The public server and implementation proxies bind to loopback. Evaluator and
owned Storybook processes use pipes/private sockets through the server.
Remote hosts and browser-only editors follow the [hosting contract](../integrations/hosting.md).
Loopback restricts network binding; it does not authenticate local clients.

Project definitions, configuration, adapters and source modules can execute
project code. Server-side JavaScript runs in the evaluator; browser code runs
in content documents. Workspace trust gates evaluation/startup, not a sandbox
for code the user chose to run. Same-origin project pages share an origin with
the local canvas, including browser storage and access to same-origin routes.
Independent artboards isolate mutable documents, not project cookies/storage
or backend side effects.

URL/Storybook lenses use dedicated proxy origins. Proxy header/cookie rewrites
and bridge injection are owned by [implementation proxy](implementation-proxy.md).
The [capture helper](../review/capture.md#helper-isolation-and-lifecycle) separately
owns renderer sandboxing, its inert mirror and denied page capabilities; those
restrictions do not describe the evaluator or every canvas content document.

## Requests and message authority

Current `same_origin` handling checks `Origin` against this server's localhost
or 127.0.0.1 HTTP origin/port and rejects `Sec-Fetch-Site` values other than
`same-origin` or `none`. Requests without Origin are accepted by this check for
local CLI/test clients. There is no shared bearer credential implied by it.

The check is applied to space opening, registered-space operations and starting
exports by POST. It is not global middleware. In particular, config-file/size
writes do not currently call it; GET can also start an export under the
[export API](../review/export.md#job-lifecycle). Do not describe all routes as
authenticated, uniformly same-origin guarded or read-only by HTTP method alone.

Renderer/host messages use the owning iframe/source, expected origin and, where
the runtime protocol supplies them, instance, generation and request identity.
An old or unrelated renderer must not operate on the active view. Registered
space coordination permits its fixed config/context operations and validates
the registered space; it is not an arbitrary loopback URL proxy.
See [artboard transport](../canvas/artboard-system.md#runtime-and-transport-contracts)
and [agent context](../integrations/agent-context.md) for their payload rules.

## Files and writes

Static requests decode the URL and reject lexical traversal outside the selected
file root. Missing files are 404, malformed encoding 400 and outside-root paths
403. The static fallback uses ordinary filesystem reads and does not canonicalize
symlinks; lexical containment is not a guarantee that every resolved inode is
inside the root. Compiler/source readers have their own containment checks;
do not infer one subsystem's policy from another's.

Project writes have explicit owners:

| Output | Owner and boundary |
| --- | --- |
| Committed manifest edits | [Configure pages](../pages/configuration-editor.md) and [sizes](../canvas/sizes.md); the server selects the config file/space |
| Handoff images | [Capture](../review/capture.md); saved under the host space's `.canonic/.handoffs/` |
| Camera/export downloads | Browser download/output, separate from manifest writes |
| Runtime, compiler and index caches | Their owning runtime/index/compiler; outside project authoring files |
| Agent server announcement | [Agent context](../integrations/agent-context.md#finding-the-server); owned announcement cleanup |

The static project server serves project files beyond the declared page list.
It is not a publication filter for secrets or an implementation of export's
source-exclusion rules. Export filtering, source opening and capture-origin
validation retain their separate contracts.

## Request and process bounds

The current route body limits are enforced while reading, not just by trusting
Content-Length ([routes.rs](../../server/src/serve/routes.rs)):

| Body kind | Limit |
| --- | --- |
| Saved shot | 64 MiB |
| Capture/mirror request | 16 MiB |
| Handoff, config edits and canvas review | 1 MiB |
| View/space-operation request | 256 KiB |
| Diagnostic record request | 16 KiB |

These byte caps do not replace decoded image, artboard, text, archive or compiler
resource bounds. Each capability validates its own structured data and releases
owned requests/resources on completion, failure, timeout or disposal. Host
question deadlines are owned by the server message transport; evaluator and
capture deadlines remain in their respective topics. Rejected work reports an
error rather than returning a truncated success.

## Verification and unresolved policy

Existing evidence is in [server tests](../../server/node/server.test.js)
(cross-origin space operations, capture origins and payload validation),
[file-serving tests](../../server/src/serve/files.rs),
[canvas route validation](../../server/src/serve/routes/canvas.rs) and
[renderer runtime tests](../../ui/src/modules/preview-sessions/controller.test.js).
Presence of checks is not a penetration test or full hostile-project assessment.

Missing policy decisions include uniform mutation-route origin/Host validation,
the treatment of symlinks and dotfiles in static project serving, and whether a
future remote/shared server needs authentication. No stronger protection or
network-sharing mode is established by the current loopback implementation.
Live trust transitions and trust of added roots remain in the extension's
[open questions](../integrations/vscode-extension.md#open-questions).
