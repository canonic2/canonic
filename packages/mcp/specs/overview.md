# Overview

## Purpose

Servers that expose tools to AI agents over the Model Context Protocol need
the same mechanics: strict newline-delimited JSON-RPC, the initialization
handshake, a validated tool list, argument checks, bounded replies,
cancellation, deadlines, progress, host confirmation and well-defined behavior
when the host disconnects. `canonic-mcp` owns those mechanics once, so each
server implements only its tools.

## Layers

| Layer | Modules | Contract |
| --- | --- | --- |
| Protocol core | `json`, `rpc`, `schema`, `tool`, `control`, `Session` | I/O-free and thread-free. A caller feeds frames to a `Session` and acts on each `Inbound`. Any runtime, synchronous or asynchronous, can drive it. |
| Stdio driver | `stdio` | Runs a `Handler` over a reader and a writer with a reader thread, the calling thread as dispatcher and worker threads grouped into lanes. |

A server with its own runtime uses the core and implements scheduling itself;
it gets identical framing, lifecycle, validation, cancellation and
confirmation semantics. A synchronous server uses the driver.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Host | The MCP client, such as an agent application, and its user interface. |
| Frame | One newline-terminated JSON-RPC message. |
| Outstanding request | A tool call or extension request admitted by the session and not yet answered or cancelled. |
| Lane | A group of worker threads with its own queue and capacity. |
| Key | A string that serializes requests within one lane. |
| Read-only tool | A tool annotated `readOnlyHint: true`; the driver lets its work finish after the input ends. |
| Mutation | Any other tool call, extension request or notification. |
| Confirmation | A form `elicitation/create` request whose answer the host reports. |
| Continuation | The work a request runs with the confirmation decision. |
| Extension method | A product request or notification method the server lists in its session options. |

## Responsibilities

The library decides:

- framing, strict parsing, message classification and reply shapes;
- the initialization lifecycle and protocol version;
- catalog validity and argument validation against input schemas;
- outstanding request tracking, duplicate refusal and cancellation;
- (driver) admission, ordering, deadlines, progress delivery, confirmation
  waits and end-of-input rules.

The server owns:

- its tools, their behavior and what they are allowed to change;
- its identity, instructions and the wording of results and errors;
- which lane, key and deadline each request uses;
- what a confirmation asks and what each decision does;
- process lifetime, signals and anything written outside protocol frames.

The library never retries work, never treats a client's self-reported name or
capabilities as authority and never grants authority itself: a
[confirmation](confirmation.md) only reports the host user's choice.

## Status

The protocol core and stdio driver are implemented and covered by unit and
integration tests; see [verification](verification.md). Servers depend on the
library through local path dependencies and own their acceptance with real
hosts.
