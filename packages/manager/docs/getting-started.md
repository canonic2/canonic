# A minimal manager and client

Add the local dependency shown in the package README. The following program
uses an explicit root passed to both modes. Its internal `manager-internal`
mode serves requests; its ordinary mode explicitly chooses to start an absent
manager. Production CLI parsing should hide the internal mode.

```rust,no_run
use canonic_manager::{Result, client, core::{Admission, IdleGrace},
    endpoint::{Lease, Root}, launch::Spawn, protocol::{Identity, Request},
    server::{self, Context, Handler}};
use serde_json::{Value, json};
use std::{fs::OpenOptions, path::Path, sync::Arc, time::Duration};

struct Acme;
impl Handler for Acme {
    fn call(&self, request: &Request, _context: &Context<'_>)
        -> std::result::Result<Value, canonic_manager::protocol::ProductError> {
        Ok(json!({"echo": request.payload}))
    }
}

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let path = std::env::args_os().nth(2).expect("pass a private root");
    let root = Root::explicit(Path::new(&path))?;
    if mode == "manager-internal" {
        let lease = Lease::acquire(&root, Duration::from_secs(15))?;
        let identity = Identity::new("1.0.0".into(), 1, vec!["echo_v1".into()])?;
        return server::serve(lease, identity, Admission::new(), Arc::new(Acme),
            &IdleGrace(Duration::from_secs(30)), server::Options::default());
    }
    root.prepare()?;
    let launch = Spawn {
        args: vec!["manager-internal".into(), root.path().into()],
        environment: vec![],
        directory: root.path().into(),
        log: OpenOptions::new().create(true).append(true)
            .open(root.path().join("acme.log"))?,
    };
    let options = client::Options::default();
    let identity = client::connect_versioned(&root, &launch, "1.0.0", &options)?;
    identity.require(1, &["echo_v1".into()])?;
    let reply = client::call(&root, &Request { protocol: 1,
        request_id: "acme-unique-request-1".into(), requires: vec!["echo_v1".into()],
        payload: json!("hello") }, &options)?;
    println!("{}", reply.data);
    Ok(())
}
```

Run `acme client /short/private/root` from the built application's directory.
The reply contains `{"echo":"hello"}`. The manager exits after its 30-second
empty grace. Each real mutation needs a unique request ID and product-owned
deduplication against that ID and its content.

The versioned connection compares the client's release with the running
manager's `Identity::build`. A newer client replaces an empty older manager;
a busy older manager keeps its work and retires when it becomes empty. Use a
complete semantic version for both, typically `env!("CARGO_PKG_VERSION")`.
See [version handoff](reference.md#version-handoff) for ordering and inspection.

For a default per-user root, use
`Root::for_user(Path::new(".acme/run"), Home::Account)`. This ignores `HOME`.
`Home::Environment` is an explicit alternative. Inspection uses
`client::connect(&root, None, &options)` and does not prepare a root or start
anything. Treat `Error::Absent` as stopped for non-launching stop/drain commands.

For a resource that outlives a reply, acquire `context.permit.hold()?` before
publishing it and keep the returned `Hold` with that resource. Release it only
after cleanup finishes. `Admission::hold()` supports initial resources loaded
before readiness. During drain, already admitted mutations can transfer their
responsibility; new work cannot acquire a hold.

Use [the reference](reference.md) to add controls and choose a launch path.
