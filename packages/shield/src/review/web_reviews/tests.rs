use super::*;
use crate::{configuration::Bundle, review};
use serde_json::json;

fn fixture(agent: &str) -> (assert_fs::TempDir, Context, review::mcp::Connection) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nhistory: {enabled: false}\n",
        &[],
        None,
    )
    .unwrap();
    let mut context = review::mcp::initialize_fixture(root.path(), &bundle);
    review::operator::integrations::configure_fixture(
        root.path(),
        review::integrations::IntegrationChoice::agent(agent),
        review::integrations::AskFallback::Block,
    );
    context.surface = "mcp".into();
    context.agent = Some(agent.into());
    (
        root,
        context,
        review::mcp::Connection::new("web-save-review".into()),
    )
}

fn change(url: &str) -> Value {
    crate::proposal_draft::web_fetch(url).unwrap()
}

fn review_id(root: &Path, agent: &str, session: &str, url: &str) -> String {
    let Offer::Pending { id, .. } = offer(root, agent, session, &change(url)).unwrap() else {
        panic!("an unpresented site must have a review");
    };
    id
}

fn propose(
    context: &Context,
    connection: &review::mcp::Connection,
    id: &str,
    url: &str,
    reason: &str,
) -> Value {
    review::mcp::call(
        context,
        connection,
        "policy_propose",
        &json!({"change":change(url),"review_ids":[id],"agent_reason":reason}),
        &|| Ok(()),
    )
}

#[test]
fn web_reviews_decline_is_durable_per_domain_session_without_history() {
    let (root, context, connection) = fixture("claude");
    let url = "http://grafana.workspace.localhost:51075/api?q=one";
    let id = review_id(root.path(), "claude", "session-one", url);
    assert_eq!(
        id,
        review_id(
            root.path(),
            "claude",
            "session-one",
            "http://auth.other.localhost:3000/csrf?fresh=yes"
        )
    );
    let proposed = propose(&context, &connection, &id, url, "Save the local site.");
    assert!(proposed["error"].is_null(), "{proposed}");
    let args = &proposed["data"]["apply_arguments"];
    let duplicate = propose(
        &context,
        &connection,
        &id,
        url,
        "Changed wording does not create another offer.",
    );
    assert!(duplicate["error"].is_object(), "{duplicate}");
    assert!(
        review::mcp::apply_confirmation(&context, &connection, args)
            .unwrap()
            .is_some()
    );
    let duplicate = review::mcp::apply_confirmation(&context, &connection, args).unwrap_err();
    assert!(matches!(duplicate, Error::WebReviewOffered));
    assert!(review::mcp::settle_apply_failure(&context, args, &duplicate).is_none());
    let declined = review::mcp::policy_outcome(&context, args, "approval_declined");
    assert_eq!(declined["error"]["code"], "approval_declined", "{declined}");
    let reconnected = review::mcp::Connection::new("reconnected-web-review".into());
    for index in 0..10 {
        let url = format!(
            "http://service-{index}.another-workspace.localhost:{}/endpoint?query={index}",
            3000 + index
        );
        assert!(matches!(
            offer(root.path(), "claude", "session-one", &change(&url)).unwrap(),
            Offer::Suppressed
        ));
        let again = propose(&context, &reconnected, &id, &url, "Try another endpoint.");
        assert!(again["error"].is_object(), "{again}");
    }
    // Neither optional history nor retained proposal recovery is the owner.
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            assert!(!state.settings.history_enabled);
            assert!(state.events.is_empty());
            state.proposals.clear();
            Ok(json!({}))
        })
        .unwrap();
    assert!(matches!(
        offer(root.path(), "claude", "session-one", &change(url)).unwrap(),
        Offer::Suppressed
    ));
    assert_ne!(id, review_id(root.path(), "claude", "session-two", url));
    assert_ne!(id, review_id(root.path(), "codex", "session-one", url));
    assert_ne!(
        id,
        review_id(root.path(), "claude", "session-one", "https://example.com/")
    );
}

#[test]
fn web_reviews_claim_is_atomic_and_suppression_is_not_permission() {
    let (root, context, connection) = fixture("claude");
    let url = "https://api.example.com:8443/v1?q=one";
    let id = review_id(root.path(), "claude", "session-one", url);
    let proposed = propose(&context, &connection, &id, url, "Save the example site.");
    assert!(proposed["error"].is_null(), "{proposed}");
    let policy_before = std::fs::read(root.path().join(".shield/config.yml")).unwrap();
    let proposal = proposed["data"]["proposal_id"].as_str().unwrap().to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                let context = &context;
                let proposal = &proposal;
                scope.spawn(move || {
                    barrier.wait();
                    claim(context, proposal).is_ok()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|ok| **ok).count(), 1);
    // The claim wrote no policy rule, even if the host never delivered an answer.
    assert_eq!(
        std::fs::read(root.path().join(".shield/config.yml")).unwrap(),
        policy_before
    );
    assert!(matches!(
        offer(
            root.path(),
            "claude",
            "session-one",
            &change("https://docs.example.com/other")
        )
        .unwrap(),
        Offer::Suppressed
    ));
    assert!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals
            .iter()
            .all(|p| p.status != "applied")
    );
}

#[test]
fn web_reviews_tool_permission_is_claimed_before_native_host_prompt() {
    let (root, context, connection) = fixture("codex");
    let url = "https://api.example.com/";
    let id = review_id(root.path(), "codex", "session-one", url);
    let proposed = propose(&context, &connection, &id, url, "Save the example site.");
    assert!(proposed["error"].is_null(), "{proposed}");
    let args = &proposed["data"]["apply_arguments"];
    review::mcp::claim_native_web_review(&context, "codex", args, Some("session-one")).unwrap();
    assert!(
        review::mcp::claim_native_web_review(&context, "codex", args, Some("session-one")).is_err()
    );
    assert!(matches!(
        offer(root.path(), "codex", "session-one", &change(url)).unwrap(),
        Offer::Suppressed
    ));
}

#[test]
fn web_reviews_native_claim_leaves_other_proposals_to_the_server() {
    let (root, context, connection) = fixture("codex");
    review_id(
        root.path(),
        "codex",
        "session-one",
        "https://api.example.com/",
    );
    let rule = json!({"id":"acme-read","decision":"allow","reason":"Acme routine",
        "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}});
    let proposed = review::mcp::call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"rule_add","rule":rule}}),
        &|| Ok(()),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    let mut args = proposed["data"]["apply_arguments"].clone();
    // A proposal without web allowances has no save offer to claim.
    assert!(
        !review::mcp::claim_native_web_review(&context, "codex", &args, Some("session-one"))
            .unwrap()
    );
    // Invalid arguments are the server's error to report, which the hook passes
    // through; only an already offered review is a save-offer refusal.
    args["unexpected"] = json!(true);
    assert!(!matches!(
        review::mcp::claim_native_web_review(&context, "codex", &args, Some("session-one")),
        Err(Error::WebReviewOffered)
    ));
}

#[test]
fn web_reviews_capacity_does_not_evict_suppression_or_grant_permission() {
    let (root, context, _) = fixture("claude");
    let url = "https://api.example.com/";
    let id = review_id(root.path(), "claude", "session-one", url);
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.web_reviews[0].prompted = true;
            for index in 1..MAX_OFFERS {
                state.web_reviews.push(Entry {
                    id: format!("{index:064x}"),
                    agent: "claude".into(),
                    proposal: None,
                    prompted: true,
                    created_at_ms: crate::review::clock::best_effort_ms(),
                });
            }
            Ok(serde_json::json!({}))
        })
        .unwrap();
    assert!(matches!(
        offer(root.path(), "claude", "session-one", &change(url)).unwrap(),
        Offer::Suppressed
    ));
    assert!(matches!(
        offer(
            root.path(),
            "claude",
            "new-session",
            &change("https://another.example.net/")
        )
        .unwrap(),
        Offer::Suppressed
    ));
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.web_reviews.len(), MAX_OFFERS);
    assert!(
        state
            .web_reviews
            .iter()
            .any(|entry| entry.id == id && entry.prompted)
    );
    assert!(state.proposals.is_empty());
    // Records past their lifetime free their slots for later sessions.
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            for entry in &mut state.web_reviews {
                entry.created_at_ms = 0;
            }
            Ok(serde_json::json!({}))
        })
        .unwrap();
    assert!(matches!(
        offer(
            root.path(),
            "claude",
            "new-session",
            &change("https://another.example.net/")
        )
        .unwrap(),
        Offer::Pending { .. }
    ));
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.web_reviews.len(), 1);
}
