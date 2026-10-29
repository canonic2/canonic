//! Enrolled identity acquisition, cross-source constraints and offline closure ownership.
use super::*;

struct Local {
    root: assert_fs::TempDir,
    identity: String,
    entries: Vec<Value>,
    sequence: u64,
}
impl Local {
    fn new() -> Self {
        let root = assert_fs::TempDir::new().unwrap();
        let identity = crate::policy::fingerprint(
            root.path()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .as_bytes(),
        );
        Self {
            root,
            identity,
            entries: Vec::new(),
            sequence: 0,
        }
    }
    fn source(&self) -> Value {
        json!({"identity":self.identity,"location":self.root.path().canonicalize().unwrap(),"key":null})
    }
    fn publish(&mut self, name: &str, version: &str, edges: Value) {
        let pack = json!({"schema_version":1,"id":format!("acme/{name}"),"version":version,"description":"Acme cross-source fixture","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":edges,"rules":[],"kind":"pack"}).to_string();
        let path = format!("{name}-{version}.json");
        std::fs::write(self.root.path().join(&path), &pack).unwrap();
        let entry = json!({"id":format!("acme/{name}"),"version":version,"path":path,"sha256":crate::policy::fingerprint(pack.as_bytes())});
        self.entries
            .retain(|e| e["id"] != entry["id"] || e["version"] != entry["version"]);
        self.entries.push(entry);
        self.sequence += 1;
        std::fs::write(
            self.root.path().join("catalog.json"),
            json!({"schema_version":1,"sequence":self.sequence,"packs":self.entries}).to_string(),
        )
        .unwrap();
        drop(edges);
    }
}

fn sources(context: &Context) -> (Local, Local) {
    let mut a = Local::new();
    let mut b = Local::new();
    a.publish("child", "1.0.0", json!([]));
    b.publish("child", "1.0.0", json!([]));
    b.publish("bridge", "1.0.0", bridge(&a.identity, "^1"));
    a.publish("parent", "1.0.0", edge(&b.identity, "bridge", "^1"));
    a.publish("sibling", "1.0.0", edge(&b.identity, "bridge", "^1"));
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            state.operator["sources"]["a"] = a.source();
            state.operator["sources"]["b"] = b.source();
            state.operator["default"] = json!("a");
            Ok(json!({}))
        })
        .unwrap();
    (a, b)
}

fn edge(identity: &str, name: &str, requirement: &str) -> Value {
    json!([{"pack":format!("acme/{name}"),"registry":identity,"requirement":requirement}])
}
fn bridge(identity: &str, requirement: &str) -> Value {
    json!([{"pack":"acme/child","requirement":requirement},{"pack":"acme/child","registry":identity,"requirement":requirement}])
}
fn read(context: &Context) -> review::model::State {
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .read()
        .unwrap()
}
fn add(context: &Context, connection: &Connection, reference: &str, key: &str) {
    commit_pack_change(
        context,
        connection,
        json!({"kind":"pack_add","reference":reference,"profile":"strict"}),
        key,
    );
}
fn unchanged_policy(before: &review::model::State, after: &review::model::State) {
    assert_eq!(json!(after.active), json!(before.active));
    assert_eq!(after.operator, before.operator);
    assert_eq!(after.candidates, before.candidates);
}

/// Removing the bare `acme/child` name lists both installed children, and the
/// published output schema declares every field of that ambiguity error.
fn assert_bare_child_removal_is_ambiguous(
    context: &Context,
    connection: &Connection,
    children: &[&Value],
) {
    let ambiguous = pack_change(
        context,
        connection,
        json!({"kind":"pack_remove","reference":"acme/child"}),
        "ambiguous",
    );
    assert_eq!(
        ambiguous["error"]["code"], "ambiguous_reference",
        "{ambiguous}"
    );
    assert_eq!(ambiguous["error"]["alternatives_total"], 2, "{ambiguous}");
    let alternatives = ambiguous["error"]["alternatives"].as_array().unwrap();
    for (alternative, child) in alternatives.iter().zip(children) {
        let qualified = format!("{}/acme/child", child["source_alias"].as_str().unwrap());
        assert!(
            *alternative == child["selector"] || *alternative == json!(qualified),
            "{ambiguous}"
        );
        let message = ambiguous["error"]["message"].as_str().unwrap();
        assert!(
            message.contains(alternative.as_str().unwrap()),
            "{ambiguous}"
        );
    }
    let catalog = tools();
    let propose = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "policy_propose")
        .unwrap();
    let output = &propose["outputSchema"]["properties"];
    for key in ambiguous.as_object().unwrap().keys() {
        assert!(output.get(key).is_some(), "{key}: {ambiguous}");
    }
    let declared = &output["error"]["anyOf"][0]["properties"];
    for key in ambiguous["error"].as_object().unwrap().keys() {
        assert!(declared.get(key).is_some(), "{key}: {ambiguous}");
    }
    let limit = output["error"]["anyOf"][0]["properties"]["alternatives"]["maxItems"]
        .as_u64()
        .unwrap();
    assert!(alternatives.len() as u64 <= limit, "{ambiguous}");
}

#[test]
fn cross_source_add_captures_colliding_names_transitive_edges_and_offline_removal() {
    let (_root, context, connection) = fixture();
    let (a, b) = sources(&context);
    let custom = proposal(&context, &connection, "custom");
    review::operator::commit_proposal_fixture(
        &context,
        custom["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    let before = read(&context);
    let added = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"a/acme/parent","profile":"strict"}),
        "closure",
    );
    assert_eq!(added["error"], Value::Null, "{added}");
    unchanged_policy(&before, &read(&context));
    drop(a);
    drop(b);
    review::operator::commit_proposal_fixture(
        &context,
        added["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    let installed = read(&context);
    let rows = installed.operator["installed"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let children: Vec<_> = rows.iter().filter(|r| r["id"] == "acme/child").collect();
    assert_eq!(children.len(), 2);
    assert_ne!(
        children[0]["source_identity"],
        children[1]["source_identity"]
    );
    assert_ne!(children[0]["selector"], children[1]["selector"]);
    assert!(rows.iter().all(|r| r["profile"] == "strict"));
    for child in &children {
        assert_eq!(child["incoming"][0]["parent"], "acme/bridge");
        assert_eq!(
            child["incoming"][0]["registry_identity"],
            rows.iter().find(|r| r["id"] == "acme/bridge").unwrap()["source_identity"]
        );
    }
    assert_bare_child_removal_is_ambiguous(&context, &connection, &children);
    let blocked = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":children[0]["selector"]}),
        "required",
    );
    assert_eq!(blocked["error"]["code"], "dependency_required");
    add(
        &context,
        &connection,
        children[0]["selector"].as_str().unwrap(),
        "direct-child",
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/parent"}),
        "prune",
    );
    let remaining = read(&context);
    assert_eq!(remaining.operator["installed"].as_array().unwrap().len(), 1);
    assert_eq!(
        remaining.operator["installed"][0]["selector"],
        children[0]["selector"]
    );
    assert_eq!(remaining.operator["installed"][0]["direct"], true);
    assert_eq!(
        remaining.candidates["project"]["rules"],
        before.candidates["project"]["rules"]
    );
}

/// Moves an enrolled source to a new alias, keeping its identity.
fn rename_source(context: &Context, from: &str, to: &str) {
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update(context, &|| Ok(()), |state| {
            let value = state.operator["sources"]
                .as_object_mut()
                .unwrap()
                .remove(from)
                .unwrap();
            state.operator["sources"][to] = value;
            Ok(json!({}))
        })
        .unwrap();
}

/// After the update, each row keeps its source and profile, pinned rows stay
/// at 1.0.0, unpinned rows take the newest allowed release, and the shared
/// bridge keeps both incoming constraints.
fn assert_refreshed_unpinned_rows(before: &review::model::State, after: &review::model::State) {
    for row in after.operator["installed"].as_array().unwrap() {
        let old = before.operator["installed"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["selector"] == row["selector"])
            .unwrap();
        assert_eq!(row["source_identity"], old["source_identity"]);
        assert_eq!(row["profile"], old["profile"]);
        if row["pinned"] == true {
            assert_eq!(row["version"], "1.0.0");
        } else {
            assert_eq!(
                row["version"],
                if row["id"] == "acme/parent" {
                    "2.0.0"
                } else {
                    "1.1.0"
                }
            );
        }
    }
    let bridge = after.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/bridge")
        .unwrap();
    assert_eq!(bridge["incoming"].as_array().unwrap().len(), 2);
}

#[test]
fn cross_source_updates_refresh_foreign_dependencies_and_preserve_pins_and_shared_constraints() {
    let (_root, context, connection) = fixture();
    let (mut a, mut b) = sources(&context);
    add(&context, &connection, "a/acme/parent", "parent");
    add(
        &context,
        &connection,
        "a/acme/sibling@1.0.0",
        "pinned-parent",
    );
    add(&context, &connection, "b/acme/child@1.0.0", "pinned-child");
    a.publish("child", "1.1.0", json!([]));
    b.publish("child", "1.1.0", json!([]));
    b.publish("bridge", "1.1.0", bridge(&a.identity, "^1"));
    b.publish("bridge", "2.0.0", bridge(&a.identity, "^2"));
    a.publish("parent", "2.0.0", edge(&b.identity, "bridge", ">=1,<3"));
    rename_source(&context, "b", "renamed");
    let before = read(&context);
    let updated = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update"}),
        "update",
    );
    assert_eq!(updated["error"], Value::Null, "{updated}");
    unchanged_policy(&before, &read(&context));
    // Pinned roots are reported as skipped.
    let pinned: Vec<_> = before.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["pinned"] == true)
        .map(|row| json!({"selector":row["selector"],"id":row["id"],"version":row["version"]}))
        .collect();
    assert_eq!(pinned.len(), 2, "{}", before.operator);
    assert_eq!(
        updated["data"]["skipped_pinned"],
        json!(pinned),
        "{updated}"
    );
    drop(a);
    drop(b);
    review::operator::commit_proposal_fixture(
        &context,
        updated["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    let after = read(&context);
    assert_refreshed_unpinned_rows(&before, &after);
    // The current alias bound to the retained source identity names its rows.
    let child = after.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "acme/child" && r["source_alias"] == "b")
        .unwrap()
        .clone();
    assert_eq!(child["pinned"], true);
    let named = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_update","reference":"renamed/acme/child"}),
        "renamed-update",
    );
    assert_eq!(named["error"], Value::Null, "{named}");
    assert_eq!(
        named["data"]["skipped_pinned"],
        json!([{"selector":child["selector"],"id":"acme/child","version":"1.0.0"}]),
        "{named}"
    );
    // Adding an installed selection without a version keeps its version and pin.
    let readded = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"renamed/acme/child"}),
        "renamed-add",
    );
    assert_eq!(readded["error"], Value::Null, "{readded}");
    let proposal = read(&context)
        .proposals
        .into_iter()
        .find(|p| readded["data"]["proposal_id"] == p.id)
        .unwrap();
    let rows = proposal.candidate_metadata["installed"].as_array().unwrap();
    let kept = rows
        .iter()
        .find(|r| r["selector"] == child["selector"])
        .unwrap();
    assert_eq!(kept["pinned"], true, "{kept}");
    assert_eq!(kept["version"], "1.0.0", "{kept}");
    assert_eq!(
        rows.len(),
        after.operator["installed"].as_array().unwrap().len()
    );
}

#[test]
fn cross_source_bare_add_acquires_from_the_default_source_beside_another_sources_row() {
    let (_root, context, connection) = fixture();
    let (a, b) = sources(&context);
    add(&context, &connection, "b/acme/child", "other-source");
    let retained = read(&context).operator["installed"][0].clone();
    assert_eq!(retained["source_identity"], json!(b.identity));
    // A bare name uses the default source `a`; b's same-named row does not answer.
    add(&context, &connection, "acme/child", "default-source");
    let rows = read(&context).operator["installed"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let kept = rows
        .iter()
        .find(|row| row["selector"] == retained["selector"])
        .unwrap();
    for field in ["source_identity", "version", "direct", "pinned", "profile"] {
        assert_eq!(kept[field], retained[field], "{field}: {kept}");
    }
    let added = rows
        .iter()
        .find(|row| row["selector"] != retained["selector"])
        .unwrap();
    assert_eq!(added["id"], "acme/child");
    assert_eq!(added["source_identity"], json!(a.identity));
    assert_eq!(added["direct"], true);
    // An alias-qualified name keeps matching its own source's installed row.
    let readded = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"b/acme/child"}),
        "same-source",
    );
    assert_eq!(readded["error"], Value::Null, "{readded}");
    let proposal = read(&context)
        .proposals
        .into_iter()
        .find(|p| readded["data"]["proposal_id"] == p.id)
        .unwrap();
    assert_eq!(
        proposal.candidate_metadata["installed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn cross_source_add_of_a_missing_catalog_name_is_not_found() {
    let (_root, context, connection) = fixture();
    let (_a, _b) = sources(&context);
    let missing = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"a/acme/absent"}),
        "missing",
    );
    assert_eq!(missing["error"]["code"], "not_found", "{missing}");
}

#[test]
fn cross_source_enrollment_and_artifact_failures_preserve_policy_and_pending_proposals() {
    for failure in ["unenrolled", "alias", "binding", "artifact", "release"] {
        let (_root, context, connection) = fixture();
        let (a, mut b) = sources(&context);
        add(&context, &connection, "a/acme/parent", "install");
        let other = Local::new();
        match failure {
            "unenrolled" | "alias" | "binding" => {
                Store::open(Path::new(&context.project_root))
                    .unwrap()
                    .update(&context, &|| Ok(()), |state| {
                        let sources = state.operator["sources"].as_object_mut().unwrap();
                        sources.remove("b");
                        if failure != "unenrolled" {
                            let mut source = other.source();
                            if failure == "binding" {
                                source["identity"] = json!(b.identity);
                            }
                            sources.insert("b".into(), source);
                        }
                        Ok(json!({}))
                    })
                    .unwrap();
            }
            "artifact" => {
                std::fs::write(b.root.path().join("child-1.0.0.json"), "altered").unwrap();
            }
            _ => b.publish("child", "1.0.0", edge(&a.identity, "parent", "^1")),
        }
        let before = read(&context);
        let failed = pack_change(
            &context,
            &connection,
            json!({"kind":"pack_update"}),
            "failed",
        );
        assert!(failed["error"].is_object(), "{failure}: {failed}");
        let after = read(&context);
        unchanged_policy(&before, &after);
        assert_eq!(after.proposals.len(), before.proposals.len());
        drop(a);
        drop(b);
        // Existing locks remain usable without registrations or their source directories.
        add(&context, &connection, "acme/parent", "offline-profile");
    }
}

#[test]
fn cross_source_unenrolled_identities_and_cycles_refuse_initial_installation() {
    for failure in ["identity", "alias", "cycle", "profile", "version"] {
        let (_root, context, connection) = fixture();
        let (mut a, mut b) = sources(&context);
        match failure {
            "identity" => a.publish("parent", "1.0.0", edge("unenrolled", "bridge", "^1")),
            "alias" => a.publish("parent", "1.0.0", edge("b", "bridge", "^1")),
            "cycle" => b.publish("child", "1.0.0", edge(&a.identity, "parent", "^1")),
            "version" => a.publish("parent", "1.0.0", edge(&b.identity, "bridge", "^9")),
            _ => {
                add(&context, &connection, "b/acme/child", "direct");
                let child = read(&context).operator["installed"][0]["selector"].clone();
                commit_pack_change(
                    &context,
                    &connection,
                    json!({"kind":"pack_add","reference":child,"profile":"loose"}),
                    "weaken",
                );
            }
        }
        let before = read(&context);
        let failed = pack_change(
            &context,
            &connection,
            json!({"kind":"pack_add","reference":"a/acme/parent","profile":"strict"}),
            "failed",
        );
        assert!(failed["error"].is_object(), "{failure}: {failed}");
        let after = read(&context);
        unchanged_policy(&before, &after);
        assert_eq!(after.proposals.len(), before.proposals.len());
    }
}

#[test]
fn cross_source_add_preserves_unrelated_roots_without_refreshing_their_sources() {
    let (_root, context, connection) = fixture();
    let (_a, _b) = sources(&context);
    let mut unrelated = Local::new();
    unrelated.publish("standalone", "1.0.0", json!([]));
    Store::open(Path::new(&context.project_root))
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["sources"]["unrelated"] = unrelated.source();
            Ok(json!({}))
        })
        .unwrap();
    add(
        &context,
        &connection,
        "unrelated/acme/standalone",
        "unrelated",
    );
    let before = read(&context).operator["installed"][0].clone();
    drop(unrelated);
    add(&context, &connection, "a/acme/parent", "cross-source");
    let after = read(&context);
    let standalone = after.operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["selector"] == before["selector"])
        .unwrap();
    assert_eq!(standalone["version"], before["version"]);
    assert_eq!(standalone["profile"], before["profile"]);
    assert_eq!(standalone["source"], before["source"]);
    assert_eq!(after.operator["installed"].as_array().unwrap().len(), 5);
}
