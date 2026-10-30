# Background runtime documentation

Use this library when many short-lived commands need one per-user process
to retain shared live state. Your application decides what that state means
and when it can be released.

Start with [a minimal manager and client](getting-started.md). Then read the
[API reference](reference.md) for admission transfers, control extensions,
launch choices, limits and failure handling. [Development](development.md)
describes tests and checks.
