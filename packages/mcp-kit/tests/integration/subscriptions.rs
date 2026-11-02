//! Subscription streams share the dispatcher without occupying tool workers.
use crate::fixture::{self, Events, Host, Latch, Script, WAIT};
use canonic_mcp_kit::{
    Catalog, Change, Client, ModernOptions, Resource, ResourceCatalog, RpcError, ServerInfo,
    SubscriptionOptions, ToolResult, modern, rpc,
    stdio::{ChangeSender, Context, Handler, Options, Outcome, Request, SendChangeError},
    subscription::SUBSCRIPTION_KEY,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

struct Producer {
    sender: mpsc::Sender<ChangeSender>,
    script: Script,
}

impl Handler for Producer {
    fn connected(&self, changes: ChangeSender) {
        self.sender.send(changes).expect("producer receiver");
    }
    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
        self.script.initialize(client)
    }
    fn call(&self, request: Request, context: &Context) -> Outcome {
        self.script.call(request, context)
    }
}

fn options() -> Options {
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.subscriptions = Some(SubscriptionOptions {
        resource_uris: vec!["acme://report".into()],
        ..SubscriptionOptions::default()
    });
    options.session.resources = Some(Arc::new(
        ResourceCatalog::new(vec![Resource::new("acme://report", "report")], vec![])
            .expect("valid resources"),
    ));
    options
}

fn start(script: Script, options: Options) -> (Host, ChangeSender) {
    let (sender, receiver) = mpsc::channel();
    let host = Host::start_with(
        Arc::new(Producer { sender, script }),
        fixture::catalog(),
        options,
    );
    (
        host,
        receiver.recv_timeout(WAIT).expect("connection producer"),
    )
}

fn request(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"][modern::VERSION_KEY] = json!(modern::PROTOCOL_VERSION);
    params["_meta"][modern::CAPABILITIES_KEY] = json!({});
    rpc::request(id, method, params)
}

fn listen(host: &Host, id: Value, filters: Value) -> Value {
    let expected = id.clone();
    let mut params = json!({});
    params["notifications"] = filters;
    host.send(&request(id, "subscriptions/listen", params));
    let ack = host.recv();
    assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
    assert_eq!(ack["params"]["_meta"][SUBSCRIPTION_KEY], expected);
    ack
}

#[test]
fn subscriptions_driver_delivers_while_busy_and_keeps_progress_on_its_rpc() {
    let events = Events::new();
    let worker_events = events.clone();
    let latch = Latch::default();
    let worker_latch = latch.clone();
    let (host, changes) = start(
        Script::new(move |_, context| {
            worker_events.record("started");
            worker_latch.wait(context);
            assert!(context.progress(1.0, Some(1.0), Some("ready")));
            Outcome::Tool(ToolResult::success(vec![]))
        }),
        options(),
    );
    listen(
        &host,
        json!("stream"),
        json!({"toolsListChanged":true,"resourceSubscriptions":["acme://report"]}),
    );
    let mut call = request(json!(1), "tools/call", json!({"name":"acme_probe"}));
    call["params"]["_meta"]["progressToken"] = json!("tool-progress");
    host.send(&call);
    assert_eq!(events.next(), "started");
    changes
        .send(Change::ResourceUpdated("acme://unknown".into()))
        .unwrap();
    changes
        .send(Change::ResourceUpdated("acme://report".into()))
        .unwrap();
    let update = host.recv();
    assert_eq!(update["method"], "notifications/resources/updated");
    assert_eq!(update["params"]["uri"], "acme://report");
    assert_eq!(update["params"]["_meta"][SUBSCRIPTION_KEY], "stream");
    latch.release();
    let progress = host.recv();
    assert_eq!(progress["method"], "notifications/progress");
    assert_eq!(progress["params"]["progressToken"], "tool-progress");
    assert!(progress["params"]["_meta"].get(SUBSCRIPTION_KEY).is_none());
    assert_eq!(host.recv()["id"], 1);
    let (result, remaining) = host.finish();
    result.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["id"], "stream");
    assert_eq!(remaining[0]["result"]["_meta"][SUBSCRIPTION_KEY], "stream");
    assert!(matches!(
        changes.send(Change::ResourceUpdated("acme://report".into())),
        Err(SendChangeError::Closed(_))
    ));
}

#[test]
fn subscriptions_driver_replaces_catalog_and_cancel_releases_id() {
    let (host, changes) = start(Script::new(|_, _| panic!("no worker dispatch")), options());
    listen(&host, json!(8), json!({"toolsListChanged":true}));
    changes
        .send(Change::Tools(Arc::new(Catalog::new(vec![]).unwrap())))
        .unwrap();
    let notification = host.recv();
    assert_eq!(notification["method"], "notifications/tools/list_changed");
    assert_eq!(notification["params"]["_meta"][SUBSCRIPTION_KEY], 8);
    host.cancel(8);
    host.send(&request(json!(8), "tools/list", json!({})));
    let listed = host.recv();
    assert_eq!(listed["id"], 8);
    assert_eq!(listed["result"]["tools"], json!([]));
    listen(&host, json!("8"), json!({}));
    let (result, remaining) = host.finish();
    result.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["id"], "8");
}

#[test]
fn subscriptions_driver_expires_without_worker_timeout_or_new_input() {
    let mut options = options();
    options.session.subscriptions.as_mut().unwrap().timeout = Duration::from_millis(30);
    let (host, _) = start(Script::new(|_, _| panic!("no workers")), options);
    listen(&host, json!("expiry"), json!({}));
    let ended = host.recv();
    assert_eq!(ended["id"], "expiry");
    assert_eq!(ended["result"]["resultType"], "complete");
    assert_eq!(ended["result"]["_meta"][SUBSCRIPTION_KEY], "expiry");
    let (result, remaining) = host.finish();
    result.unwrap();
    assert_eq!(remaining, Vec::<Value>::new());
}

#[test]
fn subscriptions_driver_refuses_oversized_ack_and_releases_id() {
    let uri = format!("acme://{}", "x".repeat(1500));
    let mut options = options();
    options.max_output_bytes = 1024;
    options
        .session
        .subscriptions
        .as_mut()
        .unwrap()
        .resource_uris = vec![uri.clone()];
    let (host, _) = start(Script::new(|_, _| panic!("no workers")), options);
    host.send(&request(
        json!(1),
        "subscriptions/listen",
        json!({"notifications":{"resourceSubscriptions":[uri]}}),
    ));
    let refused = host.recv();
    assert_eq!(refused["error"]["code"], rpc::INTERNAL_ERROR);
    listen(&host, json!(1), json!({}));
    let (result, remaining) = host.finish();
    result.unwrap();
    assert_eq!(remaining.len(), 1);
}

#[test]
fn subscriptions_independent_runtime_owns_ack_timer_and_graceful_completion() {
    let mut session =
        canonic_mcp_kit::Session::try_new(fixture::catalog(), options().session).unwrap();
    let canonic_mcp_kit::Inbound::Subscription { id, acknowledged } =
        session.receive_value(request(
            json!("direct"),
            "subscriptions/listen",
            json!({"notifications":{"toolsListChanged":true}}),
        ))
    else {
        panic!("listen")
    };
    assert_eq!(acknowledged["params"]["_meta"][SUBSCRIPTION_KEY], id);
    let frames = session
        .publish(Change::Tools(Arc::new(Catalog::new(vec![]).unwrap())))
        .unwrap();
    assert_eq!(frames[0]["method"], "notifications/tools/list_changed");
    assert!(session.subscription_wait().is_some());
    let expired =
        session.expire_subscriptions(std::time::Instant::now() + Duration::from_secs(301));
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0]["id"], id);
    assert_eq!(expired[0]["result"]["_meta"][SUBSCRIPTION_KEY], id);
    assert!(session.end_subscription(&id).is_none());
    assert_eq!(session.outstanding(), 0);
    assert!(session.request_context(&id).is_none());
}

#[test]
fn subscriptions_worker_rejects_disabled_catalog_without_losing_reply() {
    let publisher = Arc::new(Mutex::new(None::<ChangeSender>));
    let worker = publisher.clone();
    let (host, changes) = start(
        Script::new(move |_, _| {
            let sender = worker.lock().unwrap().clone().expect("connection sender");
            let prompts = Arc::new(canonic_mcp_kit::PromptCatalog::new(vec![]).unwrap());
            assert!(matches!(
                sender.send(Change::Prompts(prompts)),
                Err(SendChangeError::Invalid(_))
            ));
            Outcome::Tool(ToolResult::success(vec![]))
        }),
        options(),
    );
    *publisher.lock().unwrap() = Some(changes);
    listen(&host, json!("stream"), json!({"toolsListChanged":true}));
    host.send(&request(
        json!(1),
        "tools/call",
        json!({"name":"acme_echo"}),
    ));
    let reply = host.recv();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["isError"], false);
    host.send(&request(json!(2), "prompts/list", json!({})));
    assert_eq!(host.recv()["error"]["code"], rpc::METHOD_NOT_FOUND);
    let (result, remaining) = host.finish();
    result.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["id"], "stream");
}

#[test]
fn subscriptions_driver_refuses_oversized_final_result_on_expiry_and_eof() {
    for expiry in [true, false] {
        let mut options = options();
        options.max_output_bytes = 1024;
        options.session.modern.as_mut().unwrap().server.title = Some("x".repeat(2048));
        if expiry {
            options.session.subscriptions.as_mut().unwrap().timeout = Duration::from_millis(30);
        }
        let (mut host, _) = start(Script::new(|_, _| panic!("no worker dispatch")), options);
        listen(&host, json!("oversized-final"), json!({}));
        if !expiry {
            host.close();
        }
        let refused = host.recv();
        assert_eq!(refused["id"], "oversized-final");
        assert_eq!(refused["error"]["code"], rpc::INTERNAL_ERROR);
        assert!(refused.get("result").is_none());
        let (result, remaining) = host.finish();
        result.unwrap();
        assert_eq!(remaining, Vec::<Value>::new());
    }
}

#[test]
fn subscriptions_driver_reports_resource_composition_failure() {
    struct Conflict(Change);
    impl Handler for Conflict {
        fn connected(&self, sender: ChangeSender) {
            sender
                .send(self.0.clone())
                .expect("resource change queued before input");
        }
        fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
            panic!("no initialization")
        }
        fn call(&self, _: Request, _: &Context) -> Outcome {
            panic!("no worker dispatch")
        }
    }
    let mut options = options();
    options.session.skills = Some(Arc::new(
        canonic_mcp_kit::SkillCatalog::new(vec![canonic_mcp_kit::SkillDefinition::new(
            "skill://acme/status",
            vec![canonic_mcp_kit::SkillFile::text(
                "SKILL.md",
                "---\nname: status\ndescription: Inspect Acme status.\n---\nRead status.\n",
            )],
        )])
        .unwrap(),
    ));
    let change = Change::Resources(Arc::new(
        ResourceCatalog::new(
            vec![Resource::new("skill://acme/status/SKILL.md", "collision")],
            vec![],
        )
        .unwrap(),
    ));
    let result = canonic_mcp_kit::stdio::serve(
        Arc::new(Conflict(change)),
        fixture::catalog(),
        std::io::Cursor::new(Vec::<u8>::new()),
        Vec::<u8>::new(),
        &options,
    );
    let Err(canonic_mcp_kit::stdio::ServeError::Configuration(error)) = result else {
        panic!("resource composition must fail");
    };
    assert!(error.0.contains("skill://acme/status/SKILL.md"));
}
