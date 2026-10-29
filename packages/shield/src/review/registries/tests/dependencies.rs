//! Signed TLS closure acquisition exercises shared CLI/MCP preparation and offline review.
use super::super::*;
use crate::review::{
    self,
    mcp::{Connection, call},
};
use canonic_registry::testing::{Response, Server, SigningKey};
use std::cell::Cell;

/// A signed registry whose catalogs the tests edit before signing.
struct Registry {
    server: Server,
    key: SigningKey,
    entries: Vec<Value>,
    /// Requests already received before the last [`Registry::clear`].
    cleared: Cell<usize>,
}

/// The fixture CA's fetcher with the ordinary test deadline.
fn trusted() -> Fetcher {
    Server::fetcher(Duration::from_secs(2))
}

impl Registry {
    fn new() -> Self {
        let key = SigningKey::generate();
        let server = Server::new();
        server.set("catalog-key.pub", Response::ok(key.public_hex()));
        let mut registry = Self {
            server,
            key,
            entries: Vec::new(),
            cleared: Cell::new(0),
        };
        registry.release("parent", "1.0.0", Some("bridge"));
        registry.release("bridge", "1.0.0", Some("child"));
        registry.release("child", "1.0.0", None);
        registry.publish(1);
        registry
    }

    fn release(&mut self, name: &str, version: &str, dependency: Option<&str>) {
        let mut pack = json!({"schema_version":1,"id":format!("acme/{name}"),"version":version,"description":"Acme TLS dependency fixture","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":[],"rules":[],"kind":"pack"});
        if let Some(dependency) = dependency {
            pack["dependencies"] =
                json!([{"pack":format!("acme/{dependency}"),"requirement":format!("^{version}")}]);
        }
        self.artifact(&pack);
    }

    fn artifact(&mut self, pack: &Value) {
        let path = format!(
            "{}-{}.json",
            pack["id"].as_str().unwrap().replace('/', "-"),
            pack["version"].as_str().unwrap()
        );
        let bytes = pack.to_string();
        self.server.set(&path, Response::ok(bytes.clone()));
        self.entries.push(json!({"id":pack["id"],"version":pack["version"],"path":path,"sha256":policy::fingerprint(bytes.as_bytes())}));
    }

    fn catalog(&self, sequence: u64) -> Value {
        json!({"schema_version":1,"registry_id":"acme","sequence":sequence,"expires_at":"9999-12-31T00:00:00Z","packs":self.entries})
    }

    fn signed(&self, catalog: &Value) {
        let bytes = catalog.to_string();
        self.server.set(
            "catalog.sig",
            Response::ok(self.key.sign_hex(bytes.as_bytes())),
        );
        self.server.set("catalog.json", Response::ok(bytes));
    }

    fn publish(&self, sequence: u64) {
        self.signed(&self.catalog(sequence));
    }

    fn next(&mut self) {
        self.release("parent", "2.0.0", Some("bridge"));
        self.release("bridge", "2.0.0", Some("child"));
        self.release("child", "2.0.0", None);
        self.publish(2);
    }

    /// Counts only requests received after this.
    fn clear(&self) {
        self.cleared.set(self.server.requests().len());
    }

    fn requests(&self, name: &str) -> usize {
        self.server.requests()[self.cleared.get()..]
            .iter()
            .filter(|p| p.ends_with(name))
            .count()
    }
}

fn project(registry: &Registry) -> (assert_fs::TempDir, Context, Connection) {
    let (root, mut context) = review::tests::enrolled();
    context.fetcher = trusted();
    let source = enrollment(&context.fetcher, registry.server.url()).unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator = crate::review::project::metadata(state);
            state.operator["sources"]["hosted"] = json!(source);
            Ok(json!({}))
        })
        .unwrap();
    context.surface = "mcp".into();
    context.connection_id = Some("tls-dependencies".into());
    let connection = Connection::new("tls-dependencies".into());
    (root, context, connection)
}

fn propose(context: &Context, connection: &Connection, kind: &str, key: &str) -> Value {
    let mut change = json!({"kind":kind});
    if kind == "pack_add" {
        change["reference"] = json!("hosted/acme/parent");
        change["profile"] = json!("strict");
    }
    call(
        context,
        connection,
        "policy_propose",
        &json!({"change":change,"idempotency_key":key}),
        &|| Ok(()),
    )
}

fn commit(context: &Context, proposal: &Value) {
    assert_eq!(proposal["error"], Value::Null, "{proposal}");
    review::operator::commit_proposal_fixture(
        context,
        proposal["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
}

#[test]
fn signed_https_add_and_update_capture_transitive_closures_for_offline_confirmation() {
    let mut registry = Registry::new();
    let (root, context, connection) = project(&registry);
    let custom =
        review::tests::propose(&context, &review::tests::input("custom"), &|| Ok(())).unwrap();
    review::operator::commit_proposal_fixture(&context, custom["proposal_id"].as_str().unwrap())
        .unwrap();
    registry.clear();
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let added = propose(&context, &connection, "pack_add", "add");
    assert_eq!(added["error"], Value::Null, "{added}");
    let captured = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(captured.candidates, before.candidates);
    assert_eq!(captured.operator, before.operator);
    assert_eq!(registry.requests("catalog.json"), 1);
    assert_eq!(registry.requests("catalog.sig"), 1);
    // Network failure after proposal capture cannot change reviewed bytes.
    registry.server.set(
        "catalog.json",
        Response {
            status: 404,
            ..Response::ok(Vec::new())
        },
    );
    commit(&context, &added);
    let installed = Store::open(root.path()).unwrap().read().unwrap();
    let rows = installed.operator["installed"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r["version"] == "1.0.0"
        && r["profile"] == "strict"
        && r["source"]["key"].is_string()));
    assert_eq!(rows.iter().filter(|r| r["direct"] == true).count(), 1);
    assert!(
        rows.iter()
            .all(|r| r["source_identity"] == rows[0]["source_identity"])
    );
    registry.next();
    registry.clear();
    let updated = propose(&context, &connection, "pack_update", "update");
    assert_eq!(updated["error"], Value::Null, "{updated}");
    let pending = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(json!(pending.active), json!(installed.active));
    assert_eq!(pending.operator, installed.operator);
    assert_eq!(registry.requests("catalog.json"), 1);
    drop(registry);
    commit(&context, &updated);
    let active = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(
        active.candidates["project"]["rules"],
        before.candidates["project"]["rules"]
    );
    for row in active.operator["installed"].as_array().unwrap() {
        assert_eq!(row["version"], "2.0.0");
        let previous = rows.iter().find(|r| r["id"] == row["id"]).unwrap();
        assert_eq!(row["selector"], previous["selector"]);
        assert_eq!(row["profile"], previous["profile"]);
    }
}

#[test]
fn signed_https_dependency_failures_preserve_active_policy_and_create_no_proposal() {
    for failure in [
        "signature",
        "key",
        "expiry",
        "identity",
        "rollback",
        "replacement",
        "release-replacement",
        "digest",
        "missing",
        "redirect",
        "oversized",
        "deadline",
    ] {
        let mut registry = Registry::new();
        let (root, context, connection) = project(&registry);
        commit(
            &context,
            &propose(&context, &connection, "pack_add", "install"),
        );
        registry.next();
        let mut catalog = registry.catalog(2);
        match failure {
            "signature" => registry
                .server
                .set("catalog.sig", Response::ok("0".repeat(128))),
            "key" => {
                let key = SigningKey::generate();
                registry
                    .server
                    .set("catalog-key.pub", Response::ok(key.public_hex()));
                registry.server.set(
                    "catalog.sig",
                    Response::ok(key.sign_hex(catalog.to_string().as_bytes())),
                );
            }
            "expiry" | "identity" | "rollback" | "replacement" | "release-replacement" => {
                match failure {
                    "expiry" => catalog["expires_at"] = json!("2000-01-01T00:00:00Z"),
                    "identity" => catalog["registry_id"] = json!("foreign"),
                    "rollback" => catalog["sequence"] = json!(0),
                    "release-replacement" => {
                        catalog["packs"][2]["sha256"] = json!("0".repeat(64));
                    }
                    _ => catalog["sequence"] = json!(1),
                }
                registry.signed(&catalog);
            }
            "digest" => registry
                .server
                .set("acme-child-2.0.0.json", Response::ok("altered bytes")),
            "missing" | "redirect" => registry.server.set(
                "acme-child-2.0.0.json",
                Response {
                    status: if failure == "missing" { 404 } else { 302 },
                    ..Response::ok(Vec::new())
                },
            ),
            "oversized" => registry
                .server
                .set("acme-child-2.0.0.json", Response::ok(vec![b'x'; 1_048_577])),
            "deadline" => registry.server.set(
                "acme-child-2.0.0.json",
                Response {
                    delay: Duration::from_millis(800),
                    ..Response::ok(Vec::new())
                },
            ),
            _ => unreachable!(),
        }
        let before = Store::open(root.path()).unwrap().read().unwrap();
        let rejected = if failure == "deadline" {
            let mut context = context.clone();
            context.fetcher = Server::fetcher(Duration::from_millis(500));
            propose(&context, &connection, "pack_update", "invalid")
        } else {
            propose(&context, &connection, "pack_update", "invalid")
        };
        assert!(rejected["error"].is_object(), "{failure}: {rejected}");
        let after = Store::open(root.path()).unwrap().read().unwrap();
        assert_eq!(json!(after.active), json!(before.active), "{failure}");
        assert_eq!(after.operator, before.operator, "{failure}");
        assert_eq!(after.candidates, before.candidates, "{failure}");
        assert_eq!(after.proposals.len(), before.proposals.len(), "{failure}");
        assert_eq!(registry.requests("stolen.json"), 0);
        if failure == "deadline" {
            assert_eq!(registry.requests("acme-child-2.0.0.json"), 1);
        }
    }
}

#[test]
fn signed_https_pinned_parents_skip_refresh_and_constrain_explicit_dependency_updates() {
    let mut registry = Registry::new();
    let (root, context, connection) = project(&registry);
    let added = call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"pack_add","reference":"hosted/acme/parent@1.0.0","profile":"strict"},"idempotency_key":"pinned"}),
        &|| Ok(()),
    );
    commit(&context, &added);
    let before = Store::open(root.path()).unwrap().read().unwrap();
    registry.next();
    registry.release("child", "1.1.0", None);
    registry.publish(2);
    registry.clear();
    let skipped = propose(&context, &connection, "pack_update", "skip-pins");
    commit(&context, &skipped);
    assert_eq!(registry.requests("catalog.json"), 0);
    assert_eq!(
        Store::open(root.path()).unwrap().read().unwrap().operator,
        before.operator
    );
    let updated = call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"pack_update","reference":"acme/child"},"idempotency_key":"dependency"}),
        &|| Ok(()),
    );
    drop(registry);
    commit(&context, &updated);
    let after = Store::open(root.path()).unwrap().read().unwrap();
    let rows = after.operator["installed"].as_array().unwrap();
    assert_eq!(
        rows.iter().find(|r| r["id"] == "acme/parent").unwrap()["version"],
        "1.0.0"
    );
    let child = rows.iter().find(|r| r["id"] == "acme/child").unwrap();
    assert_eq!(child["version"], "1.1.0");
    assert_eq!(child["direct"], false);
    assert_eq!(child["profile"], "strict");
}

#[test]
fn signed_https_invalid_dependency_graphs_refuse_before_operator_review() {
    for failure in [
        "foreign",
        "cycle",
        "version",
        "profile",
        "artifact-identity",
        "shield-version",
    ] {
        let mut registry = Registry::new();
        let (root, context, connection) = project(&registry);
        let mut pack = json!({"schema_version":1,"id":"acme/child","version":"2.0.0","description":"Acme invalid dependency","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":[],"rules":[],"kind":"pack"});
        match failure {
            "foreign" => {
                pack["dependencies"] =
                    json!([{"pack":"acme/parent","requirement":"^1","registry":"foreign"}]);
            }
            "cycle" => {
                pack["dependencies"] = json!([{"pack":"acme/parent","requirement":"^2"}]);
            }
            "version" => {
                pack["dependencies"] = json!([{"pack":"acme/missing","requirement":"^9"}]);
            }
            "profile" => {
                pack["dependencies"] = json!([{"pack":"acme/parent","requirement":"^1","profiles":{"loose":"strict","recommended":"loose","strict":"strict"}}]);
            }
            "shield-version" => pack["engines"] = json!({"shield":"^99"}),
            _ => {}
        }
        registry.release("parent", "2.0.0", Some("bridge"));
        registry.release("bridge", "2.0.0", Some("child"));
        registry.artifact(&pack);
        if failure == "artifact-identity" {
            registry.entries.last_mut().unwrap()["id"] = json!("acme/other");
        }
        registry.publish(2);
        let before = Store::open(root.path()).unwrap().read().unwrap();
        let rejected = call(
            &context,
            &connection,
            "policy_propose",
            &json!({"change":{"kind":"pack_add","reference":"hosted/acme/parent@2.0.0"},"idempotency_key":"invalid"}),
            &|| Ok(()),
        );
        assert!(rejected["error"].is_object(), "{failure}: {rejected}");
        let after = Store::open(root.path()).unwrap().read().unwrap();
        assert_eq!(after.operator, before.operator, "{failure}");
        assert_eq!(after.candidates, before.candidates, "{failure}");
        assert_eq!(after.proposals.len(), before.proposals.len(), "{failure}");
    }
}

#[test]
fn cross_source_https_closures_verify_each_enrolled_key_and_confirm_offline() {
    for corrupt in [false, true] {
        let mut a = Registry::new();
        let b = Registry::new();
        let foreign = enrollment(&trusted(), b.server.url()).unwrap();
        let parent = json!({"schema_version":1,"id":"acme/parent","version":"2.0.0","description":"Acme foreign HTTPS dependency","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":[{"pack":"acme/bridge","requirement":"^1","registry":foreign.identity()}],"rules":[],"kind":"pack"});
        a.artifact(&parent);
        a.publish(2);
        let (root, context, connection) = project(&a);
        Store::open(root.path())
            .unwrap()
            .update(&context, &|| Ok(()), |state| {
                state.operator["sources"]["foreign"] = json!(foreign);
                state.operator["default"] = json!("hosted");
                Ok(json!({}))
            })
            .unwrap();
        a.clear();
        b.clear();
        if corrupt {
            b.server.set("catalog.sig", Response::ok("0".repeat(128)));
        }
        let before = Store::open(root.path()).unwrap().read().unwrap();
        let added = call(
            &context,
            &connection,
            "policy_propose",
            &json!({"change":{"kind":"pack_add","reference":"hosted/acme/parent@2.0.0","profile":"strict"},"idempotency_key":"foreign"}),
            &|| Ok(()),
        );
        let pending = Store::open(root.path()).unwrap().read().unwrap();
        assert_eq!(pending.candidates, before.candidates);
        assert_eq!(pending.operator, before.operator);
        assert_eq!(a.requests("catalog.json"), 1);
        assert_eq!(b.requests("catalog.json"), 1);
        assert_eq!(a.requests("catalog.sig"), 1);
        assert_eq!(b.requests("catalog.sig"), 1);
        assert_eq!(b.requests("catalog-key.pub"), 0);
        drop(a);
        drop(b);
        if corrupt {
            assert!(added["error"].is_object(), "{added}");
            assert_eq!(pending.proposals.len(), before.proposals.len());
        } else {
            commit(&context, &added);
            let active = Store::open(root.path()).unwrap().read().unwrap();
            let rows = active.operator["installed"].as_array().unwrap();
            assert_eq!(rows.len(), 3);
            let parent = rows.iter().find(|r| r["id"] == "acme/parent").unwrap();
            let child = rows.iter().find(|r| r["id"] == "acme/child").unwrap();
            assert_ne!(parent["source_identity"], child["source_identity"]);
            assert_eq!(child["source_identity"], foreign.identity());
            assert_ne!(parent["source"]["key"], child["source"]["key"]);
            assert!(rows.iter().all(|r| r["profile"] == "strict"));
        }
    }
}

#[test]
fn cross_source_mixed_local_and_https_closures_share_verification_and_review() {
    for hosted_root in [false, true] {
        let mut registry = Registry::new();
        let signed = enrollment(&trusted(), registry.server.url()).unwrap();
        let local = assert_fs::TempDir::new().unwrap();
        let edges = if hosted_root {
            json!([])
        } else {
            json!([{"pack":"acme/bridge","requirement":"^1","registry":signed.identity()}])
        };
        let bytes = json!({"schema_version":1,"id":"acme/local","version":"1.0.0","description":"Acme mixed source fixture","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":edges,"rules":[],"kind":"pack"}).to_string();
        std::fs::write(local.path().join("pack.json"), &bytes).unwrap();
        std::fs::write(local.path().join("catalog.json"), json!({"schema_version":1,"packs":[{"id":"acme/local","version":"1.0.0","path":"pack.json","sha256":policy::fingerprint(bytes.as_bytes())}]}).to_string()).unwrap();
        let local_source = enrollment(&trusted(), local.path().to_str().unwrap()).unwrap();
        if hosted_root {
            let parent = json!({"schema_version":1,"id":"acme/parent","version":"2.0.0","description":"Acme local dependency","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["dependencies-v1","structured-rules-v1"],"dependencies":[{"pack":"acme/local","requirement":"^1","registry":local_source.identity()}],"rules":[],"kind":"pack"});
            registry.artifact(&parent);
            registry.publish(2);
        }
        let (root, context, connection) = project(&registry);
        Store::open(root.path())
            .unwrap()
            .update(&context, &|| Ok(()), |state| {
                state.operator["sources"]["local"] = json!(local_source);
                Ok(json!({}))
            })
            .unwrap();
        let reference = if hosted_root {
            "hosted/acme/parent@2.0.0"
        } else {
            "local/acme/local"
        };
        let added = call(
            &context,
            &connection,
            "policy_propose",
            &json!({"change":{"kind":"pack_add","reference":reference},"idempotency_key":"mixed"}),
            &|| Ok(()),
        );
        drop(local);
        drop(registry);
        commit(&context, &added);
        let active = Store::open(root.path()).unwrap().read().unwrap();
        let rows = active.operator["installed"].as_array().unwrap();
        assert_eq!(rows.len(), if hosted_root { 2 } else { 3 });
        assert!(rows.iter().any(|r| r["source"]["key"].is_null()));
        assert!(rows.iter().any(|r| r["source"]["key"].is_string()));
    }
}
