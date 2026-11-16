use super::*;
use crate::{
    ModernOptions, ServerInfo, Tool, ToolResult, modern,
    worker::wire::{self, Assignment},
};
use std::{sync::Mutex, time::Instant};

struct Fixture {
    decoder: Decoder,
    catalog: Catalog,
    options: SessionOptions,
    request: Request,
    source: Context,
    control: Arc<Control>,
}

type Mutation = Box<dyn Fn(&mut Call)>;

fn assignment() -> Assignment {
    Assignment {
        frontend: "acme-frontend".into(),
        worker: "acme-worker".into(),
        sequence: 1,
    }
}

fn fixture(modern: bool, progress: bool) -> Fixture {
    let catalog = Catalog::new(vec![
        Tool::new(
            "acme_probe",
            "Read Acme status",
            json!({
                "type":"object", "properties":{}, "additionalProperties":false,
            }),
        )
        .with_output_schema(json!({
            "type":"object", "properties":{"status":{"type":"string"}},
            "required":["status"], "additionalProperties":false,
        })),
    ])
    .unwrap();
    let options = SessionOptions {
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0"))),
        tasks: Some(crate::task::TaskOptions {
            max_records: 8,
            max_bytes: 65536,
            max_running: 2,
            max_input_rounds: 3,
            execution_timeout: Duration::from_secs(30),
        }),
        ..SessionOptions::default()
    };
    let mut params = json!({"name":"acme_probe","arguments":{}});
    if progress {
        params["_meta"] = json!({"progressToken":"same-host-token"});
    }
    let request = Request::Tool(crate::tool::ToolCall::from_params(&params).unwrap());
    let schema = catalog.result_schema("acme_probe");
    let protocol = modern.then(|| {
        params["_meta"][modern::VERSION_KEY] = json!(modern::PROTOCOL_VERSION);
        params["_meta"][modern::CAPABILITIES_KEY] = json!({"elicitation":{"form":{}},
            "extensions":{crate::task::EXTENSION:{}}});
        params["_meta"][modern::CLIENT_KEY] = json!({"name":"acme-client","version":"1"});
        params["_meta"]["acme.example/trace"] = json!({"opaque":[1, "two"]});
        params["requestState"] = json!("acme-state");
        params["inputResponses"] = json!({"confirmation":{"action":"accept"}});
        RequestContext::parse(
            &params,
            options.modern.as_ref().unwrap(),
            "tools/call",
            options.tasks.as_ref(),
            schema.clone(),
        )
        .unwrap()
    });
    let control = Arc::new(Control::until(Instant::now() + Duration::from_secs(20)));
    let grant = crate::http::Grant::new(vec!["https://acme.example/mcp".into()])
        .with_scopes(["read", "write"])
        .with_subject("acme-user")
        .with_client_id("acme-client")
        .with_expiry(UNIX_EPOCH + Duration::new(2_000_000_000, 123));
    let source = Context::from_worker(
        control.clone(),
        protocol,
        Some(Arc::new(grant)),
        Some(json!("reused-id")),
        schema,
        false,
        None,
    );
    Fixture {
        decoder: Decoder::new(Arc::new(catalog.clone()), options.clone()).unwrap(),
        catalog,
        options,
        request,
        source,
        control,
    }
}

impl Fixture {
    fn call(&self) -> Call {
        let call = Call::new(assignment(), &self.request, &self.source).unwrap();
        wire::decode(&wire::encode(&call, 1024 * 1024).unwrap(), 1024 * 1024).unwrap()
    }
    fn restore(
        &self,
        call: &Call,
        progress: Option<ProgressSink>,
    ) -> Result<(Request, Context), RpcError> {
        call.restore(&assignment(), &self.decoder, self.control.clone(), progress)
    }
}

#[test]
fn worker_context_roundtrip_preserves_request_authority_schema_and_original_control() {
    for modern in [false, true] {
        let fixture = fixture(modern, true);
        let (request, context) = fixture.restore(&fixture.call(), None).unwrap();
        assert_eq!(request, fixture.request);
        assert_eq!(context.request_context(), fixture.source.request_context());
        assert_eq!(context.request_id(), fixture.source.request_id());
        assert_eq!(context.control().deadline(), fixture.control.deadline());
        let grant = context.authorization().unwrap();
        let source = fixture.source.authorization().unwrap();
        assert_eq!(grant.audiences(), source.audiences());
        assert_eq!(
            grant.scopes().collect::<Vec<_>>(),
            source.scopes().collect::<Vec<_>>()
        );
        assert_eq!(grant.subject(), source.subject());
        assert_eq!(grant.client_id(), source.client_id());
        assert_eq!(grant.expires_at(), source.expires_at());
        let schema = context.result_schema().unwrap();
        assert!(
            schema
                .validate(&ToolResult::structured(json!({"status":"ready"}), false))
                .is_ok()
        );
        assert!(
            schema
                .validate(&ToolResult::structured(json!({"status":7}), false))
                .is_err()
        );
        assert!(
            context.worker_hold().is_err(),
            "remote resources need an explicit owner protocol"
        );
        fixture.control.cancel();
        assert!(context.check().is_err());
        assert!(Call::new(assignment(), &fixture.request, &context).is_err());
    }
}

#[test]
fn worker_context_progress_respects_token_backpressure_cancellation_and_settlement() {
    let fixture = fixture(true, true);
    let updates = Arc::new(Mutex::new(Vec::new()));
    let sent = updates.clone();
    let sink: ProgressSink = Arc::new(move |count, total, message| {
        let mut updates = sent.lock().unwrap();
        if updates.len() == 1 {
            return false;
        }
        updates.push((count, total, message.map(str::to_owned)));
        true
    });
    let (_, context) = fixture
        .restore(&fixture.call(), Some(sink.clone()))
        .unwrap();
    let reporter = context.progress_reporter().unwrap();
    assert!(reporter.progress(1.0, Some(3.0), Some("working")));
    assert!(!context.progress(2.0, None, None));
    updates.lock().unwrap().clear();
    drop(context);
    assert!(
        !reporter.progress(2.0, None, None),
        "a retained reporter cannot publish after settlement"
    );
    assert!(updates.lock().unwrap().is_empty());
    let (_, context) = fixture
        .restore(&fixture.call(), Some(sink.clone()))
        .unwrap();
    fixture.control.cancel();
    assert!(!context.progress(2.0, None, None));
    let no_token = self::fixture(false, false);
    let (_, context) = no_token.restore(&no_token.call(), Some(sink)).unwrap();
    assert!(context.progress_reporter().is_none());
    assert!(!context.progress(1.0, None, None));
}

#[test]
fn worker_context_refuses_foreign_work_bad_metadata_and_renewed_deadlines() {
    let fixture = fixture(true, false);
    let original = fixture.call();
    let mutations: Vec<Mutation> = vec![
        Box::new(|call| call.version = 1),
        Box::new(|call| call.assignment.sequence += 1),
        Box::new(|call| call.assignment.worker = "foreign".into()),
        Box::new(|call| call.metadata["id"] = json!({"invalid":true})),
        Box::new(|call| call.metadata["unexpected"] = json!(true)),
        Box::new(|call| call.metadata["protocol"]["version"] = json!("unknown")),
        Box::new(|call| call.metadata["protocol"]["capabilities"] = json!({})),
        Box::new(|call| call.metadata["protocol"]["clientInfo"] = json!({"name":"foreign"})),
        Box::new(|call| call.metadata["apps"] = json!(true)),
        Box::new(|call| call.metadata["grant"]["audiences"] = json!([])),
        Box::new(|call| call.metadata["grant"]["expiresAt"]["nanoseconds"] = json!(1_000_000_000)),
        Box::new(|call| call.work.params["arguments"] = json!({"undeclared":true})),
    ];
    for mutate in mutations {
        let mut call = original.clone();
        mutate(&mut call);
        assert!(fixture.restore(&call, None).is_err(), "{call:?}");
    }
    assert!(
        original
            .restore(
                &assignment(),
                &fixture.decoder,
                Arc::new(Control::new(Duration::from_mins(1))),
                None
            )
            .is_err()
    );
    let mut no_tasks = fixture.options.clone();
    no_tasks.tasks = None;
    let decoder = Decoder::new(Arc::new(fixture.catalog.clone()), no_tasks).unwrap();
    assert!(
        original
            .restore(&assignment(), &decoder, fixture.control.clone(), None)
            .is_err()
    );
    let cancelled = Arc::new(Control::until(Instant::now()));
    let (_, context) = original
        .restore(&assignment(), &fixture.decoder, cancelled, None)
        .unwrap();
    assert!(
        context.check().is_err(),
        "restore cannot renew an exhausted control"
    );
}

#[test]
fn worker_context_never_discards_opaque_authority_extensions() {
    let fixture = fixture(false, false);
    let source = Context::from_worker(
        fixture.control.clone(),
        None,
        Some(Arc::new(
            crate::http::Grant::new(vec!["https://acme.example/mcp".into()])
                .with_extension("private authority"),
        )),
        None,
        None,
        false,
        None,
    );
    assert!(Call::new(assignment(), &fixture.request, &source).is_err());
    let source = Context::from_worker(
        fixture.control.clone(),
        None,
        Some(Arc::new(
            crate::http::Grant::new(vec!["https://acme.example/mcp".into()])
                .with_expiry(UNIX_EPOCH - Duration::from_secs(1)),
        )),
        None,
        None,
        false,
        None,
    );
    assert!(
        Call::new(assignment(), &fixture.request, &source).is_err(),
        "an unrepresentable expiry cannot become an unexpiring grant"
    );
}

#[test]
fn worker_context_preserves_resource_cache_overrides_and_rejects_invalid_declarations() {
    let fixture = fixture(true, false);
    let mut options = fixture.options.clone();
    options.resources = Some(Arc::new(
        crate::ResourceCatalog::new(vec![], vec![]).unwrap(),
    ));
    let decoder = Decoder::new(Arc::new(fixture.catalog.clone()), options.clone()).unwrap();
    let metadata = fixture.source.request_context().unwrap().metadata();
    let params = json!({"uri":"acme://status", "_meta":metadata.as_map()});
    let cache = crate::CachePolicy::new(500, crate::CacheScope::Public).unwrap();
    let protocol = RequestContext::parse_prepared(
        &params,
        options.modern.as_ref().unwrap(),
        "resources/read",
        None,
        None,
        Arc::new(options.modern.as_ref().unwrap().server.identity()),
        Some(cache),
    )
    .unwrap();
    let source = Context::from_worker(
        fixture.control.clone(),
        Some(protocol),
        None,
        Some(json!(1)),
        None,
        false,
        None,
    );
    let request = Request::Method {
        method: "resources/read".into(),
        params,
    };
    let call = Call::new(assignment(), &request, &source).unwrap();
    let (restored, context) = call
        .restore(&assignment(), &decoder, fixture.control.clone(), None)
        .unwrap();
    assert_eq!(restored, request);
    assert_eq!(context.request_context(), source.request_context());
    for cache in [
        json!({"ttlMs":500,"cacheScope":"invalid"}),
        json!({"ttlMs":u64::MAX,"cacheScope":"public"}),
    ] {
        let mut invalid = call.clone();
        invalid.metadata["protocol"]["cache"] = cache;
        assert!(
            invalid
                .restore(&assignment(), &decoder, fixture.control.clone(), None)
                .is_err()
        );
    }
    let invalid = SessionOptions {
        legacy_protocols: false,
        modern: None,
        ..SessionOptions::default()
    };
    assert!(Decoder::new(Arc::new(fixture.catalog.clone()), invalid).is_err());
}

#[test]
fn worker_terminal_codec_roundtrips_results_and_keeps_unsupported_continuations_owned() {
    use crate::{
        Content, ReadResourceResult, ResourceContents,
        stdio::{Confirm, Outcome},
    };
    struct Captured(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Captured {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    let fixture = fixture(true, false);
    let (_, context) = fixture.restore(&fixture.call(), None).unwrap();
    let outcomes = [
        Outcome::Method(Err(RpcError::new(-32603, "failure without data"))),
        Outcome::Tool(ToolResult::structured(json!({"status":"ready"}), false)),
        Outcome::Method(Ok(json!({"ready":true}))),
        Outcome::Method(Err(
            RpcError::new(-32603, "fixture failure").with_data(json!({"uncertain":true}))
        )),
        Outcome::Done,
        Outcome::ResourceNotFound("acme://missing".into()),
        Outcome::ResourceWithCache(
            ReadResourceResult::new(vec![
                ResourceContents::text("acme://status", Some("text/plain".into()), "ready")
                    .unwrap(),
            ])
            .unwrap(),
            crate::CachePolicy::new(200, crate::CacheScope::Private).unwrap(),
        ),
    ];
    for original in &outcomes {
        let reply = wire::Reply::terminal(assignment(), original, 65536).unwrap();
        let bytes = wire::encode(&reply, 65536).unwrap();
        let received: wire::Reply = wire::decode(&bytes, 65536).unwrap();
        let restored = received.outcome(&assignment(), &context).unwrap();
        let second = wire::Reply::terminal(assignment(), &restored, 65536).unwrap();
        assert_eq!(second.kind, reply.kind);
        assert_eq!(second.value, reply.value);
    }
    let oversized = Outcome::Tool(ToolResult::success(vec![Content::text("x".repeat(1024))]));
    assert!(wire::Reply::terminal(assignment(), &oversized, 128).is_err());
    assert!(wire::Reply::terminal(assignment(), &Outcome::Done, 1).is_err());
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let capture = Captured(dropped.clone());
    let pending = Outcome::Confirm(Confirm::new("Continue?", move |_, _| {
        let _keep = &capture;
        Outcome::Done
    }));
    assert!(wire::Reply::terminal(assignment(), &pending, 65536).is_err());
    assert!(!dropped.load(std::sync::atomic::Ordering::Acquire));
    drop(pending);
    assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
}
