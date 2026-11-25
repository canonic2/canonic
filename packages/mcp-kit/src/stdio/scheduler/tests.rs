//! A deterministic backlog exercises the real dispatcher's progress drain.
use super::*;
use crate::{Catalog, Client, Inbound, ServerInfo, SessionOptions};
use serde_json::json;

struct NoCalls;
impl Handler for NoCalls {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        panic!("no protocol input")
    }
    fn call(&self, _: Request, _: &Context) -> Outcome {
        panic!("progress drain only")
    }
}

fn scheduler_with_progress(frames: Vec<(u64, Value)>) -> (Scheduler, Session) {
    let mut scheduler =
        Scheduler::start(&Arc::new(NoCalls), &Options::default(), thread::current()).unwrap();
    // Supply one live assignment without a consumer handler.
    scheduler.lanes[0].idle.clear();
    scheduler.lanes[0].running = 1;
    scheduler.running.insert(
        0,
        Running {
            execution: Trace::default().span(Stage::Worker),
            meta: Meta {
                binding: super::super::execution::Binding::default(),
                apps: false,
                trace: Trace::default(),
                reply_kind: ReplyKind::Ordinary,
                result_schema: None,
                rich_rounds: 0,
                request_context: None,
                grant: None,
                id: None,
                lane: 0,
                key: None,
                timeout: Duration::from_mins(1),
                deadline: Arc::default(),
                read_only: true,
                progress_token: None,
            },
            worker: 0,
            control: Arc::new(Control::new(Duration::from_mins(1))),
            answered: false,
            grace: None,
        },
    );
    let session = Session::new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions::default(),
    );
    // Only this test channel has extra capacity. Preloading more than a turn's
    // budget models a producer refilling the production channel during a drain,
    // without depending on thread scheduling. settle uses the real Receiver and
    // drain path; removing its budget deterministically consumes too many items.
    let (sender, receiver) = mpsc::sync_channel(frames.len());
    scheduler.progress = receiver;
    scheduler.progress_sender = sender;
    for frame in frames {
        scheduler.progress_sender.try_send(frame).unwrap();
    }
    (scheduler, session)
}

fn initialize_forms(session: &mut Session, capabilities: &Value) {
    assert!(matches!(
        session.receive_value(
            json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{
            "protocolVersion":"2025-11-25","clientInfo":{"name":"acme","version":"1"},
            "capabilities":capabilities}})
        ),
        Inbound::Initialize { .. }
    ));
    session.accept_initialize(json!("init"), &ServerInfo::new("acme", "1"));
    session.receive_value(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
}

#[test]
fn progress_drain_stops_at_its_budget_and_preserves_remaining_frames() {
    let expected: Vec<Value> = (0..=2 * PROGRESS_BUFFER).map(Value::from).collect();
    let (mut scheduler, mut session) =
        scheduler_with_progress(expected.iter().cloned().map(|value| (0, value)).collect());
    let mut received = Vec::new();
    for count in [PROGRESS_BUFFER, PROGRESS_BUFFER, 1, 0] {
        let batch = scheduler.settle(&mut session);
        assert_eq!(batch.len(), count);
        for frame in batch {
            let Outgoing::Value(value) = frame else {
                panic!("progress value expected")
            };
            received.push(value);
        }
    }
    assert_eq!(received, expected);
}

#[test]
fn stale_progress_consumes_the_turn_budget() {
    // Unknown assignment 1 is stale; live assignment 0 must wait for turn two.
    let mut frames = vec![(1, Value::Null); PROGRESS_BUFFER];
    frames.push((0, Value::from("live")));
    let (mut scheduler, mut session) = scheduler_with_progress(frames);
    assert!(scheduler.settle(&mut session).is_empty());
    let mut next = scheduler.settle(&mut session);
    assert_eq!(next.len(), 1);
    assert!(matches!(next.pop(), Some(Outgoing::Value(value)) if value == "live"));
    assert!(scheduler.settle(&mut session).is_empty());
}

#[test]
fn human_input_lifetime_has_no_wakeup_or_expiry_and_keeps_remaining_budget() {
    use crate::elicitation::{Action, Answer, Form, Request as Question};

    for rich in [false, true] {
        let (mut scheduler, mut session) = scheduler_with_progress(vec![]);
        initialize_forms(&mut session, &json!({"elicitation":{"form":{}}}));
        let run = scheduler.running.get_mut(&0).unwrap();
        // The original budget was one minute, with only three seconds left.
        run.control = Arc::new(Control::new(Duration::from_secs(3)));
        run.meta.key = Some("acme".into());
        scheduler.lanes[0].keys.insert("acme".into());
        let question = Question::form(
            "Choose?",
            Form::new(json!({"type":"object","properties":{}})).unwrap(),
        )
        .unwrap();
        let selected = question
            .answer(&json!({"action":"accept","content":{}}))
            .unwrap();
        let step = if rich {
            Finish::Elicit(Elicit::new(question, |_, _| panic!("queue only")))
        } else {
            Finish::Confirm(Confirm::new("Continue?", |_, _| panic!("queue only")))
        };
        let mut out = Vec::new();
        scheduler.complete(&mut session, 0, step, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(scheduler.lanes[0].idle.len(), 1);
        assert_eq!(scheduler.lanes[0].used(), 1);
        assert!(scheduler.next_wake().is_none());

        // Advance the real expiry sweep far past both former five-minute limits.
        // No sleeping or test-only replacement scheduler is involved.
        out.clear();
        scheduler.expire(
            &mut session,
            &mut out,
            Instant::now() + Duration::from_hours(12),
        );
        assert!(out.is_empty());
        let id = scheduler.waiting.as_ref().unwrap().elicitation.clone();
        if rich {
            scheduler.elicited(&id, Answer::Response(selected), &Trace::default());
        } else {
            scheduler.confirmed(&id, Decision::Accepted, &Trace::default());
        }
        assert!(scheduler.waiting.is_none());
        let job = &scheduler.lanes[0].queue[0];
        if rich {
            assert!(
                matches!(&job.work, Work::ElicitResume(_, Answer::Response(reply)) if reply.action == Action::Accept)
            );
        } else {
            assert!(matches!(job.work, Work::Resume(_, Decision::Accepted)));
        }
        assert!(job.control.remaining() <= Duration::from_secs(3));
        assert!(!job.control.expired());
        assert!(job.holds_key);
        assert!(scheduler.lanes[0].keys.contains("acme"));
        // Active queue time counts again; expiration releases capacity and the key.
        scheduler.expire(
            &mut session,
            &mut out,
            Instant::now() + Duration::from_secs(4),
        );
        assert_eq!(scheduler.lanes[0].used(), 0);
        assert!(scheduler.lanes[0].keys.is_empty());
        assert!(scheduler.idle());
    }
}

#[test]
fn human_input_lifetime_queued_answers_release_ownership_on_cancel_or_disconnect() {
    for disconnect in [false, true] {
        let (mut scheduler, mut session) = scheduler_with_progress(vec![]);
        initialize_forms(&mut session, &json!({"elicitation":{}}));
        let run = scheduler.running.get_mut(&0).unwrap();
        run.meta.id = Some(json!(7));
        run.meta.key = Some("acme".into());
        scheduler.lanes[0].keys.insert("acme".into());
        let owned = Arc::new(());
        let weak = Arc::downgrade(&owned);
        let mut out = Vec::new();
        scheduler.complete(
            &mut session,
            0,
            Finish::Confirm(Confirm::new("Continue?", move |_, _| {
                drop(owned);
                panic!("teardown must discard the queued answer")
            })),
            &mut out,
        );
        let question = scheduler.waiting.as_ref().unwrap().elicitation.clone();
        scheduler.confirmed(&question, Decision::Accepted, &Trace::default());
        assert!(weak.upgrade().is_some());
        if disconnect {
            scheduler.disconnect(&mut session);
        } else {
            scheduler.cancel(&mut session, &json!(7), &Trace::default());
        }
        assert!(weak.upgrade().is_none());
        assert!(scheduler.idle());
        assert!(scheduler.lanes[0].keys.is_empty());
        scheduler.confirmed(&question, Decision::Accepted, &Trace::default());
        assert!(scheduler.idle());
    }
}

#[test]
fn a_parked_form_releases_the_key_and_its_answer_waits_to_reacquire_it() {
    for cancel in [false, true] {
        let (mut scheduler, mut session) = scheduler_with_progress(vec![]);
        scheduler.release_key_while_waiting = true;
        initialize_forms(&mut session, &json!({"elicitation":{}}));
        let run = scheduler.running.get_mut(&0).unwrap();
        run.meta.id = Some(json!(7));
        run.meta.key = Some("acme".into());
        scheduler.lanes[0].keys.insert("acme".into());
        let mut out = Vec::new();
        scheduler.complete(
            &mut session,
            0,
            Finish::Confirm(Confirm::new("Continue?", |_, _| panic!("queue only"))),
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(scheduler.lanes[0].idle.len(), 1);
        assert!(scheduler.lanes[0].keys.is_empty());
        // Another call now owns the same key.
        scheduler.lanes[0].keys.insert("acme".into());
        if cancel {
            scheduler.cancel(&mut session, &json!(7), &Trace::default());
            assert!(scheduler.lanes[0].keys.contains("acme"));
        } else {
            let question = scheduler.waiting.as_ref().unwrap().elicitation.clone();
            scheduler.confirmed(&question, Decision::Accepted, &Trace::default());
            assert!(scheduler.lanes[0].next_ready().is_none());
            scheduler.lanes[0].keys.remove("acme");
            assert!(matches!(
                scheduler.lanes[0].next_ready().unwrap().work,
                Work::Resume(_, Decision::Accepted)
            ));
        }
        assert!(scheduler.waiting.is_none());
    }
}

#[test]
fn modern_input_keeps_its_cleanup_until_publication() {
    use crate::modern::{CAPABILITIES_KEY, PROTOCOL_VERSION, VERSION_KEY};
    use std::sync::atomic::{AtomicBool, Ordering};

    for (cancel, disconnect, limit) in [
        (false, false, 8192),
        (true, false, 8192),
        (false, true, 8192),
        (false, false, 1),
    ] {
        let (mut scheduler, _) = scheduler_with_progress(vec![]);
        let mut session = Session::new(
            Arc::new(
                Catalog::new(vec![crate::Tool::new(
                    "acme",
                    "Review",
                    json!({"type":"object"}),
                )])
                .unwrap(),
            ),
            SessionOptions {
                modern: Some(crate::ModernOptions::new(ServerInfo::new("acme", "1"))),
                ..SessionOptions::default()
            },
        );
        let id = json!(7);
        assert!(matches!(
            session.receive_value(json!({"jsonrpc":"2.0","method":"tools/call","id":id,"params":{
                "name":"acme","_meta":{VERSION_KEY:PROTOCOL_VERSION,CAPABILITIES_KEY:{"elicitation":{}}}
            }})),
            Inbound::Tool { .. }
        ));
        let context = session.request_context(&id).unwrap();
        scheduler.running.get_mut(&0).unwrap().meta.id = Some(id.clone());
        let released = Arc::new(AtomicBool::new(false));
        let observe = Arc::clone(&released);
        let interim = super::super::PendingInput::new(
            context.confirmation("Continue?", "state").unwrap(),
            move || observe.store(true, Ordering::Release),
        );
        let finish = settle(
            Some(&id),
            Ok(Outcome::PendingInput(interim)),
            false,
            limit,
            Some(&context),
            false,
            &Trace::default(),
        );
        if cancel {
            scheduler.cancel(&mut session, &id, &Trace::default());
        }
        if disconnect {
            scheduler.disconnect(&mut session);
        }
        let mut out = Vec::new();
        scheduler.complete(&mut session, 0, finish, &mut out);
        assert_eq!(
            released.load(Ordering::Acquire),
            cancel || disconnect || limit == 1
        );
        if !cancel && !disconnect && limit != 1 {
            assert!(matches!(out.as_slice(), [Outgoing::Interim(..)]));
            drop(out);
            assert!(released.load(Ordering::Acquire));
        }
    }
}
