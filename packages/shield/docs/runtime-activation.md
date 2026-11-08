# Active policy and repair

Shield evaluates requests against a saved snapshot of confirmed configuration,
rules, and packs. Manual file edits take effect only after review with
`shield policy review`.

## Continue after an update

Shield converts supported older saved state when a command, MCP connection or
hook next opens the project. It preserves the confirmed policy and existing
approval scope, expiry and consumption, and keeps private recovery copies.
An interrupted conversion can retry without deleting state or re-enrolling.
Existing historical pack and rule formats remain readable. Subsequent pack
maintenance and rule changes use the normal review flow.

An older Claude text-read interceptor keeps its original behavior and owned
files. Replacing it with native hooks still uses the normal setup review. Old
reviews whose saved evidence is incomplete remain visible as closed outcomes;
the agent can propose a fresh change for confirmation.

## Review an edit

```sh
shield status --json
shield policy review
```

Choose `manual` in `shield policy review` to review file edits. After confirmation,
`shield status --json` reports the active generation and any repair needs.
Guided operations such as `shield rule add` already include review and activation.

## Repair projected files

Project policy files are copies of the saved snapshot, called **projections**.
If `status` reports projection repair, choose `repair` in `shield policy review`.
Repair recreates missing files and finishes interrupted writes whose old
contents still match. It preserves other edits for review through `manual`.
A concurrent policy change cancels the repair confirmation.

If publication fails, the prior policy stays active. If publication succeeds but
writing a projection fails, the receipt reports a committed change with repair
needed; the new policy is already active. Check status before retrying.
Writer ownership is released automatically if Shield exits or crashes. The next
operation continues without deleting lock files. Shield also recognizes legacy
PID markers and reclaims one only after establishing that its writer exited;
live or unverifiable ownership is preserved.

## Recover the compiled cache

Shield rebuilds missing, corrupt, or incompatible compiled plans before evaluation.
Use `shield cache prepare` to do this explicitly without changing policy.
See [compiled policy cache](compiled-cache.md).

## Integration consistency

Existing Claude/Codex native registrations can continue using their older MCP
and hook launch commands after an executable update. A native registration
created before the Ask fallback setting keeps Block as its default; saved
explicit choices stay intact. This compatibility preserves existing ownership
and does not approve additional tools.

Confirmation compiles the candidate, then publishes its configuration, rules,
artifacts, and revision in one transaction. Runtime consumers load that
snapshot's verified plan. Review, active policy, and proposals reference shared
saved source objects.

Prepared evidence binds the project, revision, snapshot digest, and activation.
The consumer holds the publication lock while revalidating and handing off an
allowed request. Changed activation or a decision changed by rule expiry
requires fresh evidence. The host must protect operator actions and state;
activation does not isolate unrestricted same-user processes.

For recovery by symptom, use [troubleshooting](troubleshooting.md).
