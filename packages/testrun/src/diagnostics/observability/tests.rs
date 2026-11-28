use super::*;
#[cfg(unix)]
fn private(path: &std::path::Path, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
fn scope() -> Scope {
    Scope {
        account: "acme".into(),
        project: "api".into(),
        environment: "staging".into(),
        region: "eu".into(),
        resource: "api".into(),
        release: "v1".into(),
    }
}
#[test]
fn diagnostic_observability_selection_is_literal_and_credentials_are_scrubbed() {
    let mut s = scope();
    s.release = "v1\"\\literal".into();
    let labels = s
        .fields()
        .map(|(k, _)| (k.into(), k.into()))
        .into_iter()
        .collect();
    let q = selector(&s, &labels).unwrap();
    assert!(q.contains("release=\"v1\\\"\\\\literal\""));
    let mut v = json!({"authorization":"private","nested":{"token":"private","body":"a selected-secret b","selected-secret":"value"}});
    scrub(&mut v, "selected-secret");
    let raw = v.to_string();
    assert!(!raw.contains("private"));
    assert!(!raw.contains("selected-secret"));
    assert!(raw.contains("redacted-provider-credential"));
    let w = Window {
        from: "2026-01-01T00:00:00Z".into(),
        to: "2026-01-01T01:00:00Z".into(),
    };
    assert!(w.validate(60).is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_observability_http_projects_scope_and_stops_on_denial_rate_or_redirect() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    for (status, scenario) in [
        (200, "ok"),
        (401, "ok"),
        (403, "ok"),
        (429, "ok"),
        (302, "ok"),
        (200, "scope"),
        (200, "window"),
        (200, "records"),
        (200, "bytes"),
    ] {
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        let s = scope();
        let payload = scenario_payload(&s, scenario);
        let app = axum::Router::new().fallback(axum::routing::get(
            move |request: axum::extract::Request| {
                let seen = seen.clone();
                let payload = payload.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(request.uri().path(), "/loki/api/v1/query_range");
                    assert_eq!(
                        request.headers()["authorization"],
                        "Bearer acme-credential-secret"
                    );
                    let q = url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
                        .collect::<BTreeMap<_, _>>();
                    assert!(q["query"].contains("environment=\"staging\""));
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [
                            ("retry-after", "2"),
                            ("location", "http://127.0.0.1:1/never"),
                        ],
                        payload,
                    )
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("token");
        private(&token, b"acme-credential-secret");
        let file = dir.path().join("config.json");
        let labels: BTreeMap<_, _> = s.fields().into_iter().map(|(k, _)| (k, k)).collect();
        let config = json!({"schemaVersion":1,"sourceId":"incident","scope":s,"endpoint":format!("http://{addr}"),"authorizationOwner":"external","authorization":{"kind":"bearer_file","path":token},"read":{"kind":"loki","scopeLabels":labels},"timeoutMs":1000,"maxWindowSeconds":60,"maxRecords":10,"maxBytes":4096});
        private(&file, &serde_json::to_vec(&config).unwrap());
        let options = Options {
            config_file: file,
            source_id: "incident".into(),
            scope: s,
            window: Window {
                from: "2026-01-01T00:00:00Z".into(),
                to: "2026-01-01T00:01:00Z".into(),
            },
            include_text: false,
        };
        assert_wrong_scope_refused(&options).await;
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        let result = Capture::new(options)
            .unwrap()
            .read(Duration::from_secs(2))
            .await;
        if status == 200 && scenario == "ok" {
            let bytes = result.unwrap();
            let raw = String::from_utf8(bytes.clone()).unwrap();
            assert!(!raw.contains("private customer"));
            assert!(!raw.contains("must-omit"));
            assert!(!raw.contains("acme-credential-secret"));
            let i = super::super::analysis::decode::decode("observability-evidence-v1", &bytes)
                .unwrap();
            assert_eq!(i.rows.len(), 2);
        } else {
            let e = result.unwrap_err();
            assert_eq!(
                e.code,
                match (status, scenario) {
                    (401 | 403, _) => "OBSERVABILITY_DENIED",
                    (429, _) => "OBSERVABILITY_RATE_LIMITED",
                    (_, "scope" | "window") => "OBSERVABILITY_SCOPE",
                    (_, "records" | "bytes") => "OBSERVABILITY_BOUNDS",
                    _ => "OBSERVABILITY_HTTP",
                }
            );
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        server.abort();
        let _ = server.await;
    }
}
#[cfg(unix)]
#[test]
fn diagnostic_observability_private_files_refuse_symlinks_and_world_readable_credentials() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("token");
    private(&p, b"secret");
    assert!(crate::artifacts::read_private_file(&p, 3).is_err());
    let link = d.path().join("link");
    symlink(&p, &link).unwrap();
    assert!(crate::artifacts::read_private_file(&link, 64).is_err());
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(crate::artifacts::read_private_file(&p, 64).is_err());
}

#[cfg(unix)]
fn scenario_payload(s: &Scope, scenario: &str) -> String {
    let labels: serde_json::Map<String, Value> = s
        .fields()
        .into_iter()
        .map(|(k, v)| (k.into(), json!(v)))
        .collect();
    let mut payload = json!({"status":"success","sensitive":"must-omit","data":{"resultType":"streams","result":[{"stream":labels,"values":[["1767225600000000000","private customer text"]],"sensitive":"must-omit"}]}});
    match scenario {
        "scope" => payload["data"]["result"][0]["stream"]["environment"] = json!("production"),
        "window" => payload["data"]["result"][0]["values"][0][0] = json!("1"),
        "records" => {
            payload["data"]["result"][0]["values"] =
                json!(vec![json!(["1767225600000000000", "text"]); 11])
        }
        "bytes" => payload["sensitive"] = json!("x".repeat(5000)),
        _ => {}
    }
    payload.to_string()
}

#[cfg(unix)]
async fn assert_wrong_scope_refused(options: &Options) {
    let mut wrong = options.clone();
    wrong.scope.environment = "production".into();
    assert_eq!(
        Capture::new(wrong)
            .unwrap()
            .read(Duration::from_secs(2))
            .await
            .unwrap_err()
            .code,
        "OBSERVABILITY_SCOPE"
    );
}
