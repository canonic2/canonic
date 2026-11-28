//! Scope-wide HAR and manual cleanup, with matcher refusals before allocation.
use super::har_replay_filter::{archive, fetch, refuses};
use super::*;

pub(super) async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    site: &Site,
    target: &str,
    context: &Value,
) {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let url = format!("{}{prefix}/clear", site.url);
    let archived = archive(
        fixture,
        mcp,
        target,
        std::slice::from_ref(&url),
        "Acme archived",
    )
    .await;
    for (pattern, flags) in [
        ("(?=Acme)", ""),
        ("(?!Acme)", ""),
        ("(?<=Acme)", ""),
        ("(?<!Acme)", ""),
        (r"(Acme)\1", ""),
        (r"(?<n>Acme)\k<n>", ""),
        ("Acme", "v"),
    ] {
        let matcher = json!({"kind":"regex","pattern":pattern,"flags":flags});
        for scope in [context, &page] {
            // HTTP routes run backtracking features on the pinned RegExp engine
            // and WebSocket routes on the page's native RegExp.
            operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":matcher}})).await;
            let removed = operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"remove_matcher","matcher":matcher}})).await;
            assert!(
                removed["handlers"].as_array().unwrap().is_empty(),
                "{removed}"
            );
            let route = operation(
                mcp,
                target,
                json!({"operation":"web_socket_route_add_matcher","scope":scope,"matcher":matcher}),
            )
            .await;
            operation(mcp,target,json!({"operation":"web_socket_route_remove","scope":scope,"routeId":route["routeId"]})).await;
        }
    }
    assert!(
        operation(mcp, target, json!({"operation":"har_list"})).await["routes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for (operation_name, field, scope) in [
        ("context_har_set", "context", context),
        ("page_har_set", "page", &page),
    ] {
        operation(
            mcp,
            target,
            json!({"operation":operation_name,(field):scope,"archiveId":archived["archiveId"]}),
        )
        .await;
    }
    assert_eq!(fetch(mcp, target, &page, &url).await, "Acme archived");
    // Fresh CLI clears Page replay; Context replay remains live and owns its archive.
    let request =
        json!({"operation":"network_route","scope":page,"action":{"kind":"clear"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
    let rules = operation(mcp, target, json!({"operation":"har_list"})).await;
    assert_eq!(rules["routes"].as_array().unwrap().len(), 1, "{rules}");
    assert_eq!(fetch(mcp, target, &page, &url).await, "Acme archived");
    refuses(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":archived["archiveId"]}),
        "HAR_ARCHIVE_IN_USE",
    )
    .await;
    // A Page handler beside Context replay must survive Context clear.
    let handler = operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"add","url":url}}),
    )
    .await["handlerId"]
        .clone();
    operation(mcp,target,json!({"operation":"network_route","scope":context,"action":{"kind":"clear","behavior":"wait"}})).await;
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>await(await fetch({})).text()",json!(url))}})).await;
    let callback = installed_handler_callback(mcp, target, &handler).await;
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":200,"body":"Acme sibling","contentType":"text/plain"}})).await;
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "Acme sibling"
    );
    operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"clear"}}),
    )
    .await;
    for (operation_name, field, scope) in [
        ("page_route_set", "page", &page),
        ("context_route_set", "context", context),
    ] {
        operation(
            mcp,
            target,
            json!({"operation":operation_name,(field):scope,"url":url}),
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"network_route","scope":scope,"action":{"kind":"clear"}}),
        )
        .await;
    }
    assert_eq!(fetch(mcp, target, &page, &url).await, "Acme network");
    operation(
        mcp,
        target,
        json!({"operation":"har_remove","archiveId":archived["archiveId"]}),
    )
    .await;
    assert_eq!(
        site.counts.lock().unwrap().get(&format!("{prefix}/clear")),
        Some(&1)
    );
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}
