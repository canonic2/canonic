# highlight.js browser assets

Unmodified ES modules from `@highlightjs/cdn-assets` 11.11.1, BSD-3-Clause.
`manifest.json` records the package source and SHA-256 of every shipped module;
`LICENSE` contains the upstream license. Copy `es/core.min.js` and the ten
explicitly registered `es/languages/*.min.js` files from that pinned package to
update them. Keep the worker registrations, manifest and language tests together.

These prebuilt third-party files are served unchanged with the UI. Authored UI
code has no build step. No CDN or package registry is contacted at runtime.
