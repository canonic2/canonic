//! Initial opaque popups keep only a witnessed live same-context opener realm.
use super::*;

fn document(frame: &str, loader: &str, url: &str) -> Value {
    json!({"id":frame,"loaderId":loader,"url":url,"domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"InsecureScheme","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]})
}
fn opaque_world(
    index: &mut Index,
    session: &str,
    frame: &str,
    id: i64,
    unique: &str,
    default: bool,
    origin: &str,
) -> Reference {
    event(index,Some(session),"Runtime.executionContextCreated",json!({"context":{"id":id,"uniqueId":unique,"origin":origin,"name":"Acme","auxData":{"frameId":frame,"isDefault":default}}})).unwrap();
    index.nodes[&index.worlds[&(session.into(), id)]].reference
}
fn setup(case: &str) -> (Index, Reference, Reference, Reference, Reference) {
    let (mut index, _, opener) = page();
    index
        .seed_frame_tree(
            "page-session",
            &json!({"frameTree":{"frame":document("opener-root","opener-initial","about:blank")}}),
        )
        .unwrap();
    let source = opaque_world(
        &mut index,
        "page-session",
        "opener-root",
        1,
        "Acme-opener-realm",
        true,
        if case == "http-opener" {
            "https://example.com"
        } else {
            "://"
        },
    );
    let mut info = target("native-popup", "native-acme-context", "page");
    info["url"] = json!("");
    info["openerId"] = json!(if case == "unknown-opener" {
        "unknown"
    } else {
        "native-page"
    });
    info["openerFrameId"] = json!("opener-root");
    info["canAccessOpener"] = json!(case != "inaccessible-opener");
    if case == "foreign-opener" {
        index.register_context("foreign-context").unwrap();
        attach(
            &mut index,
            None,
            target("foreign-page", "foreign-context", "page"),
            "foreign-session",
        );
        index.seed_frame_tree("foreign-session",&json!({"frameTree":{"frame":document("foreign-root","foreign-initial","about:blank")}})).unwrap();
        opaque_world(
            &mut index,
            "foreign-session",
            "foreign-root",
            1,
            "Acme-foreign-realm",
            true,
            "://",
        );
        info["openerId"] = json!("foreign-page");
        info["openerFrameId"] = json!("foreign-root");
    }
    attach(&mut index, None, info, "popup-session");
    let popup = index.target_reference("native-popup").unwrap();
    index
        .seed_frame_tree(
            "popup-session",
            &json!({"frameTree":{"frame":document("native-popup","initial","")}}),
        )
        .unwrap();
    let main = opaque_world(
        &mut index,
        "popup-session",
        "native-popup",
        5,
        "Acme-popup-main",
        true,
        "://",
    );
    let isolated = opaque_world(
        &mut index,
        "popup-session",
        "native-popup",
        6,
        "Acme-popup-isolated",
        false,
        "://",
    );
    match case {
        "retired-opener" | "replaced-opener" => {
            index.retire(source).unwrap();
            if case == "replaced-opener" {
                opaque_world(
                    &mut index,
                    "page-session",
                    "opener-root",
                    2,
                    "Acme-new-opener-realm",
                    true,
                    "://",
                );
            }
        }
        "closing-opener" => index.mark_closing(opener).unwrap(),
        _ => {}
    }
    (index, popup, source, main, isolated)
}
fn commit(index: &mut Index, url: &str, loader: &str) {
    event(
        index,
        Some("popup-session"),
        "Page.frameNavigated",
        json!({"frame":document("native-popup",loader,url),"type":"Navigation"}),
    )
    .unwrap();
}

#[test]
fn initial_popup_opaque_commit_preserves_native_realms_with_fresh_document_authority() {
    let (mut index, popup, _, main, isolated) = setup("owned-opener");
    let old_frame = index.page_frame(popup, None).unwrap();
    commit(&mut index, "about:blank", "committed");
    let frame = index.page_frame(popup, None).unwrap();
    assert_ne!(frame, old_frame);
    assert!(index.route(old_frame, Some(Kind::Frame)).is_err());
    for (old, numeric, native) in [
        (main, 5, "Acme-popup-main"),
        (isolated, 6, "Acme-popup-isolated"),
    ] {
        assert!(index.route(old, Some(Kind::World)).is_err());
        let fresh = index
            .world(frame, "popup-session", numeric)
            .expect("The live initial popup realm must remain indexed");
        assert_ne!(fresh, old);
        assert_eq!(
            index
                .route(fresh, Some(Kind::World))
                .unwrap()
                .execution_unique_id
                .as_deref(),
            Some(native)
        );
    }
    assert_eq!(index.execution_world(popup).unwrap().0, frame);
}

#[test]
fn initial_popup_opaque_continuation_refuses_missing_foreign_retired_or_inaccessible_opener_proofs()
{
    for case in [
        "unknown-opener",
        "foreign-opener",
        "retired-opener",
        "replaced-opener",
        "closing-opener",
        "http-opener",
        "inaccessible-opener",
        "nonblank-commit",
    ] {
        let (mut index, _, _, main, isolated) = setup(case);
        commit(
            &mut index,
            if case == "nonblank-commit" {
                "about:srcdoc"
            } else {
                "about:blank"
            },
            "committed",
        );
        assert!(!index.nodes.contains_key(&main.id), "{case}");
        assert!(!index.nodes.contains_key(&isolated.id), "{case}");
    }
}

#[test]
fn initial_popup_opaque_proof_is_consumed_and_actual_world_destruction_still_fences_execution() {
    for destroy in [false, true] {
        let (mut index, popup, _, main, _) = setup("owned-opener");
        commit(&mut index, "about:blank", "committed");
        let frame = index.page_frame(popup, None).unwrap();
        let fresh = index
            .world(frame, "popup-session", 5)
            .expect("First initial commit keeps the witnessed realm");
        assert_eq!(fresh.id, main.id);
        if destroy {
            event(
                &mut index,
                Some("popup-session"),
                "Runtime.executionContextDestroyed",
                json!({"executionContextId":5,"executionContextUniqueId":"Acme-popup-main"}),
            )
            .unwrap();
        } else {
            commit(&mut index, "about:blank", "second");
        }
        assert!(index.route(fresh, Some(Kind::World)).is_err());
        assert!(index.execution_world(popup).is_err());
    }
}
