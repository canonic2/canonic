use super::*;

fn registration() -> Registration {
    serde_json::from_value(json!({
        "binary":"/acme/shield","binary_digest":"a".repeat(64),"version":"0.20.0",
        "entry":{},"agents":{"codex":{"groups":{}}}
    }))
    .unwrap()
}

#[test]
fn scoped_identity_ignores_other_agents_and_advisory_metadata() {
    let before = registration();
    let mut after = before.clone();
    after.binary_digest = "replacement".into();
    after.version = "replacement".into();
    after.entry = json!({"command":"another-shared-template"});
    after.agents.insert(
        "claude".into(),
        serde_json::from_value(json!({"groups":{}})).unwrap(),
    );
    let mut tools = crate::review::mcp::default_approved();
    tools.reverse();
    after.approved_tools = Some(tools);
    assert_eq!(scoped(&before, "codex"), scoped(&after, "codex"));
    after.ask_fallback.insert(
        "codex".into(),
        crate::review::integrations::AskFallback::Allow,
    );
    assert_ne!(scoped(&before, "codex"), scoped(&after, "codex"));
    let mut after = before.clone();
    after.binary = std::path::PathBuf::from("/acme/another-shield");
    assert_ne!(scoped(&before, "codex"), scoped(&after, "codex"));
}

#[test]
fn legacy_context_compatibility_is_exact_and_pruned_with_its_reference() {
    let context = Conversation {
        reference: "session-acme".into(),
        agent: "codex".into(),
        session_id: "acme".into(),
        registration: "a".repeat(64),
        created: 1,
    };
    let key = hash(&json!([context.agent, context.session_id]));
    let scoped = format!("agent-v1:{}", "b".repeat(64));
    let current = BTreeMap::from([("codex".into(), scoped.clone())]);
    let contexts = BTreeMap::from([(key.clone(), context.clone())]);
    let mut compatibility = Compatibility::default();
    compatibility.contexts.insert(
        context.reference.clone(),
        ContextBinding {
            key: key.clone(),
            agent: context.agent.clone(),
            old_registration: context.registration.clone(),
            scoped,
        },
    );
    assert!(compatibility.validate(&contexts, &BTreeMap::new()).is_ok());
    assert!(compatibility.context_matches(&key, &context, &current));
    assert!(!compatibility.context_matches("another-key", &context, &current));
    let mut changed = context.clone();
    changed.registration = "c".repeat(64);
    assert!(!compatibility.context_matches(&key, &changed, &current));
    assert!(
        compatibility
            .validate(&BTreeMap::new(), &BTreeMap::new())
            .is_err()
    );
    compatibility.prune(&BTreeMap::new(), &BTreeMap::new());
    assert!(compatibility.is_empty());
}
