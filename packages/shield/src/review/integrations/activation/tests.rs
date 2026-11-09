use super::*;
use crate::{
    configuration::Bundle,
    mcp_connections::launcher::Binding,
    review::{self, integrations, model::State},
};
use std::collections::BTreeMap;

fn bundle(local: bool, operation: &str) -> Bundle {
    let mut cloud = json!({"transport":{"type":"stdio","command":"acme-connector","args":[operation]},"agents":{"claude":"acme_cloud","codex":"acme_cloud"}});
    if operation == "disabled" {
        cloud["enabled"] = json!(false);
    }
    if operation == "alias" {
        cloud["agents"] = json!({"claude":"acme_next","codex":"acme_next"});
    }
    let mut connections = json!({"neighbor":{"transport":{"type":"stdio","command":"acme-neighbor"},"agents":{"claude":"acme_neighbor","codex":"acme_neighbor"}}});
    if operation != "removed" {
        connections["cloud"] = cloud;
    }
    let source = json!({"schema_version":1,"packs":[],"mcp":connections}).to_string();
    if local {
        Bundle::capture_layers(
            "schema_version: 1\npacks: []\n",
            Some(&source),
            &[],
            None,
            None,
            &[],
        )
        .unwrap()
    } else {
        Bundle::capture(&source, &[], None).unwrap()
    }
}

fn state(context: &Context) -> State {
    review::project::load(context).unwrap()
}

fn connections(state: &State) -> Connections {
    serde_json::from_value(state.candidates["project"]["lock"]["mcp"].clone()).unwrap()
}

fn fixture(local: bool) -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle(local, "before"));
    std::fs::write(
        root.path().join(".mcp.json"),
        json!({"mcpServers":{"manual":{"type":"http","url":"https://manual.example.com/mcp"}}})
            .to_string(),
    )
    .unwrap();
    let before = state(&context);
    let plan = setup::prepare(
        root.path(),
        &before.operator,
        integrations::IntegrationChoice::Agents(
            integrations::IntegrationChoice::bits(&["claude", "codex"]).unwrap(),
        ),
    )
    .unwrap()
    .unwrap();
    integrations::apply(&context, &before.operator, &plan).unwrap();
    (root, context)
}

fn registered(context: &Context) -> super::super::Registration {
    registration(&state(context).operator).unwrap().unwrap()
}

fn launch(registration: &super::super::Registration, agent: &str, id: &str) -> Binding {
    serde_json::from_value(registration.connections[agent]["launches"][id].clone()).unwrap()
}

fn commit(context: &Context, local: bool, operation: &str) -> Value {
    let before = state(context);
    review::publication::commit(
        context,
        &before,
        json!(bundle(local, operation)),
        before.operator.clone(),
        None,
    )
    .unwrap()
}

#[test]
fn connection_publication_updates_registered_hosts_and_preserves_neighbors_automatically() {
    for local in [false, true] {
        let (root, context) = fixture(local);
        let previous = registered(&context);
        let original_connections = connections(&state(&context));
        let hooks = std::fs::read(root.path().join(".claude/settings.local.json")).unwrap();
        let receipt = commit(&context, local, "after");
        assert_eq!(receipt["connection_publication"], "published", "{receipt}");
        let current = state(&context);
        assert!(current.operator["integration_pending"].is_null());
        assert!(current.operator["integration_publication"].is_null());
        let configured = connections(&current);
        let next = registration(&current.operator).unwrap().unwrap();
        assert_eq!(json!(previous.agents), json!(next.agents));
        for agent in ["claude", "codex"] {
            assert_eq!(
                previous.connections[agent]["entries"]["acme_neighbor"],
                next.connections[agent]["entries"]["acme_neighbor"]
            );
            assert_eq!(
                previous.connections[agent]["launches"]["neighbor"],
                next.connections[agent]["launches"]["neighbor"]
            );
            let projected =
                integrations::mcp_projection(root.path(), agent, "cloud", &configured["cloud"])
                    .unwrap();
            integrations::verify_mcp_connection(root.path(), agent, "acme_cloud", &projected)
                .unwrap();
            if local {
                let old = launch(&previous, agent, "cloud");
                let (id, binding) =
                    integrations::resolve_mcp_token(root.path(), &old.token).unwrap();
                assert_eq!(id, "cloud");
                assert_eq!(old.token, launch(&next, agent, "cloud").token);
                assert!(binding.matches(&configured["cloud"]));
                assert_ne!(
                    previous.connections[agent]["entries"]["acme_cloud"],
                    next.connections[agent]["entries"]["acme_cloud"]
                );
                let old_transport = super::super::connections::projected(
                    root.path(),
                    &previous,
                    agent,
                    "cloud",
                    &original_connections["cloud"],
                )
                .unwrap();
                assert!(
                    integrations::verify_mcp_connection(
                        root.path(),
                        agent,
                        "acme_cloud",
                        &old_transport
                    )
                    .is_err()
                );
            }
        }
        assert_eq!(
            std::fs::read(root.path().join(".claude/settings.local.json")).unwrap(),
            hooks
        );
        let shared: Value =
            serde_json::from_str(&std::fs::read_to_string(root.path().join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            shared["mcpServers"]["manual"]["url"],
            "https://manual.example.com/mcp"
        );
    }
}

#[test]
fn connection_publication_removal_disable_and_alias_change_revoke_old_locators() {
    for operation in ["removed", "disabled", "alias"] {
        let (root, context) = fixture(true);
        let previous = registered(&context);
        let old = launch(&previous, "claude", "cloud");
        commit(&context, true, operation);
        assert!(
            integrations::resolve_mcp_token(root.path(), &old.token).is_err(),
            "{operation}"
        );
        let next = registered(&context);
        let neighbor = launch(&next, "claude", "neighbor");
        assert!(integrations::resolve_mcp_token(root.path(), &neighbor.token).is_ok());
        if operation == "alias" {
            let next = launch(&next, "claude", "cloud");
            assert_ne!(old.token, next.token);
            assert!(integrations::resolve_mcp_token(root.path(), &next.token).is_ok());
        }
    }
}

fn interrupted(context: &Context) -> (Plan, String) {
    let before = state(context);
    let candidate = json!(bundle(true, "after"));
    let source = BTreeMap::from([("project".into(), candidate.clone())]);
    let evaluator =
        crate::configuration::Evaluator::new(review::snapshots(&source).unwrap()).unwrap();
    let plan = prepare(
        Path::new(&context.project_root),
        &before.operator,
        &connections(&before),
        evaluator.effective().mcp_connections(),
        evaluator.revision(),
    )
    .unwrap()
    .unwrap();
    let mut metadata = before.operator.clone();
    stage(&mut metadata, &plan, None).unwrap();
    let compiled = review::cache::build_candidate(
        context,
        &source,
        evaluator.revision(),
        &metadata,
        &evaluator,
    )
    .unwrap();
    let receipt = Store::open(Path::new(&context.project_root))
        .unwrap()
        .update_checkpoint(context, &|| Ok(()), |state| {
            state.candidates = source;
            state.policy_revision = evaluator.revision().into();
            state.operator = metadata.clone();
            let receipt = review::runtime::publish(state, context, None, compiled);
            super::bind(state, &mut metadata, &receipt);
            Ok(receipt)
        })
        .unwrap();
    (plan, receipt["activation_id"].as_str().unwrap().into())
}

#[test]
fn connection_publication_resumes_after_activation_and_partial_host_write_without_new_setup() {
    let (root, context) = fixture(true);
    let previous = registered(&context);
    let old = launch(&previous, "claude", "cloud");
    let neighbor = launch(&previous, "claude", "neighbor");
    let (plan, activation) = interrupted(&context);
    let sequence_before = state(&context).sequence;
    resume_launch(&context, &"0".repeat(64)).unwrap();
    resume_launch(&context, &neighbor.token).unwrap();
    assert_eq!(state(&context).sequence, sequence_before);
    assert!(integrations::resolve_mcp_token(root.path(), &old.token).is_err());
    assert!(integrations::resolve_mcp_token(root.path(), &neighbor.token).is_ok());
    integrations::publish(root.path(), &plan).unwrap();
    let mut shared: Value =
        serde_json::from_str(&std::fs::read_to_string(root.path().join(".mcp.json")).unwrap())
            .unwrap();
    shared["mcpServers"]["added_while_interrupted"] =
        json!({"type":"http","url":"https://new.example.com/mcp"});
    std::fs::write(root.path().join(".mcp.json"), shared.to_string()).unwrap();
    resume_launch(&context, &old.token).unwrap();
    let current = state(&context);
    let configured = connections(&current);
    let (_, binding) = integrations::resolve_mcp_token(root.path(), &old.token).unwrap();
    assert!(binding.matches(&configured["cloud"]));
    assert_eq!(current.active.as_ref().unwrap().activation_id(), activation);
    let sequence = current.sequence;
    assert!(resume(&context, None, &activation).unwrap());
    assert_eq!(state(&context).sequence, sequence);
    let shared: Value =
        serde_json::from_str(&std::fs::read_to_string(root.path().join(".mcp.json")).unwrap())
            .unwrap();
    assert_eq!(
        shared["mcpServers"]["added_while_interrupted"]["url"],
        "https://new.example.com/mcp"
    );
}

#[test]
fn connection_publication_conflicts_preserve_pending_work_and_refuse_stale_activation() {
    let (root, context) = fixture(true);
    let (plan, activation) = interrupted(&context);
    let codex = root.path().join(".codex/config.toml");
    let original = std::fs::read_to_string(&codex).unwrap();
    std::fs::write(&codex, original.replace("acme_cloud", "manually_changed")).unwrap();
    let changed = std::fs::read(&codex).unwrap();
    assert!(!resume(&context, None, "foreign-activation").unwrap());
    assert!(resume(&context, None, &activation).is_err());
    assert_eq!(std::fs::read(&codex).unwrap(), changed);
    assert_eq!(
        json!(pending(&state(&context).operator).unwrap().unwrap()),
        json!(plan)
    );
    let mut next = state(&context);
    next.operator["integration_publication"]["proposal_id"] = json!("later-review");
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator = next.operator;
            Ok(json!({}))
        })
        .unwrap();
    assert!(!resume(&context, None, &activation).unwrap());
    assert_eq!(std::fs::read(&codex).unwrap(), changed);
}

#[test]
fn connection_publication_revalidates_activation_inside_the_host_publication_writer() {
    let (root, context) = fixture(true);
    let (plan, activation) = interrupted(&context);
    let expected = state(&context).operator;
    let before = std::fs::read(root.path().join(".mcp.json")).unwrap();
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            let compiled = state.active.as_ref().unwrap().compiled.clone();
            review::runtime::publish(state, &context, None, compiled);
            Ok(json!({}))
        })
        .unwrap();
    assert!(matches!(
        integrations::apply_owned(&context, &expected, &plan, &|| Ok(()), Some(&activation)),
        Err(Error::StaleSnapshot)
    ));
    assert_eq!(
        std::fs::read(root.path().join(".mcp.json")).unwrap(),
        before
    );
    assert_eq!(
        json!(pending(&state(&context).operator).unwrap().unwrap()),
        json!(plan)
    );
}

#[test]
fn connection_publication_scopes_alias_updates_and_skips_permission_only_mapping_changes() {
    let (root, context) = fixture(true);
    let before = state(&context);
    let configured = connections(&before);
    let mut changed = configured.clone();
    changed
        .get_mut("cloud")
        .unwrap()
        .agents
        .insert("codex".into(), "acme_next".into());
    let plan = prepare(
        root.path(),
        &before.operator,
        &configured,
        &changed,
        "reviewed-next",
    )
    .unwrap()
    .unwrap();
    assert_eq!(plan.targets, Some(vec!["codex".into()]));
    assert!(plan.before.is_none() && plan.after.is_empty());
    assert_eq!(
        plan.desired.as_ref().unwrap().connections["claude"],
        plan.previous.as_ref().unwrap().connections["claude"]
    );
    let mut changed = configured.clone();
    changed.get_mut("cloud").unwrap().command_fields.insert(
        "inspect".into(),
        crate::mcp_connections::CommandField {
            executable: "acme".into(),
            pointer: "/argv".into(),
            encoding: "argv-v1".into(),
        },
    );
    assert!(
        prepare(
            root.path(),
            &before.operator,
            &configured,
            &changed,
            "reviewed-next"
        )
        .unwrap()
        .is_none()
    );
}
