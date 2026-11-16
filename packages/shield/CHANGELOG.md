# Shield changelog

## Unreleased

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
