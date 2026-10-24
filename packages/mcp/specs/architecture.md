# Architecture

## Modules

| Module | Layer | Owns |
| --- | --- | --- |
| `json` | Core | Strict parsing; bounded, interruptible encoding and measurement |
| `rpc` | Core | Message classification, ID rules, error codes, reply builders and envelope encoding |
| `control` | Core | Cancellation flag and monotonic deadline |
| `schema` | Core | The enforced JSON Schema subset and the catalog's keyword check |
| `tool` | Core | Tools, annotations, the catalog, call parameters, results and content |
| `session` | Core | The connection state machine, outstanding IDs and confirmation requests |
| `stdio` | Driver | The public driver API, the dispatcher loop and output writing |
| `stdio::reader` | Driver | Bounded frame reading on the reader thread |
| `stdio::scheduler` | Driver | Lanes, keys, workers, deadlines, cancellation, confirmation waits and end of input |

Dependencies point inward: `session` uses `json`, `rpc`, `schema` and `tool`;
`stdio` uses the core. The core never starts threads or performs I/O.

## Threads and state

- The dispatcher thread owns the `Session` and all scheduling state. Workers
  never touch either; they receive one assignment at a time and send back an
  encoded reply, an error reply, a confirmation step or nothing.
- Workers and the reader wake the dispatcher by unparking it after sending.
  The dispatcher parks until the earliest pending deadline or indefinitely.
- Worker results use an unbounded channel bounded in practice by the worker
  count; reader frames and progress use bounded channels, so a flooding input
  cannot block a worker from reporting.

## Conventions

- Rust 1.99.0, edition 2024, one package with a committed `Cargo.lock`.
- `unsafe_code` is forbidden; Clippy pedantic and `unwrap_used` warn, and
  checks run with warnings denied. Modules are self-named files.
- Public fallible functions return typed `thiserror` errors and document them.
- Dependencies are `serde`, `serde_json` and `thiserror` only. Base64 for
  image content is implemented in `tool`.
