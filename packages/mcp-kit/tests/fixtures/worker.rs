//! Subprocess-only acceptance fixture: one actual MCP frontend and replaceable
//! implementations with private pipes. No installation or external resources.
use canonic_mcp_kit::{
    Catalog, Client, Content, RpcError, ServerInfo, Tool, ToolResult,
    stdio::{self, Context, Handler, Options, Outcome, Request},
    worker::{self, Contract, Frontend, Implementation, wire},
};
use canonic_process_kit::{
    self as process,
    endpoint::{Lease, Root},
    frame,
    protocol::Identity,
    worker::Worker,
};
use serde_json::json;
use std::{
    io::{self, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

fn catalog() -> Arc<Catalog> {
    Arc::new(
        Catalog::new(vec![Tool::new(
            "acme_echo",
            "Return the implementation process.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        )])
        .expect("catalog"),
    )
}
fn options() -> Options {
    Options {
        session: canonic_mcp_kit::SessionOptions {
            modern: Some(canonic_mcp_kit::ModernOptions::new(ServerInfo::new(
                "acme", "1.0.0",
            ))),
            ..Default::default()
        },
        ..Options::default()
    }
}
fn contract() -> Contract {
    Contract::new(&catalog(), &options(), json!({"policy":"acme-v1"}), 1024).expect("contract")
}
fn identity(id: &str, build: &str) -> Identity {
    Identity {
        manager_id: id.to_owned(),
        build: build.to_owned(),
        protocol: 1,
        control_version: process::protocol::CONTROL_VERSION,
        capabilities: vec!["mcp-worker-v1".into()],
    }
}

struct Native {
    identity: Identity,
    contract: Contract,
    child: Mutex<Child>,
    io: Mutex<(ChildStdin, BufReader<ChildStdout>)>,
    sequence: AtomicU64,
}
impl Native {
    fn launch(build: &str) -> Arc<dyn Implementation> {
        let mut child = Command::new(std::env::current_exe().expect("fixture path"))
            .args(["worker", build])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("worker spawn");
        let input = child.stdin.take().expect("stdin");
        let output = BufReader::new(child.stdout.take().expect("stdout"));
        Arc::new(Self {
            identity: identity(&format!("worker-{}", child.id()), build),
            contract: contract(),
            child: Mutex::new(child),
            io: Mutex::new((input, output)),
            sequence: AtomicU64::new(0),
        })
    }
}
impl Worker for Native {
    fn identity(&self) -> &Identity {
        &self.identity
    }
    fn prepare(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        if self.child.lock().expect("child").try_wait()?.is_some() {
            return Err(process::Error::Worker);
        }
        Ok(())
    }
    fn fence(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        Ok(())
    }
    fn retire(&self, transition: &worker::Transition<'_>) -> process::Result<()> {
        self.stop(transition.deadline)
    }
    fn activate(&self, _: &worker::Transition<'_>) -> process::Result<()> {
        Ok(())
    }
    fn stop(&self, _: Instant) -> process::Result<()> {
        let mut child = self.child.lock().expect("child");
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        child.wait()?;
        Ok(())
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.stop(Instant::now() + Duration::from_secs(2));
    }
}
impl Implementation for Native {
    fn contract(&self) -> &Contract {
        &self.contract
    }
    fn call(&self, request: Request, context: &Context) -> Result<Outcome, worker::Failure> {
        let sequence = self
            .sequence
            .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| RpcError::new(-32603, "sequence exhausted"))?
            + 1;
        let mut io = self.io.lock().expect("private channel");
        frame::write(&mut io.0, b"request", 16).expect("request readiness");
        io.0.flush().expect("readiness request flush");
        // The worker starts its monotonic budget before this readiness frame.
        // Capture the source's remaining time only after observing it, so the
        // receiver's deadline cannot gain transport/decoding time.
        assert_eq!(frame::read(&mut io.1, 16).expect("worker ready"), b"ready");
        let call = wire::Call::new(
            wire::Assignment {
                frontend: "acme-frontend".into(),
                worker: self.identity.manager_id.clone(),
                sequence,
            },
            &request,
            context,
        )?;
        frame::write(
            &mut io.0,
            &wire::encode(&call, 1024 * 1024).map_err(|_| RpcError::new(-32603, "encode"))?,
            1024 * 1024,
        )
        .map_err(|_| RpcError::new(-32603, "channel"))?;
        let bytes =
            frame::read(&mut io.1, 1024 * 1024).map_err(|_| RpcError::new(-32603, "channel"))?;
        let reply: wire::Reply = wire::decode(&bytes, 1024 * 1024)?;
        reply.check(&call.assignment)?;
        Ok(reply.outcome(&call.assignment, context)?)
    }
}

struct Resident;
impl Handler for Resident {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1.0.0"))
    }
    fn call(&self, _: Request, _: &Context) -> Outcome {
        panic!("execution belongs to implementation")
    }
}

fn run_worker(build: &str) {
    let mut input = io::stdin().lock();
    let mut output = io::stdout();
    let decoder = wire::Decoder::new(catalog(), options().session).expect("worker declarations");
    while let Ok(intent) = frame::read(&mut input, 16) {
        assert_eq!(intent, b"request");
        let checkpoint = Instant::now();
        frame::write(&mut output, b"ready", 16).expect("worker ready");
        output.flush().expect("readiness flush");
        let Ok(bytes) = frame::read(&mut input, 1024 * 1024) else {
            break;
        };
        let call: wire::Call = wire::decode(&bytes, 1024 * 1024).expect("private call");
        call.check("acme-frontend", &format!("worker-{}", std::process::id()))
            .expect("channel identity");
        let control = Arc::new(canonic_mcp_kit::Control::until(
            checkpoint + Duration::from_millis(call.remaining_ms),
        ));
        let (request, context) = call
            .restore(&call.assignment, &decoder, control, None)
            .expect("typed work and context");
        assert!(matches!(request, Request::Tool(_)));
        context.check().expect("original budget");
        let mut result = ToolResult::success(vec![Content::text(format!(
            "{}:{}",
            build,
            std::process::id()
        ))]);
        result.structured_content = Some(json!({
            "id":context.request_id(),
            "client":context.request_context().and_then(|protocol| protocol.client_info.as_ref()),
            "state":context.request_context().and_then(|protocol| protocol.request_state.as_deref()),
            "metadata":context.request_context().map(|protocol| protocol.metadata().as_map()),
        }));
        let reply = wire::Reply::terminal(call.assignment, &Outcome::Tool(result), 1024 * 1024)
            .expect("typed terminal result");
        frame::write(
            &mut output,
            &wire::encode(&reply, 1024 * 1024).expect("encode"),
            1024 * 1024,
        )
        .expect("reply");
        output.flush().expect("private pipe flush");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|role| role == "worker") {
        run_worker(args.get(2).expect("worker build"));
        return;
    }
    let root = Root::explicit(args.get(1).expect("owned root")).expect("root");
    let lease = Lease::acquire(&root, Duration::from_secs(2)).expect("lease");
    let frontend = Frontend::new(
        lease,
        identity("acme-frontend", "1.0.0"),
        Native::launch("1.0.0"),
        worker::SupervisorOptions {
            preparation: Duration::from_secs(2),
            activation: Duration::from_secs(2),
            cleanup: Duration::from_secs(2),
            capacity: 8,
            history: 4,
            history_bytes: 65536,
        },
        1024 * 1024,
        8,
    )
    .expect("frontend");
    // A private fixture controller supplies the authorized update signal. It
    // never invokes lifecycle callbacks on the MCP dispatcher.
    let control_root = Root::explicit(root.path().join("control")).expect("control root");
    let control_lease =
        Lease::acquire(&control_root, Duration::from_secs(2)).expect("control lease");
    let controller = frontend.clone();
    let (ready, waiting) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let endpoint = control_lease.bind(false).expect("control endpoint");
        endpoint
            .listener()
            .set_nonblocking(false)
            .expect("blocking fixture controller");
        ready.send(()).expect("ready");
        let (stream, _) = endpoint.listener().accept().expect("control accept");
        process::endpoint::check_peer(&stream).expect("private peer");
        let mut io = frame::Deadline::new(&stream, Instant::now() + Duration::from_secs(5));
        let command = frame::read(&mut io, 16).expect("update trigger");
        assert_eq!(command, b"update");
        controller
            .replace(
                &worker::Request {
                    frontend: "acme-frontend".into(),
                    sequence: 1,
                    selection: worker::Selection {
                        key: "acme-v2".into(),
                        build: "2.0.0".into(),
                        protocol: 1,
                        requires: vec!["mcp-worker-v1".into()],
                    },
                },
                |_| Ok(Native::launch("2.0.0")),
                |_| Ok(()),
            )
            .expect("replacement");
        frame::write(&mut io, b"replaced", 16).expect("update acknowledgement");
    });
    waiting
        .recv_timeout(Duration::from_secs(5))
        .expect("controller readiness");
    let mut options = options();
    options.worker = Some(frontend.clone());
    #[expect(
        clippy::bufreader_stdin,
        reason = "stdio::serve moves the reader to another thread and StdinLock is not Send"
    )]
    let input = BufReader::new(io::stdin());
    stdio::serve(Arc::new(Resident), catalog(), input, io::stdout(), &options).expect("serve");
}
