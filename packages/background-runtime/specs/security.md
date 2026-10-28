# Security

This topic owns the threat model and its limits. Detailed defenses remain in
the linked contract topics. A private endpoint authenticates the effective
user; it does not authorize an application operation.

## Protected properties

- One cooperating manager holds kernel exclusion for one user and root.
- Other users cannot obtain authority by claiming an identity in a request.
- Retirement and drain cannot target a replacement incarnation accidentally.
- Endpoint cleanup preserves files and sockets outside the holder's authority.
- Connection input and output respect the library's declared bounds.
- Lost transport results do not cause automatic duplicate mutations.
- Explicit replay requires product authority; explicit force-stop is available
  only when enabled by the product and targets one exact incarnation.

## Threats and defenses

| Threat | Defense and owning contract |
| --- | --- |
| Another user connects or claims the owner's UID | Private root and sockets, with kernel peer credentials checked before frame reading ([endpoints](endpoints.md)). |
| A symlink, non-socket or permissive path occupies an endpoint | Refuse it without repair or deletion ([endpoints](endpoints.md)). |
| Racing starters or a stale PID/socket suggest another owner | Kernel lease on a persistent anchor; never infer exclusion from PID or pathname presence ([startup](startup.md)). |
| Cleanup encounters a replacement root, anchor or socket | Validate cleanup authority and compare device/inode identities ([endpoints](endpoints.md#binding-and-cleanup)). |
| A delayed stop request reaches a successor | Require the observed incarnation for retirement and drain ([protocol](protocol.md#controls-and-compatibility)). |
| A version notification causes resource loss or restart churn | Independently validate release order and incarnation, preserve live holds, and bound stale-launcher replacement ([updates](updates.md)). |
| Ordinary traffic saturates workers | Independent bounded control lane ([limits](limits.md#counts-and-retention)). |
| Oversized or incomplete frames exhaust capacity | Refuse before oversized body allocation, bound encoding and apply read deadlines ([limits](limits.md)). |
| A lost reply tempts a client to retry a mutation | Return an uncertain outcome without automatic replay; product owns durable reconciliation ([protocol](protocol.md#ambiguity-and-recovery)). |
| Parent exit, terminal signals or inherited environment affect the manager | Detached session/group, null stdin and explicit launch environment ([startup](startup.md#detached-launch)). |

These properties rely on kernel file locks, filesystem ownership and modes,
Unix peer credentials, and the OS random source for fresh incarnations.
The [platform topic](platforms.md) identifies implemented adapters and missing
host acceptance. A forged UID comparison test is not a second-user boundary test.

## Outside the boundary

- Mutually hostile processes under the same effective user are not isolated.
  Products must not treat possession of this endpoint as per-operation consent.
- Root selection, application authorization, payload validation, secrets in
  product metadata, durable outcomes and resource cleanup belong to products.
- A release claim is not artifact verification or proof of stored-state
  compatibility. Products own installation trust and successor state recovery
  ([updates](updates.md#product-state-across-replacement)).
- The lease anchor is persistent. An actor able to replace it can defeat the
  cooperating single-instance model; a PID check cannot repair that boundary.
- Detachment does not promise escape from a cgroup, container or external
  supervisor. Registered-service installation and permissions belong to products.
- A callback can block, allocate excessively or fail after producing effects.
  I/O limits neither sandbox it nor roll back its work. Product responsibility
  must remain held until cleanup or recovery is established.
- Explicitly enabled [force-stop](recovery.md#explicit-force-stop) skips graceful
  cleanup and cannot establish external-worker termination. Product crash
  recovery and fencing remain required before successor readiness.

## Known gaps

Real trickled-request-write coverage, platform and adversarial acceptance gaps are listed in
[verification](verification.md#implementation-and-coverage-gaps). Generic update
fixtures do not establish product state recovery or installer correctness.
These evidence boundaries do not weaken the required contract.
