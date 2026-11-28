# Bounded cookie regular expressions

This adapter embeds the standalone QuickJS-NG RegExp and Unicode routines,
without a JavaScript runtime or host bindings. `bridge.c` owns allocation,
interrupt checks, compilation, matching and cleanup. Rust supplies UTF-16 cookie
attributes, the original operation deadline and cancellation state.

## Upstream sources

The seven files in `vendor/` other than `libunicode-table.h` come from
QuickJS-NG commit `0fdea21ff1090084e91dad812b343d92e79ba9d9`:
<https://github.com/quickjs-ng/quickjs/tree/0fdea21ff1090084e91dad812b343d92e79ba9d9>.
They were imported from rquickjs-sys 0.14.0's bundled source and their Git blob
identities checked against that upstream tree. No rquickjs Rust dependency is used.

The Unicode 16 data header comes from Bellard QuickJS commit
`fa628f8c523ecac8ce560c081411e91fcaba2d20`:
<https://github.com/bellard/quickjs/blob/fa628f8c523ecac8ce560c081411e91fcaba2d20/libunicode-table.h>.
Its Git blob is `67df6b3a3ccded3ea9e111cbf9e760d94dbd6668`; the imported header
is byte-identical. This matches the pinned SDK Node runtime's Unicode version.
The newer engine's Unicode 17 header fails the reference comparison.

Two local changes in `vendor/libunicode.c` adapt that older data format:

- Include `../unicode16-compat.h`. It supplies the missing White_Space index;
  the 22-byte property payload itself is identical between both upstream trees.
- Do not offset script-name indices: the pinned table includes Unknown at index
  zero, whereas the newer engine's own table excludes it.

All other imported engine files remain unchanged. `CONFIG_ALL_UNICODE` is
required when compiling the pinned table. The MIT license is in `vendor/LICENSE`;
the installed combined notice is `third-party/browser/ecmascript-notices/LICENSE`.

## Bounds and reference checks

Each call owns at most 32 MiB of native allocations and a 512 KiB stack budget.
Allocation headers are included in the heap bound. Allocation failure, parser
failure, interruption and execution failure reclaim the complete allocation
list. Capture storage uses the engine's allocation count, including internal
registers. Input and pattern buffers retain trailing sentinels outside their
explicit lengths; embedded NULs retain their original meaning.

`pinned_cookie_filter_unicode_reference` compares every Unicode code point,
including lone UTF-16 surrogates, with the pinned Node runtime for representative
categories, scripts, script extensions, case folding, whitespace and emoji.
Public cookie-filter fixtures additionally compare syntax, flags, lookaround,
backreferences, Unicode sets, mutation and jar ordering with the pinned SDK.
