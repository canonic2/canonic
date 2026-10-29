# Active policy and repair

Shield evaluates requests against a saved snapshot of confirmed configuration,
rules, and packs. Manual file edits take effect only after review with
`shield policy review`.

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
An abandoned writer lock blocks changes. Preserve the private state for recovery.

## Recover the compiled cache

Shield rebuilds missing, corrupt, or incompatible compiled plans before evaluation.
Use `shield cache prepare` to do this explicitly without changing policy.
See [compiled policy cache](compiled-cache.md).

## Integration consistency

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
