# Code quality

Status: existing engineering baseline. [Gallery and verification](features/gallery.md)
specifies proposed behavioral evidence as components are implemented;
[Delivery](roadmap.md) tracks criteria separately from engineering checks.

Use the Gate/Shield quality baseline: pinned Rust 1.99.0 with rustfmt, Clippy and
llvm-tools; committed Cargo.lock; no unsafe code; pedantic/unwrap_used/
mod_module_files lints; strict warnings; documented public APIs and rustdoc links.
Do not disable too_many_lines or add broad lint suppression. Tests may unwrap.

Run fmt, all-target/all-feature Clippy, nextest, doctests, private API docs,
cargo-deny and release builds. Also test the library with the gallery feature
disabled. Use insta for stable reviewed rendering evidence, proptest for meaningful
invariants, assert_cmd/predicates for CLI output and temporary process fixtures.
Coverage uses cargo-llvm-cov without an invented threshold.

Keep one integration binary with component/gallery modules. PTY tests require
host access in an agent sandbox and run separately from other process-heavy
suites. State, buffer rendering and PTY tests prove different boundaries. Product
feature checks prove local dependency compatibility, not completed UI migrations.

Dependency policy allows approved upstream registries and MIT/Apache/Unicode
licenses, ignores unpublished proprietary package licenses, warns on duplicate
versions, denies wildcards/unknown sources and checks advisories. Exceptions
require the same package-specific rationale as Gate and Shield. Artifacts stay
under ignored target/; no binaries or reports are published.

Ratatui's layout/cache graph uses foldhash, licensed under Zlib. A narrow
foldhash exception is recorded in this package and the three consumers; the
general license allowlist and source/advisory checks remain unchanged.
