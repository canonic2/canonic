# Build a live manager handover

External drain requests use `Admission::try_drain`: they can cancel reversible
preparation, but refuse once handover is irreversibly fenced. The server's built-in
`drain` follows this rule. `Admission::drain` remains a local reconciliation
primitive; products must verify which resources they still own before invoking
cleanup after a fenced failure.

Use planned handover when a replacement manager must take over without stopping
live resources. Process Kit supplies admission fences and bounded copies of files
and sockets. Your application owns candidate selection, resource inventories,
durable ownership, adoption and recovery. These APIs are opt-in;
`client::connect_versioned` still requests natural retirement.

Cross-process resource transfer has native acceptance on macOS, Linux and Windows.
Windows acceptance covers same-user processes in one session; transfers across
integrity/logon boundaries and provider failure paths remain unverified.

## Keep resource owners stable

A small supervisor can retain children, PTYs, log draining and exit statuses while
managers change. Managers transfer control authority through an application
protocol. For resources inside the manager, `transfer` copies `Resource::File`
(including pipe ends represented by `File`), `TcpListener`, `TcpStream`,
`LocalListener` and `LocalStream`. Local types come from `net`.

Copies share kernel state. They do not copy buffered readers, threads, callbacks,
event-loop registrations, child-parent relationships or application authority.
Pause old readers/writers and keep standby copies idle until ownership commits.
Dropping a copy closes it; calling `shutdown` can affect every socket duplicate.

## Prepare, fence and retire

Use the `Admission` supplied to `server::serve`:

```rust
use canonic_process_kit::{Result, core::Admission};

fn abandon_preparation(gate: &Admission) -> Result<()> {
    let preparation = gate.quiesce()?;
    // Validate the candidate and prepare its inventory here.
    // Before fencing, returning an error also resumes admission on drop.
    preparation.abort()
}
```

Quiescence rejects new ordinary requests, work controls and unscoped holds with
the existing `Draining` refusal. Inspection controls continue answering; existing
permits/holds retain their obligations and may finish or split them. Cleanup
controls may settle resources during reversible preparation. Idle exit and pending
version retirement wait. `Snapshot::quiescing` identifies this pause. No drain
cleanup callback runs.

A successful application integration follows this order:

1. Verify candidate identity and compatibility, then start it without product
   writer authority on a fresh private transfer channel.
2. Quiesce admission, pause background writers/schedulers and let mutations
   finish. Prepare and validate the candidate's standby resource inventory.
3. Call `Quiescence::fence()` **before durable ownership commit**. It requires
   zero mutation permits, permits existing holds to remain, and also refuses
   cleanup mutations. From here, drop/abort cannot reopen admission.
   `Snapshot::handover_fenced` exposes this boundary.
4. Revalidate the prepared inventory and state revision after fencing, then
   commit the application's durable transfer record. Stop old resource access
   and release each hold only after verified completion or transfer of its
   responsibility. Failed fenced attempts require reconciliation, drain or exit.
5. Call `commit()`: it atomically requires zero mutations and zero holds, then
   irreversibly retires without drain cleanup. `HandoverBusy` preserves the fence.
   Existing inspection replies finish before `server::serve` returns.
6. Release all old application writer locks and the lease. The successor acquires
   a fresh `Lease`, validates the committed attempt, imports ownership and creates
   new resource holds **before** advertising readiness.

Dropping/aborting preparation before fencing resumes admission with a fresh idle
epoch. An explicit drain permanently preempts preparation and follows normal
cleanup. After fencing, `abort` returns `HandoverState` and leaves admission closed.
Products serialize drain, transfer and durable commit; admission cannot stop
background code holding independent write access. The final lifecycle exit reason
is `Retired`, keeping the existing event/control protocol.

Every startup path must honor the durable transfer record, including unrelated
clients racing to start a manager after lease release. Never duplicate a writer
lease to create two apparent owners. A precommit candidate cannot modify live
state to test its compatibility. Missing acknowledgements after commit cannot
authorize the old writer to resume.

## Copy resources on a dedicated channel

Both ends independently supply the same `transfer::Binding`: a fresh transfer ID
and exact predecessor/successor incarnations. Fields use request-ID syntax:
1–128 ASCII letters, digits, hyphens or underscores; incarnations differ. These
labels bind the transcript, not executable identity or durable authority.

Run the two sides concurrently. The sender retains the original:

```rust
use canonic_process_kit::{Result, net::Stream, transfer::{Binding, Resource, Sender}};
use std::time::Duration;

fn send_file(stream: Stream, expected: Binding, original: &Resource) -> Result<()> {
    let mut sender = Sender::new(stream, expected, Duration::from_secs(3))?;
    sender.send(original, serde_json::json!({"name": "acme-log", "schema": 1}))
}
```

The successor imports a standby resource:

```rust
use canonic_process_kit::{Result, net::Stream, transfer::{Binding, Receiver, Resource}};
use std::time::Duration;

fn receive_file(stream: Stream, expected: Binding) -> Result<(Resource, serde_json::Value)> {
    let mut receiver = Receiver::new(stream, expected, Duration::from_secs(3))?;
    receiver.receive()
    // The caller validates metadata and keeps the copy idle until ownership commits.
}
```

Use a fresh stream exclusively for this transcript; do not clone or interleave
messages on it. Each resource has a sequence and import acknowledgement. A session
allows 64 copies, with 64 KiB encoded metadata per copy. Each handshake/copy gets
one absolute I/O deadline, including interruptions and acknowledgement; the timeout
must be positive and fit the monotonic clock. Products bound total preparation
and native API call duration separately.

Successful `send` proves import, not product validation or durable commit. Keep
originals and old holds until those are separately confirmed. Once sending may
have begun, any failure returns `OutcomeUnknown(transfer_id)`; reconcile instead
of automatically retrying. Transport/protocol errors poison the channel; reuse
returns `HandoverState`. Local capacity/metadata refusals precede transmission.
Failed receives close imported local resources. Product metadata can include
generation/schema identity, buffers or references to validated snapshots.

## Platform boundaries

Unix uses `SCM_RIGHTS`. Received descriptors become close-on-exec, atomically on
Linux. macOS uses `fcntl` after receipt, leaving a small inheritance window during
concurrent inheriting child creation. Applications needing strict inheritance
isolation serialize that spawn boundary. Transfer-channel timeouts do not freeze
the copied resources' own nonblocking flags or timeouts.

Windows checks the actual channel peer PID and token user. Sockets use
`WSADuplicateSocketW`/`WSASocketW`; file/pipe imports pull with `DuplicateHandle`
while the sender retains the original through acknowledgement. Imported resources
are non-inheritable. Native structures are serialized field by field, excluding
padding. File pulls require `PROCESS_DUP_HANDLE` access to the sender, which
Windows may deny across integrity/logon boundaries even for one account. Provider
failures are reported, not emulated with a restart. Failed candidates should exit
to release any provider-owned pending import state.

Process/job/ConPTY ownership, application buffers and child reaping are not
generalized as file transfers. Retain their native owner or implement application
adoption. Old clients need a compatible request protocol or a reconnect path.
Both manager incarnations need the application's handover protocol before these
primitives can preserve its work across updates.
