//! Control roles preserve idle epochs and allow bounded cleanup through drain.

use super::fixtures::*;
use canonic_background_runtime::{
    Error, client,
    core::{Admission, ControlRole, Explicit, Hold},
    endpoint::Lease,
    protocol::{Control, ProductError, Request},
    server::{self, Context, Handler},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Controls {
    held: Mutex<Option<Hold>>,
    calls: AtomicUsize,
}
impl Handler for Controls {
    fn call(&self, _: &Request, _: &Context<'_>) -> Result<Value, ProductError> {
        Ok(Value::Null)
    }
    fn extensions(&self) -> &[&str] {
        &["acme_inspect", "acme_work", "acme_cleanup"]
    }
    fn control_role(&self, request: &Control) -> ControlRole {
        match request.action.as_str() {
            "acme_inspect" => ControlRole::Inspection,
            "acme_cleanup" => ControlRole::Cleanup,
            _ => ControlRole::Work,
        }
    }
    fn control(&self, request: &Control, context: &Context<'_>) -> Result<Value, ProductError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if request.action == "acme_inspect" {
            assert!(matches!(context.permit.hold(), Err(Error::ReadOnly)));
            return Ok(json!("inspection"));
        }
        if request.action == "acme_cleanup" {
            // This mutation must remain tracked even after the final existing hold settles.
            let cleanup = context.permit.hold().unwrap();
            self.held.lock().unwrap().take();
            drop(cleanup);
        }
        Ok(json!("complete"))
    }
}

#[test]
fn inspection_preserves_idle_and_cleanup_remains_available_during_drain() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Controls {
        held: Mutex::new(None),
        calls: AtomicUsize::new(0),
    });
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let runtime_gate = gate.clone();
    let runtime_handler = handler.clone();
    let server = std::thread::spawn(move || {
        server::serve(
            lease,
            identity(),
            runtime_gate,
            runtime_handler,
            &Explicit,
            server::Options::default(),
        )
    });
    ready(&root);
    let mut missing = control(&root, "acme_cleanup");
    missing.requires.push("acme_missing".into());
    assert!(matches!(
        client::control(&root, &missing, &options()),
        Err(Error::Incompatible)
    ));
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    wait(|| (gate.snapshot().work == 0).then_some(()));
    let _ = gate.events();
    for _ in 0..3 {
        assert_eq!(
            client::control(&root, &control(&root, "acme_inspect"), &options())
                .unwrap()
                .data,
            "inspection"
        );
    }
    assert!(
        !gate
            .events()
            .contains(&canonic_background_runtime::core::Event::IdleCancelled)
    );
    assert_eq!(
        client::control(&root, &control(&root, "acme_work"), &options())
            .unwrap()
            .data,
        "complete"
    );
    *handler.held.lock().unwrap() = Some(gate.hold().unwrap());
    stop(&root, "drain");
    let before = handler.calls.load(Ordering::SeqCst);
    assert!(matches!(
        client::control(&root, &control(&root, "acme_work"), &options()),
        Err(Error::Draining)
    ));
    assert_eq!(handler.calls.load(Ordering::SeqCst), before);
    assert_eq!(
        client::control(&root, &control(&root, "acme_inspect"), &options())
            .unwrap()
            .data,
        "inspection"
    );
    assert_eq!(
        client::control(&root, &control(&root, "acme_cleanup"), &options())
            .unwrap()
            .data,
        "complete"
    );
    server.join().unwrap().unwrap();
    assert!(!root.leased().unwrap());
}
