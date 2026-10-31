---
name: policy-review
description: Review recurring Shield permission interruptions when the user asks, interpret retained evidence, and prepare a narrow lasting policy change through Shield's supported approval route.
---

# Review Shield permissions

Use this workflow when the user asks to review recurring permission interruptions
or interpret a request before a lasting policy change. Do not start a review
after every task or every Allow, Ask or Deny.

1. Inspect `suggestion_list`. Follow each row's next action. When needed, inspect
   selected `history_list`, `rule_list` or `policy_show` evidence. Incomplete
   coverage is not evidence that a command grammar or permission is safe. Reuse
   an existing proposal or respect a retained deferral instead of repeating it.
2. Determine whether the user needs a one-time action, a lasting policy change,
   or an unsupported operation. Follow the returned route. Shield's MCP tools
   cannot grant a one-time action. A denied request alone does not authorize
   a lasting permission change.
3. For an authorized lasting change, use `policy_propose` with the narrowest
   justified scope and the returned review context where available. Learn the
   actual command grammar; do not guess it from a single invocation. Preserve
   useful deny boundaries and explain any broadening. Read the bundled guides
   through `shield_docs_list`, `shield_docs_search` and `shield_docs_read` when
   rule syntax or scope needs clarification.
4. Inspect the retained proposal through `policy_show`. Copy its exact
   `apply_arguments` unchanged into `policy_apply` only through the registered
   adapter's returned approval route. Claude Code uses host form confirmation;
   Codex uses its tool permission review. Generic hosts require the user to run
   `shield policy review` in their terminal. Preparation and successful diagnostic
   evaluation do not activate policy or approve execution.
5. Treat decline, cancellation, expiry, stale policy and unavailable approval as
   explicit outcomes. Inspect current policy and `policy_receipt` before a
   still-needed retry. Retry only while the original task and authorization
   remain valid. Do not repeat a rejected proposal to seek acceptance; wait for
   a new user request. Do not add conversational confirmation around work the
   user has already authorized.

Never edit protected Shield state, launch the operator CLI from an agent, change
the host's permission mode, fabricate confirmation responses, or select a more
privileged adapter using a client name. The host owns skill activation and user
approval; reading this file grants no authority.
