# Updates and manager replacement

## Purpose and trigger

An updated client can discover an older running manager and request its safe
retirement. Replacement transfers exclusive ownership of a root through the
kernel lease. Product state is recovered by the successor under that lease;
the library does not transfer live handles or replay application requests.

`client::connect_versioned` explicitly permits both startup and version-based
retirement. Detection happens when this function connects, using the client's
supplied release version and the running manager's identity. Ordinary `connect`
without a launcher remains read-only inspection. Installing a binary alone
does not notify a running manager. Products own installation, selection of the
executable started by their launcher, and login-registration updates.

## Versions and compatibility

Release order uses complete semantic versions, including numeric version and
prerelease precedence. Build metadata does not affect precedence. The client
validates its version before connecting or launching. An unorderable running
build returns `InvalidVersion` without requesting retirement.

| Running release relative to client | Connection behavior |
| --- | --- |
| Equal, including metadata-only differences | Reuse the running incarnation. |
| Newer | Reuse it; do not request a downgrade. |
| Older and empty | Request retirement and wait for a ready replacement. |
| Older and busy | Record pending retirement and return the running identity. |

Release order is independent of package, control, product protocol and stored
state format versions. Every returned identity, including a busy older one,
must pass the product's protocol and required-capability checks before ordinary
work is sent. A busy incompatible manager retains its independent controls;
version detection does not make its product requests compatible.

## Ownership handoff

| Phase | Required behavior | Owner |
| --- | --- | --- |
| Observe | Obtain a ready identity within the overall readiness deadline. Observe lifecycle, release, incarnation and capabilities without sending product work. | Library client |
| Notify | Target that exact incarnation with `retire_if_older`; the manager independently validates release precedence before changing lifetime. | Library control protocol |
| Settle | If busy, keep serving and supervising existing resources. Record pending retirement without starting cleanup or interrupting work. | Library accounting; product resource ownership |
| Fence | Once mutation reservations and holds are empty, close admission and atomically recheck emptiness. Pending replacement bypasses the ordinary idle grace or explicit exit policy. | Library core |
| Finish | Let previously admitted replies finish; refuse new controls once work and holds settle. Join workers, remove only owned endpoints and release the lease last. | Library driver |
| Initialize successor | Acquire the same lease before loading or recovering product state; create a fresh incarnation and holds for recovered resources before serving. Lease losers must not initialize as owners. | Library lease; product initialization |
| Become ready | Bind private endpoints and answer identity only after product initialization. Check the new identity's protocol and capabilities before sending work. | Library driver; product client |

Activation may race across clients or overlap an exiting process. It never
authorizes a successor to skip lease acquisition. The successful successor's
initialization starts only after exclusion transfers. A `retired: true` reply
establishes admission closure, not endpoint disappearance, lease release or
successor readiness. A versioned connection waits for a ready identity instead
of treating that reply as a completed handoff.

Pending retirement leaves ordinary admission open while busy. Continuing work
or a product hold can therefore delay replacement indefinitely. The caller
gets the old identity in this case; it does not wait for all resources to end.
A later connection can activate a successor after retirement. Products that
need an intentional interruption use their explicit drain/cleanup workflow.
The library never drops a hold to satisfy an update deadline.

Pending retirement is in-memory intent for the current incarnation. It is not
a durable update job or a guarantee that some client remains to launch the
successor. If the process dies, kernel exclusion is released and the next
authorized connection can start a manager through the ordinary startup path.
Product crash recovery still runs before readiness.

## Product state across replacement

The product defines which durable state survives, how it is validated and how
resources are re-established. It must persist required state before releasing
the corresponding old-process obligations, and reject or recover incompatible
state before advertising successor readiness. A release comparison is not a
stored-state compatibility check or permission to perform a data migration.

In-flight requests, sockets, resource handles and credentials are not carried
in the update control. Live responsibilities stay with the old process until
the product establishes safe release. A stored record that itself requires the
old manager to stay resident remains represented by a hold; such a product
must define how explicit cleanup makes replacement possible.

Connections to the old incarnation do not become successor connections.
Clients reconnect and revalidate identity. An uncertain application result
still requires the product's stable-ID outcome lookup or reconciliation;
neither reconnection nor a changed incarnation permits automatic replay.
Products can explicitly use [request recovery](recovery.md#request-recovery)
to coordinate stable-ID outcome lookup and authorize an identical-envelope
replay. The version handshake itself sends no product requests.

## Wire contract

The synchronous server advertises capability `retire-if-older-v1` only when its
own build can be ordered as a bounded semantic version. The additive control
uses `control_version: 1`, action `retire_if_older`, a stable `request_id`, the
observed `manager_id`, required capability `retire-if-older-v1`, and exactly
`arguments: {"version":"2.0.0"}`. Build/version bounds belong to
[Limits](limits.md#sizes-and-identifiers).

Unknown control versions, missing capabilities, stale incarnations, malformed
arguments and invalid versions refuse before acknowledgement and lifetime
effects. A valid equal or older version is accepted with `newer: false` and
does not schedule retirement. The normal acknowledgement precedes the result.

| Result field | Meaning |
| --- | --- |
| `newer` | The proposed release has greater precedence than this manager's build. |
| `retired` | This notification committed or observed admission closure for a newer release; not proof of final exit. |
| `retirement_pending` | The lifetime snapshot records a replacement request, including one made by an earlier notification. |
| `blockers` | Product blocker summary when a newer release cannot yet retire; otherwise null. |

Status exposes `lifetime.retirement_pending`. The first replacement request
emits `RetirementRequested`; repeats do not repeat the event. Automatic pending
retirement exits with reason `Retired`; an explicit drain retains `Drained`.
The [lifetime contract](lifetime.md#retirement-and-drain) owns atomic fencing
and response/hold accounting.

## Races, failures and older control implementations

One absolute readiness deadline covers identity probes, retirement exchanges,
backoff and replacement readiness. Each connection and frame operation is
capped by its remaining budget. Arbitrary launcher callback duration remains
the product's responsibility. Deadline expiry returns `Starting`; it does not
cancel an already recorded pending retirement or roll back activation.

Identity probes and incarnation-targeted version retirement are safe to retry
after a lost reply. The client reobserves identity before retrying this
idempotent control. It sends no application request during handoff. Stale
targets cannot retire a successor. Concurrent clients converge through the
lease and fresh identity rather than PID checks or socket-path replacement.

If activation produces a different incarnation that is still older than the
client, the connection returns `Incompatible` instead of repeatedly retiring
replacement processes. Products must ensure their launcher or registered
service selects the installed executable. A newer successor is reused.

A manager serving the common control wire without `retire-if-older-v1` uses
the existing exact-incarnation `retire_if_empty` fallback. An empty manager
can be replaced; a busy one returns its identity and retains its own exit
policy, without pending version retirement. A manager with a different control
wire requires a product migration procedure; the library cannot negotiate an
unrecognized protocol by assuming it is safe to terminate.

## Acceptance and evidence boundary

- Empty replacement produces a fresh ready incarnation after lease transfer.
- Busy notification preserves work and holds; pending retirement fences new
  admission only when those obligations settle.
- Concurrent update clients reach one replacement; stale launchers do not
  produce restart loops.
- Equal, older, invalid and stale-incarnation notifications cannot initiate
  retirement; metadata-only changes are equal.
- Lost control replies reobserve identity without replaying product work.
- Pre-feature controls preserve busy managers and permit empty retirement.

[Verification](verification.md#existing-coverage) maps these mechanics to real
process/socket tests and deterministic core tests. Durable-state reload,
resource re-establishment, stored-format compatibility and installed-service
selection require product acceptance. The generic fixtures do not prove those
properties. Platform acceptance remains in [Platforms](platforms.md).
