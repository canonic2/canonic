//! Private fixture evidence for an expired automation-pause assertion.
use super::*;

pub(super) fn observed_html(html: &str, selector: &str) -> String {
    format!(
        "{html}<script>({})({});</script>",
        include_str!("../../../fixtures/browser-readiness-observation.js"),
        serde_json::to_string(selector).unwrap()
    )
}

pub(super) fn attempt(
    fixture: &Fixture,
    target: &str,
    context: &Value,
    old: Option<&Value>,
    caller: (&str, &Value, &Value),
) -> std::path::PathBuf {
    let path = fixture.workspace.path().join(format!(
        "debugger-pause-{}-{}.json",
        caller.0,
        context["id"].as_str().unwrap()
    ));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "stage":caller.0, "target":target, "context":context,
            "page":caller.2, "submitted":caller.1, "previousPause":old
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

pub(super) async fn read(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .request(
            "tools/call",
            json!({
                "name":"browser_send",
                "arguments":{"targetId":target,"timeoutMs":3000,"request":request}
            }),
        )
        .await;
    let Some(id) = submitted["result"]["structuredContent"]["data"]["operation"]["id"].as_str()
    else {
        return json!({"submitted":submitted});
    };
    let settled = mcp
        .request(
            "tools/call",
            json!({
                "name":"browser_status",
                "arguments":{"targetId":target,"operationId":id,"waitMs":3000}
            }),
        )
        .await;
    json!({"submitted":submitted,"settled":settled})
}

pub(super) async fn capture(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    path: &std::path::Path,
    status: &Value,
    caller: (&str, &Value, &Value),
) {
    let mut evidence: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    evidence["expiredPauseStatus"] = status.clone();
    // Save the assertion boundary before any additional observation can fail.
    std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    evidence["callerAndInput"] = mcp
        .request(
            "tools/call",
            json!({
                "name":"browser_status",
                "arguments":{"targetId":target,"operationId":caller.1["operation"]["id"]}
            }),
        )
        .await;
    evidence["calls"] = read(
        mcp,
        target,
        json!({
            "operation":"context_debugger","context":context,
            "action":{"kind":"calls","limit":32,"includeDetails":true}
        }),
    )
    .await;
    evidence["renderer"] = read(
        mcp,
        target,
        json!({
            "operation":"page_read","page":caller.2,
            "locator":{"kind":"css","selector":"#Acme-private-step"},
            "read":"state","maxBytes":4096
        }),
    )
    .await;
    evidence["readinessObservation"] = readiness(mcp, target, caller.2).await;
    std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    eprintln!("Automation-pause failure evidence: {}", path.display());
}

pub(super) async fn readiness(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    let request = json!({"operation":"page_read","page":page,
        "locator":{"kind":"css","selector":"#Acme-readiness-observation"},
        "read":"state","maxBytes":4096});
    let first = read(mcp, target, request.clone()).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let second = read(mcp, target, request).await;
    json!({"first":first,"second":second,"fixtureOnly":true,
        "scope":"page-side-frame-and-attribute-observations-not-native-input"})
}
