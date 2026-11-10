# Claude Agent SDK read-only integration

Use `createClaudeReadOnlyHooks` to let a Claude Agent SDK host read selected
plain-text files. Reads need a policy allowance or exact operator approval.
The default hook factory denies other tools, including Bash, Write, Edit, Agent,
and MCP. An explicit interactive session adds only the Shield request-review
tools described below.

The adapter uses Claude Agent SDK 0.3.292 and is tested on macOS with its Claude
Code 2.1.292 process and a local provider. It has no runtime dependencies; your
host supplies the SDK. It does not install CLI settings. For a normal coding
session, use [native project setup](claude-project.md).

## Explicit general-runtime registration

The separate `claude-runtime-read-only` module opts into `general-runtime-v1`.
Its fixed read-only mapping requires its own operator review:

```sh
shield sdk setup --adapter claude-read-only --project /path/to/acme
```

Review the binary and adapter content identities shown in the terminal. Native
setup and the existing `createClaudeReadOnlyHooks`/`createClaudeReadOnlySession`
factories do not supply this registration. Those existing factories keep their
current behavior and require no new registration.

The successful receipt prints the binary SHA-256, adapter mapping SHA-256 and
policy lineage without machine paths. Save the reviewed binary SHA-256 in
trusted application configuration and pass it as the required `binaryDigest`
option to `createClaudeRuntimeReadOnlyClient`. Never obtain the expected digest
from the evaluator's status or another reply from that same child.

The client exposes `hooks` and an asynchronous `close()` method; close it in your
application's `finally` cleanup after SDK work finishes.

The new profile belongs to this developer and policy lineage. It follows the
project across locations when binary and adapter contents match; changing either
requires a new review. Each new factory obtains a private checkout-bound context
valid for one hour. Copies and moves cannot replay an old context; create a fresh
factory. At most 128 contexts are retained per developer. No context token is
passed in model input or command arguments.

The profile authorizes only the fixed read mapping. It adds no policy permission
and cannot bypass Deny or the exact-read approval required by Ask. Callback
session labels are attribution, not proof of a native Claude session. This new
mapping has no installed-host acceptance claim; the SDK/version evidence below
describes the retained adapter.

Import `createClaudeRuntimeReadOnlyClient` from
`adapters/src/modules/claude-runtime-read-only/index.mjs` for this profile. It
returns `{hooks, close}`. Pass `hooks` to your trusted SDK host and await `close()`
when the session ends. Supply `binary`, `project`, the reviewed `binaryDigest`
and optional `timeoutMs` from trusted application configuration. Closing cancels
this client's pending approval waits and releases its private context after its
evaluator children finish.

The parent independently hashes the opened native executable, then checks its
file version and every canonical ancestor directory before and after each child.
Changed or unavailable evidence refuses the child receipt. The new factory
supports this check on macOS and Linux; Windows has no adopted equivalent and
refuses this profile. This is a receipt check under a trusted installation
premise. A temporary path substitution could still start another image before
the change is detected and could expose the child's environment. Keep the binary
and its directories outside untrusted write access. There is no atomic executable
image handoff or OS process confinement.

The new client passes only captured home/cache, locale, timezone, terminal and
temporary-directory locators to its native children, adding its private context
token only for evaluated calls and closure. Loader variables, `PATH`, Node options
and unrelated application environment are not inherited. This launch environment
does not attest the host Read operation; normalized execution evidence stays null.

Ask blocks by default. A trusted application may supply
`onApprovalRequired: async ({request_id}, {signal}) => waitForOperator(request_id, signal)`.
This handler receives only a public request ID. It must use the existing human
approval channel; returning cannot issue a grant. The original Read callback then
separately consumes the exact approved request. Human interaction must finish
within that callback's total configured deadline, at most 60 seconds. A later
Read callback starts a fresh request, even with identical text or a reused tool
ID. The client never gives private retry references to the model or handler.

Each client admits at most eight concurrent callbacks with no queue. Additional
PreToolUse calls deny without starting a child; completion overload reports lost
optional history. This is a per-client limit, not a host-wide process bound.
This profile keeps up to 128 recent correlation entries for 70 minutes. Its
128-entry host-ID ambiguity ledger is retained for the entire client lifetime;
when full, additional IDs remain untracked instead of evicting earlier evidence. Exact
completion retries retain their original delivery identity for history inspection.
A new PreToolUse with a reused host ID invalidates that correlation. Missing or
mismatched evidence remains unlinked. These bounds describe the new profile;
the retained adapter's lifecycle is described below.

## Policy and registration

Initialize the project using [the setup guide](getting-started.md). Reads match
file rules against the canonical path. This rule permits `notes.txt`:

```json
{
  "id": "acme-notes",
  "decision": "allow",
  "reason": "Read Acme notes",
  "conditions": {
    "domain": "file",
    "action": "read",
    "sources": {"all": {"exact": {"root": "project", "path": "notes.txt"}}}
  }
}
```

Declare `structured-rules-v1` and `location-v1` in the containing custom document.
See the [rule guide](policy-rules.md) for the document format.

An unmatched read returns Ask: the adapter blocks it and stores a pending
request for operator review. Deny blocks without creating a request. Native
prompts cannot bypass these outcomes, and rules cannot enable unsupported tools.

Import the adapter from this private checkout in a trusted SDK host:

```js
import { query } from '@anthropic-ai/claude-agent-sdk';
import { createClaudeReadOnlyHooks } from '/path/to/canonic/packages/shield/adapters/src/modules/claude-read-only/index.mjs';

const root = '/path/to/acme';
const hooks = await createClaudeReadOnlyHooks({
  binary: '/path/to/canonic/packages/shield/target/release/shield',
  project: root,
  timeoutMs: 5000,
});

for await (const message of query({
  prompt: 'Use Read to inspect /path/to/acme/notes.txt.',
  options: {
    cwd: root, hooks, settingSources: [], strictMcpConfig: true,
    mcpServers: {}, persistSession: false,
  },
})) {
  // Present the message using your host application's normal interface.
}
```

Replace the paths, create `notes.txt`, and activate the example rule before
running your host. Its Read can then proceed. Without a matching allowance,
the read enters the [operator approval flow](#one-time-operator-approval).

Keep the catch-all hook and transport outside agent-controlled code. Do not
narrow its matcher or add input-changing hooks, settings, or plugins after it.
The example disables settings-file hooks and configured MCP servers. Only the
trusted host can select the binary, project, and access classification.

## Approve reads in the SDK session

Use `createClaudeReadOnlySession` when your trusted SDK application provides a
human form interface. The project's Claude MCP integration must already have
been reviewed during [project setup](claude-project.md). This uses that registered
confirmation channel; an SDK label alone establishes no native authority.

```js
import { query } from '@anthropic-ai/claude-agent-sdk';
import { createClaudeReadOnlySession } from '/path/to/canonic/packages/shield/adapters/src/modules/claude-read-only/index.mjs';

async function readWithApproval(showHumanForm) {
  const options = await createClaudeReadOnlySession({
    binary: '/path/to/canonic/packages/shield/target/release/shield',
    project: '/path/to/acme',
    onElicitation: (request, context) => showHumanForm(request, context.signal),
  });
  for await (const message of query({
    prompt: 'Read /path/to/acme/notes.txt, requesting approval if needed.',
    options: { ...options, cwd: '/path/to/acme', persistSession: false },
  })) {
    // Present the message in your application's interface.
  }
}
```

Your `showHumanForm` waits for the person's answer and returns the SDK elicitation
result, such as `{action: 'accept', content: {}}`, `{action: 'decline'}` or
`{action: 'cancel'}`. Never infer acceptance from model output or ordinary tool
permission. The helper forwards only Shield form requests to that handler.

Pass the returned hooks, fixed session ID, MCP server and elicitation handler
together. On Ask, the Read result names `mcp__shield__request_approve` and its
captured `request_id`. The agent can also use `request_show` or `request_deny`
for that factory's captured IDs. These are the only permitted MCP calls. Each
request still goes through Shield's human confirmation and resource checks;
the callback's permission to call Shield issues no grant itself. Retry the read
once unchanged after acceptance. A different factory, session, path or window
cannot consume it. Policy editing, other MCP servers and process tools remain
outside this read-only session.

The pinned SDK's elicitation callback and exact read retry are covered by owned
local-provider tests for Accept and Decline. This is an application-supplied form
interface; the noninteractive example above supplies no such interface.

## Supported reads and failures

The factory fixes the project, binary, and enrollment ID at startup. Reenrolling
the project invalidates subsequent calls; each call otherwise reloads active policy.

A supported read must meet these limits:

- An existing regular `.txt` file, at most 65,536 bytes, containing valid UTF-8
  with no NUL or PDF header.
- Inside the project's confirmed readable roots and outside `.shield` under
  those roots. Additional `access_paths.read` or `write` roots require activation.
- No file or parent symlinks, hard links, directories, or special files.
- A cwd inside the project. Optional offset and limit are integers from 1 to
  1,000,000; unknown Read fields block.

Shield rechecks roots, file identity, descriptor, and pathname metadata at
handoff and approval. Only the checked path/window reaches `updatedInput`;
omitted window fields stay omitted.

The transport starts only the Rust evaluator, using direct argv and bounded
stdin/stdout. Evaluator failures, invalid replies or state, deadlines, and
cancellation return SDK deny. The deadline accepts 1–60,000 ms; the SDK callback
adds a cleanup margin. Startup and filesystem calls have no hard time bound.
SDK callback timeouts block, unlike some other hook transports. Do not register
`enforce-request` as a command-hook replacement.

## One-time operator approval

A noninteractive blocked Ask reports a stored request ID. Inspect it in your terminal, approve
it, then retry the identical read in the same factory and session. Use `deny`
only if you want to reject the request or revoke an unused grant:

```sh
shield request list --project /path/to/acme
shield request show REQUEST_ID --project /path/to/acme
shield request approve REQUEST_ID --project /path/to/acme
shield request deny REQUEST_ID --project /path/to/acme
```

Pending requests expire after ten minutes; approval lasts five minutes.
Repeating an approval returns the same grant without renewing it. Revocation
is final. Inspection redacts sensitive metadata and stores no file contents.

The grant binds enrollment, activation, revision, snapshot digest, canonical
path, cwd, offset/limit, file identity/metadata/content digest, session, and a
factory-owned random consumer ID. The original tool-call ID records provenance;
the retry has its own ID. A private factory map supplies the pending reference,
so agent input cannot supply an approval flag. A changed file, policy, factory,
session, or window requires a fresh request. Reconnecting also requires approval.

Rust saves consumption before returning `allowed-once`; concurrent retries can
allow at most one read. Lost replies, cancellation, host refusal, or missing
completion do not restore the grant. Consumption records permission, not execution.

Each project and factory retains at most 128 live request references. Full terminal
and expired records are reclaimed. A compact consumed/denied outcome remains for
up to ten minutes, within a separate 128-receipt bound; it can be evicted earlier
by newer terminal outcomes. Repeating that decision reports the recorded outcome
without another permission, approval ID or handoff. An old explicit reference
cannot fall through to a later Allow policy. Unknown references and a full live
store refuse. Clearing optional history preserves authorization records, which
contain identity, digests and metadata, never file contents.

## Scope of protection

The SDK reads by pathname, not by Shield's checked descriptor. External
same-user processes can replace paths after handoff; the adapter provides no
OS isolation or atomic descriptor transfer.

Automatic context loading and prompt `@` references are outside PreToolUse.
The hook cannot filter those reads or stop unrestricted processes from invoking
the operator CLI or editing state. PDFs, images, notebooks, writes, and process
execution are unsupported. Native Linux/Windows behavior is unverified.

## Decision and completion history

Completion callbacks acknowledge raw history publication; history inspection
verifies links and duplicate deliveries later. The SDK validates a versioned
receipt with deferred correlation and checks any echoed request reference against
the one it submitted. That acknowledgement grants no permission. A malformed or
incompatible receipt produces the history warning while preserving protection.
The current SDK receipt also includes a registry-trust revision in its snapshot
identity. Keep the adapter and Shield binary on compatible protocol versions.

History is enabled by default. It records policy revision, activation, operation,
available session/tool-call IDs, evaluation, and handoff result. An approved Ask
keeps its original decision and links the consumed grant. Unsupported tools have
no policy decision. An allowed handoff alone does not establish execution.

`PostToolUse` and `PostToolUseFailure` link a Read completion only when project,
session, tool-call ID, cwd, and path/window match the retained decision. Success
records `completed`; failure records `failed`, or `cancelled` for
`is_interrupt: true`. History omits tool responses, file contents, transcripts,
error strings, and environment. Unavailable version, time, and exit code stay null.

Each factory keeps at most 512 correlation entries without evicting them.
Additional calls remain untracked. Missing entries, reused IDs, mismatched input,
and purged or expired decisions leave completions unlinked. Identical deliveries
appear once; changed content under one delivery ID is marked conflicting.
Uncorrelated callbacks can produce separate observations. Missing callbacks,
including native permission or validation refusals, do not imply success.

History appends after the enforcement lock is released, retaining the original
evaluation. Write failures, malformed callbacks, and transport failures report
`Shield history unavailable` through `systemMessage`, without changing permission.
Process death or transport loss can leave a decision without completion.

Use `shield history list --project /path/to/acme` to inspect entries. The history
action in `shield policy review` controls recording and retention. See
[project history](project-review.md#request-history).

The vendor [hook reference](https://code.claude.com/docs/en/hooks) and
[SDK hook guide](https://code.claude.com/docs/en/agent-sdk/hooks) describe the host protocol.
