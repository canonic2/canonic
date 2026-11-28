//! Native input targets the owned page session without raising its window.
use super::investigation::owned_page;
use super::navigation::{isolated_frame_scope, isolated_scope};
use super::*;

async fn reply_to(peer: &mut Peer, method: &str, result: Value) {
    let request = command(peer, method).await;
    reply(peer, &request, result).await;
}

#[tokio::test]
async fn locator_fill_inserts_text_without_raising_the_window() {
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let request = serde_json::from_value(json!({
        "operation":"page_action","page":page,"locator":{"kind":"css","selector":"#name"},
        "action":{"kind":"fill","value":"Acme"}
    }))
    .unwrap();
    let action = tokio::spawn(async move {
        worker
            .execute_one(Uuid::new_v4(), request, deadline())
            .await
            .expect("locator fill")
    });

    isolated_scope(&mut peer, true).await;
    reply_to(
        &mut peer,
        "Runtime.evaluate",
        json!({"result":{"type":"object","subtype":"node","objectId":"Acme-field"}}),
    )
    .await;
    reply_to(
        &mut peer,
        "Runtime.callFunctionOn",
        json!({"result":{"type":"object","value":{"ready":true}}}),
    )
    .await;
    reply_to(
        &mut peer,
        "Runtime.callFunctionOn",
        json!({"result":{"type":"object","value":{"needsInput":true}}}),
    )
    .await;
    let insert = command(&mut peer, "Input.insertText").await;
    assert_eq!(insert["sessionId"], "session-acme");
    assert_eq!(insert["params"]["text"], "Acme");
    reply(&mut peer, &insert, json!({})).await;
    reply_to(
        &mut peer,
        "Runtime.callFunctionOn",
        json!({"result":{"type":"object","value":{"done":true}}}),
    )
    .await;
    reply_to(
        &mut peer,
        "Runtime.evaluate",
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    reply_to(&mut peer, "Runtime.releaseObjectGroup", json!({})).await;

    let result = action.await.unwrap();
    assert_eq!(result["dispatchCompleted"], true);
}

#[tokio::test]
async fn coordinate_click_dispatches_without_raising_the_window() {
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let frame = worker.index.page_frame(page, None).unwrap();
    let geometry = json!({"viewport":{"width":1280,"height":720},"visualViewport":{"scale":1}});
    let observation =
        worker
            .inputs
            .observe(page, frame, geometry.clone(), std::time::Instant::now(), 0);
    let request = serde_json::from_value(json!({
        "operation":"page_input","page":page,"observation":observation,
        "actions":[{"kind":"mouse_click","x":40,"y":60,"button":"left"}]
    }))
    .unwrap();
    let action = tokio::spawn(async move {
        worker
            .execute_one(Uuid::new_v4(), request, deadline())
            .await
            .expect("coordinate click")
    });

    isolated_scope(&mut peer, true).await;
    reply_to(
        &mut peer,
        "Runtime.evaluate",
        json!({"result":{"type":"object","value":geometry}}),
    )
    .await;
    for phase in ["mouseMoved", "mousePressed", "mouseReleased"] {
        let mouse = command(&mut peer, "Input.dispatchMouseEvent").await;
        assert_eq!(mouse["sessionId"], "session-acme");
        assert_eq!(mouse["params"]["type"], phase);
        assert_eq!(mouse["params"]["x"].as_f64(), Some(40.));
        assert_eq!(mouse["params"]["y"].as_f64(), Some(60.));
        reply(&mut peer, &mouse, json!({})).await;
    }

    let result = action.await.unwrap();
    assert_eq!(result["completedActions"], 1);
}

#[tokio::test]
async fn locator_click_reports_navigation_of_the_explicit_frame() {
    expect_frame_after_click("frame-acme", false).await;
    expect_frame_after_click("frame-child", false).await;
    expect_frame_after_click("frame-child", true).await;
}

async fn expect_frame_after_click(native_frame: &str, detached: bool) {
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let iframe = native_frame == "frame-child";
    if iframe {
        worker
            .apply(EventItem::Event(Event {
                method: "Page.frameAttached".into(),
                session_id: Some("session-acme".into()),
                params: json!({"frameId":native_frame,"parentFrameId":"frame-acme"}),
            }))
            .unwrap();
    }
    let frame = worker
        .index
        .event_frame("session-acme", native_frame)
        .unwrap();
    let request = serde_json::from_value(json!({
        "operation":"page_action","page":page,"frame":frame,"force":iframe,
        "locator":{"kind":"css","selector":"#continue"},"action":{"kind":"click"}
    }))
    .unwrap();
    let action = tokio::spawn(async move {
        worker
            .execute_one(Uuid::new_v4(), request, deadline())
            .await
            .unwrap()
    });

    isolated_frame_scope(&mut peer, native_frame, true).await;
    reply_to(
        &mut peer,
        "Runtime.evaluate",
        json!({"result":{"type":"object","subtype":"node","objectId":"Acme-link"}}),
    )
    .await;
    reply_to(&mut peer, "DOM.scrollIntoViewIfNeeded", json!({})).await;
    reply_to(
        &mut peer,
        "Runtime.callFunctionOn",
        json!({"result":{"type":"object","value":{"pointer":true,"x":40,"y":60}}}),
    )
    .await;
    reply_to(
        &mut peer,
        "DOM.getContentQuads",
        json!({"quads":[[30,50,50,50,50,70,30,70]]}),
    )
    .await;
    let layout = json!({"pageX":0,"pageY":0,"clientWidth":1280,"clientHeight":720});
    let visual = json!({"offsetX":0,"offsetY":0,"pageX":0,"pageY":0,"clientWidth":1280,
        "clientHeight":720,"scale":1});
    let content = json!({"x":0,"y":0,"width":1280,"height":720});
    reply_to(
        &mut peer,
        "Page.getLayoutMetrics",
        json!({
            "layoutViewport":layout,"visualViewport":visual,"contentSize":content,
            "cssLayoutViewport":layout,"cssVisualViewport":visual,"cssContentSize":content,
        }),
    )
    .await;
    for phase in ["mouseMoved", "mousePressed", "mouseReleased"] {
        let mouse = command(&mut peer, "Input.dispatchMouseEvent").await;
        assert_eq!(mouse["sessionId"], "session-acme");
        assert_eq!(mouse["params"]["type"], phase);
        if phase == "mouseReleased" {
            let event = if detached {
                json!({"method":"Page.frameDetached","sessionId":"session-acme",
                    "params":{"frameId":native_frame,"reason":"remove"}})
            } else {
                let mut document = json!({"id":native_frame,"loaderId":"loader-next",
                    "url":"https://example.com/next","domainAndRegistry":"example.com",
                    "securityOrigin":"https://example.com","mimeType":"text/html",
                    "secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated",
                    "gatedAPIFeatures":[]});
                if iframe {
                    document["parentId"] = json!("frame-acme");
                }
                json!({"method":"Page.frameNavigated","sessionId":"session-acme",
                    "params":{"type":"Navigation","frame":document}})
            };
            send(&mut peer, event).await;
        }
        reply(&mut peer, &mouse, json!({})).await;
    }

    let result = action.await.unwrap();
    assert_eq!(result["dispatchCompleted"], true);
    if detached {
        assert_eq!(result.get("frame"), Some(&Value::Null));
        assert_eq!(result.get("documentReplaced"), Some(&Value::Null));
        return;
    }
    assert_eq!(result["documentReplaced"], true);
    let current: Reference = serde_json::from_value(result["frame"].clone()).unwrap();
    assert_eq!(current.id, frame.id);
    assert_ne!(current.generation, frame.generation);
}
