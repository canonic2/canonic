# Shield changelog

## Unreleased

- Include pinned direct packs in status/list update availability and CLI/MCP
  notices. Replace `pack_updates.excluded_pinned` with `direct_total`, and report
  “No direct pack selections to check.” when the inspected scope has none.
- Make explicit pack updates include pinned selections, preserving profiles and
  pin mode at the reviewed versions. Exact update versions and recovered
  project-file selections no longer require a separate reinstall workflow.
- Avoid rebuilding unchanged guided-installation settings and preparing complete
  evaluators merely to read dependency metadata during pack acquisition.
- Recognize curl GET query parameters passed with `-G --data-urlencode`, keeping
  file-backed data outside the fetch model and retaining web/output boundaries.
- Model sed regex addresses and ranges across reads, edits and named file
  operations so Baseline can allow supported scripts while execution forms
  still require review.
- Give recurring web Asks a concrete reusable-allowance route in completion
  feedback, with project routing and existing-rule inspection before review.
- Model Base64 stream transformations and their input/output file paths, allowing
  baseline coverage without granting access to protected files or executing
  decoded content.
- Add `graphql-subtree-v1` for query-root models that verify root arguments and
  allow all nested selections. Prepared GitHub pack 0.8.0 uses this capability
  for its root-only repository boundary; activation requires a reviewed pack update.
- Accept structurally valid object argument literals in query classification
  under `graphql-v1`. This also affects existing installed classification-only
  rules, including unscoped GitHub 0.7.1 queries: those reads can receive the
  listed-read decision instead of raw-API review after an engine update alone.
  Existing finite selection models retain their restrictions.
- Refuse `$endCursor` nested inside an object literal when using `--paginate`,
  for both classification-only queries and query-root subtrees.
