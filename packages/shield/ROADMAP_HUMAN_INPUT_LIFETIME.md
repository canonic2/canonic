# Shield human input lifetime

## Briefing

The shared [human input lifetime rule](../../.canonic/rules/human-input-lifetime.md)
requires a pending approval or guided form to remain answerable while its owner
can receive the answer. This roadmap records planned corrections and does not
claim implemented support or new verification.

[Server options](src/mcp.rs) inherit MCP Kit's live-owner legacy waits and
suspended execution budgets. [Policy proposals](src/review/proposal_lifecycle.rs),
[modern confirmations](src/mcp/confirmation.rs),
[modern guided forms](src/mcp/elicitation.rs) and retained administrative
reviews ([review/mcp.rs](src/review/mcp.rs)) carry no age expiry: they close on
answer, cancellation, supersession, base change or connection loss, within
count bounds. These paths serve policy/administrative decisions and guided
configuration; waiting itself must neither grant authority nor invalidate an
unchanged answer.

Native one-time approval requests ([read](src/review/read_approval.rs),
[run](src/review/run_approval.rs), [hook](src/review/hook_approval.rs)) still
close pending requests ten minutes after creation, and
[session grant](src/review/session_grants.rs) offers keep their pending window.
Separate those patience windows from issued-grant lifetimes, which are genuine
authority validity.

[Preparation Tasks](src/mcp/tasks.rs) produce preparation results without a
demonstrated human-input wait. Adopt shared snapshot retention helpers if task
input is added. Genuine proposal,
credential or grant validity must be distinguished from the transport's generic
patience timer; retaining a prompt does not extend issued permission authority.

Owning contracts: [MCP transport](specs/interfaces/mcp-transport.md),
[MCP tools](specs/interfaces/mcp-tools.md),
[policy proposals](specs/project/agent-policy-proposals.md),
[one-time approval](specs/project/one-time-approval.md) and
[session grants](specs/project/session-grants.md). The shared correction is
defined in [MCP Kit's confirmation contract](../mcp-kit/specs/confirmation.md).

Integration prerequisite: land MCP Kit's human-wait suspension changes before or
together with Shield's migrated elicitation tests. Those tests intentionally wait
past the execution budget and cannot pass against the earlier driver contract.
Treat a Shield-only commit of that migration as dependent work, not a standalone
green baseline.

## Roadmap

- [ ] **1. Keep confirmation and guided-input state live with their owners.**
  Done: age expiry removed from policy proposals, both modern stores and
  administrative reviews, with delayed-answer tests. Remaining: remove the
  pending-request window from native read/run/hook approvals and session-grant
  offers, keeping grant lifetimes. Define explicit cancellation and connection teardown
  for retained workflows without treating cancellation of a settled interim RPC
  as revocation. Preserve opaque tokens, scope/call binding, single use, bounded
  rounds and count/byte limits. Audit task-policy declarations and reachable
  wrappers for equivalent expiry, documenting external limits precisely.
  The handler inherits `--request-timeout-ms` (5,000 ms by default, at most
  60,000 ms), with no per-confirmation admission override. Verify slow preparation
  plus post-approval revalidation within that shared active budget; human waiting
  must not replenish it. Adjust a demonstrated insufficient operation budget at
  its owner rather than restoring a fresh continuation deadline.

- [ ] **2. Revalidate policy and proposal state on delayed acceptance.**
  Trace confirmation consumers and every guided-form completion through their
  owning review/publication code. Recheck project identity, current revision,
  candidate scope, selected artifacts and operation preconditions. Re-present
  materially changed or genuinely invalid proposals while preserving unchanged
  valid answers. Keep final policy authorization distinct from form completion,
  and preserve one-time consumption, receipt inspection and grant validity.
  Release workers and authority locks during waits and keep unrelated inspection
  usable; do not add implicit persistence across reconnects.

- [ ] **3. Verify delayed forms and approvals without weakening enforcement.**
  Extend existing MCP confirmation, elicitation and affected review fixtures.
  Cover answers beyond former deadlines on legacy and modern paths, multiple
  delayed rounds, changed revisions/candidates, real domain invalidation,
  decline/cancel, disconnect races, replay, capacity recovery and resumed
  execution limits. Assert that no policy or permission is applied without the
  required accepted review. Use controlled time and focused `test-focus` runs;
  native agent/PTY acceptance requires an explicitly selected host and scope.

- [ ] **4. Self-destruct after reconciliation.** Reconcile durable requirements,
  decisions, status and remaining verification gaps into the owning transport,
  tool and authority specs. Update supported guides and recovery messages to
  match implemented behavior. Remove incoming roadmap links, then delete
  `ROADMAP_HUMAN_INPUT_LIFETIME.md` only when its work and reconciliation are
  complete. Leave this item unchecked until deletion.
