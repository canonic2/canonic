# Shield human input lifetime

## Briefing

The shared [human input lifetime rule](../../.canonic/rules/human-input-lifetime.md)
requires a pending approval or guided form to remain answerable while its owner
can receive the answer. This roadmap records planned corrections and does not
claim implemented support or new verification.

[Server options](src/mcp.rs) inherit MCP Kit's legacy confirmation and rich-input
deadlines. Shield independently assigns five-minute expiry to
[modern confirmations](src/mcp/confirmation.rs) and
[modern guided forms](src/mcp/elicitation.rs). The latter stores at most eight
forms and also bounds rounds. A driver-only fix leaves modern retries affected.
These paths serve policy/administrative decisions and guided configuration;
waiting itself must neither grant authority nor invalidate an unchanged answer.

[Preparation Tasks](src/mcp/tasks.rs) declare a one-minute input timeout but
currently produce preparation results rather than exposing a demonstrated
human-input wait. Review that policy during API adoption. Genuine proposal,
credential or grant validity must be distinguished from the transport's generic
patience timer; retaining a prompt does not extend issued permission authority.

Owning contracts: [MCP transport](specs/interfaces/mcp-transport.md),
[MCP tools](specs/interfaces/mcp-tools.md),
[policy proposals](specs/project/agent-policy-proposals.md),
[one-time approval](specs/project/one-time-approval.md) and
[session grants](specs/project/session-grants.md). The shared correction is
tracked in [MCP Kit's roadmap](../mcp-kit/ROADMAP_HUMAN_INPUT_LIFETIME.md).

## Roadmap

- [ ] **1. Keep confirmation and guided-input state live with their owners.**
  Adopt the shared driver model and remove generic five-minute eviction/checks
  from both modern stores. Define explicit cancellation and connection teardown
  for retained workflows without treating cancellation of a settled interim RPC
  as revocation. Preserve opaque tokens, scope/call binding, single use, bounded
  rounds and count/byte limits. Audit task-policy declarations and reachable
  wrappers for equivalent expiry, documenting external limits precisely.

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
