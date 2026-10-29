# Claude Agent SDK read-only integration

Use `createClaudeReadOnlyHooks` to let a Claude Agent SDK host read selected
plain-text files. Reads need a policy allowance or exact operator approval.
All other tools, including Bash, Write, Edit, Agent, and MCP, are denied.

The adapter uses Claude Agent SDK 0.3.292 and is tested on macOS with its Claude
Code 2.1.292 process and a local provider. It has no runtime dependencies; your
host supplies the SDK. It does not install CLI settings. For a normal coding
session, use [native project setup](claude-project.md).

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

A blocked Ask reports a stored request ID. Inspect it in your terminal, approve
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

Each project and factory retains at most 128 request references. Terminal and
expired project records are reclaimed; unknown references and a full store
refuse requests. Clearing optional history preserves authorization records,
which contain file identity, digest, and metadata, never contents.

## Scope of protection

The SDK reads by pathname, not by Shield's checked descriptor. External
same-user processes can replace paths after handoff; the adapter provides no
OS isolation or atomic descriptor transfer.

Automatic context loading and prompt `@` references are outside PreToolUse.
The hook cannot filter those reads or stop unrestricted processes from invoking
the operator CLI or editing state. PDFs, images, notebooks, writes, and process
execution are unsupported. Native Linux/Windows behavior is unverified.

## Decision and completion history

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
