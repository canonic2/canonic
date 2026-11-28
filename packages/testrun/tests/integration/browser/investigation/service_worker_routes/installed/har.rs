//! Context HAR replay and ordered fallback retain the actual worker owner.
use super::*;

fn expected(row: &Value, key: &str) -> Value {
    if row[key]["ok"] == true {
        row[key]["value"].clone()
    } else {
        json!("Acme aborted")
    }
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    site: &WorkerSite,
    reference: &Value,
) {
    for (index, name) in ["automatic", "ordered"].into_iter().enumerate() {
        let context =
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        let archived = har_replay_filter::archive(
            fixture,
            mcp,
            target,
            &[format!("{}/wire/hit", site.url)],
            "Acme archive",
        )
        .await["archiveId"]
            .clone();
        let pattern = format!("{}/wire/*", site.url);
        if engine == "chrome" {
            deliver(fixture, mcp, target, add(&page, &pattern), "mcp").await;
            if name == "automatic" {
                deliver(fixture,mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":archived,"notFound":"abort","urlFilter":{"kind":"glob","pattern":pattern}}),"cli").await;
            }
        }
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
        assert_eq!(evaluate(mcp,target,&page,"async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;}").await,true);
        if engine != "chrome" {
            let inventory = operation(
                mcp,
                target,
                json!({"operation":"context_worker_list","context":context}),
            )
            .await;
            assert!(
                inventory["workers"].as_array().unwrap().is_empty(),
                "{inventory}"
            );
            operation(
                mcp,
                target,
                json!({"operation":"context_close","context":context}),
            )
            .await;
            operation(
                mcp,
                target,
                json!({"operation":"har_remove","archiveId":archived}),
            )
            .await;
            continue;
        }
        let service = worker(mcp, target, &context, &format!("{}/acme-sw.js", site.url)).await;
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
        let mut row = json!({"name":name});
        let baseline = &reference["cases"][index];
        if name == "automatic" {
            for (key, path) in [("hit", "/wire/hit"), ("miss", "/wire/miss")] {
                row[key] = har_replay_filter::fetch(mcp, target, &service, path).await;
                assert_eq!(row[key], expected(baseline, key), "{row}");
            }
            row["filtered"] = har_replay_filter::fetch(mcp, target, &service, "/wireless").await;
            assert!(
                row["filtered"]
                    .as_str()
                    .unwrap()
                    .contains("Acme worker routes"),
                "{row}"
            );
            har_replay_filter::refuses(
                mcp,
                target,
                json!({"operation":"har_remove","archiveId":archived}),
                "HAR_ARCHIVE_IN_USE",
            )
            .await;
            operation(
                mcp,
                target,
                json!({"operation":"context_har_set","context":context,"archiveId":null}),
            )
            .await;
            deliver(fixture,mcp,target,json!({"operation":"context_har_set","context":context,"archiveId":archived,"notFound":"fallback","urlFilter":{"kind":"glob","pattern":pattern}}),"cli").await;
            for (key, path) in [("fallback", "/wire/fallback"), ("hitAgain", "/wire/hit")] {
                row[key] = har_replay_filter::fetch(mcp, target, &service, path).await;
                assert_eq!(row[key], expected(baseline, key), "{row}");
            }
            deliver(
                fixture,
                mcp,
                target,
                json!({"operation":"network_route","scope":context,"action":{"kind":"clear"}}),
                "act",
            )
            .await;
        } else {
            let oldest =
                deliver(fixture, mcp, target, add(&context, &pattern), "cli").await["handlerId"]
                    .clone();
            let added=deliver(fixture,mcp,target,json!({"operation":"network_route","scope":context,"action":{"kind":"add_har","archiveId":archived,"notFound":"fallback","urlFilter":{"kind":"glob","pattern":pattern}}}),"act").await;
            assert_eq!(added["handlers"][0]["kind"], "har");
            let newest =
                deliver(fixture, mcp, target, add(&context, &pattern), "mcp").await["handlerId"]
                    .clone();
            for (key, path) in [("hit", "/wire/hit"), ("miss", "/wire/miss")] {
                let (held, callback) =
                    pending(mcp, target, &service, path, &context, &newest, &service).await;
                har_replay_filter::refuses(
                    mcp,
                    target,
                    json!({"operation":"network_route","scope":context,"action":{"kind":"clear"}}),
                    "ROUTE_CALLBACKS_PENDING",
                )
                .await;
                deliver(fixture,mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fallback"}}),"cli").await;
                if key == "miss" {
                    let callback = har_interleave::callback(mcp, target, &context, &oldest).await;
                    let status = mcp.detailed_browser_status(target).await;
                    let owner = status["browser"]["callbacks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| item["reference"] == callback)
                        .unwrap();
                    assert_eq!(owner["worker"], service);
                    assert!(owner["page"].is_null());
                    fulfill(fixture, mcp, target, &callback, "Acme oldest", "act").await;
                }
                row[key] = completed(mcp, target, held).await["value"]["value"].clone();
                assert_eq!(row[key], expected(baseline, key), "{row}");
            }
            har_replay_filter::refuses(
                mcp,
                target,
                json!({"operation":"har_remove","archiveId":archived}),
                "HAR_ARCHIVE_IN_USE",
            )
            .await;
            deliver(
                fixture,
                mcp,
                target,
                json!({"operation":"network_route","scope":context,"action":{"kind":"clear"}}),
                "act",
            )
            .await;
        }
        row["afterRemoval"] =
            har_replay_filter::fetch(mcp, target, &service, "/wire/removed").await;
        assert_eq!(row["afterRemoval"], expected(baseline, "afterRemoval"));
        let status = mcp.detailed_browser_status(target).await;
        assert_eq!(status["browser"]["callbacks"], json!([]), "{status}");
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":archived}),
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
        std::fs::write(fixture.workspace.path().join(format!("worker-har-installed-{engine}-{mode}-{name}.json")),serde_json::to_vec_pretty(&json!({"actual":row,"pinnedSdk":baseline,"wire":site.wire.lock().unwrap().clone()})).unwrap()).unwrap();
    }
    assert_eq!(json!(site.wire.lock().unwrap().clone()), reference["wire"]);
}

#[tokio::test]
async fn installed_step_four_service_worker_har_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let reference = crate::browser::investigation::service_worker_har::reference(
                &fixture, engine, mode,
            )
            .await;
            let site = WorkerSite::start().await;
            let doctor = mcp.tool("browser_doctor", json!({"engine":engine})).await;
            assert_eq!(
                doctor["capabilities"]["routing"]["serviceWorkers"],
                engine == "chrome"
            );
            if engine == "chrome" {
                assert_eq!(doctor["capabilities"]["routing"]["serviceWorkerHar"], true);
                assert_eq!(
                    doctor["capabilities"]["routing"]["serviceWorkerAuthentication"],
                    true
                );
            }
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, engine, mode, &site, &reference,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("worker-har-failure-{engine}-{mode}.json")),
                    serde_json::to_vec_pretty(
                        &json!({"status":status,"wire":site.wire.lock().unwrap().clone()}),
                    )
                    .unwrap(),
                )
                .unwrap();
            }
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(closed);
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Installed worker HAR {engine}/{mode}: replay, ordered fallback and original cleanup"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
