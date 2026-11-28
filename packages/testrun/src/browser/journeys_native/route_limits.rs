//! A candidate is counted only when invoked, with Page/Context quota sharing.
use super::*;
use crate::browser::{
    contract::CallbackResponse,
    routing::{HandlerAction, Matcher},
};

async fn callback(session: &Session, id: &Value) -> Reference {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(item) = items
            .iter()
            .find(|item| item["kind"] == "route" && item["handlerId"] == *id)
        {
            return serde_json::from_value(item["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Limited handler callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
fn fetch(page: Reference, url: String) -> Request {
    Request::Evaluate {
        scope: page,
        expression: format!(
            "async()=>await (await fetch({})).text()",
            serde_json::json!(url)
        ),
        is_function: Some(true),
        argument: Default::default(),
        return_handle: false,
    }
}
async fn fulfill(session: &Session, callback: Reference, body: &str) {
    let value = successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::RouteFulfill {
                status: Some(200),
                body: Some(body.into()),
                json: None,
                content_type: Some("text/plain".into()),
                headers: vec![],
                path: None,
                file: None,
            },
        },
    )
    .await;
    assert!(value.get("handlerCleanupPending").is_none(), "{value}");
}
pub(super) async fn exercise(session: &Session, page: Reference, context: Reference, base: &str) {
    let pattern = format!("{base}/limited?request=*");
    let mut ids = Vec::new();
    for (scope, times) in [(page, 1), (page, 1), (context, 2)] {
        let result = successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::AddMatcher {
                    matcher: Matcher::Glob {
                        pattern: pattern.clone(),
                    },
                    times: Some(times),
                },
            },
        )
        .await;
        ids.push(result["handlerId"].clone());
    }
    let first = session
        .submit(
            Uuid::new_v4(),
            fetch(page, format!("{base}/limited?request=first")),
            30000,
        )
        .unwrap();
    fulfill(session, callback(session, &ids[1]).await, "Acme newest").await;
    let result = settled(session, first).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(result.result.unwrap()["value"]["value"], "Acme newest");
    let listed = successful(
        session,
        Request::NetworkRoute {
            scope: page,
            action: HandlerAction::List {},
        },
    )
    .await;
    assert_eq!(listed["handlers"].as_array().unwrap().len(), 1);
    assert_eq!(listed["handlers"][0]["handlerId"], ids[0]);
    assert_eq!(listed["handlers"][0]["remainingUses"], 1);

    let second = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate{scope:page,expression:format!("async()=>JSON.stringify(await Promise.all([{},{}].map(async url=>await (await fetch(url)).text())))",serde_json::json!(format!("{base}/limited?request=second")),serde_json::json!(format!("{base}/limited?request=third"))),is_function:Some(true),argument:Default::default(),return_handle:false},
            30000,
        )
        .unwrap();
    let page_callback = callback(session, &ids[0]).await;
    let context_callback = callback(session, &ids[2]).await;
    let fallback = successful(
        session,
        Request::CallbackResolve {
            callback: page_callback,
            response: CallbackResponse::RouteFallback {},
        },
    )
    .await;
    let next = serde_json::from_value(fallback["nextCallback"].clone()).unwrap();
    let listed = successful(
        session,
        Request::NetworkRoute {
            scope: context,
            action: HandlerAction::List {},
        },
    )
    .await;
    assert_eq!(listed["handlers"][0]["remainingUses"], 0);
    assert_eq!(listed["handlers"][0]["exhausted"], true);
    fulfill(session, context_callback, "Acme bounded").await;
    fulfill(session, next, "Acme bounded").await;
    let result = settled(session, second).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(
        result.result.unwrap()["value"]["value"],
        "[\"Acme bounded\",\"Acme bounded\"]"
    );
    for scope in [page, context] {
        let listed = successful(
            session,
            Request::NetworkRoute {
                scope,
                action: HandlerAction::List {},
            },
        )
        .await;
        assert!(
            listed["handlers"].as_array().unwrap().is_empty(),
            "{listed}"
        );
    }
    let passed = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: format!(
                "async()=>(await fetch({})).status",
                serde_json::json!(format!("{base}/limited?request=after"))
            ),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    assert_eq!(passed["value"]["value"], 200);
}
