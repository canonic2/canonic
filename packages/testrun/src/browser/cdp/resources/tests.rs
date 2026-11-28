//! Native ownership and document-lifetime fixtures; no browser or host I/O.

use super::*;
mod initial_popup;

fn target(native: &str, context: &str, kind: &str) -> Value {
    json!({"targetId":native,"browserContextId":context,"type":kind,"title":"Acme","url":"https://example.com/private","attached":false,"canAccessOpener":false})
}

fn event(
    index: &mut Index,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<(), ResourceError> {
    index.apply(Event {
        method: method.into(),
        session_id: session.map(str::to_owned),
        params,
    })
}

fn attach(index: &mut Index, parent: Option<&str>, info: Value, session: &str) {
    event(
        index,
        parent,
        "Target.attachedToTarget",
        json!({"sessionId":session,"targetInfo":info,"waitingForDebugger":false}),
    )
    .unwrap();
}

fn page() -> (Index, Reference, Reference) {
    let mut index = Index::new(Uuid::new_v4());
    let context = index.register_context("native-acme-context").unwrap();
    attach(
        &mut index,
        None,
        target("native-page", "native-acme-context", "page"),
        "page-session",
    );
    let id = index.targets["native-page"];
    let page = index.nodes[&id].reference;
    (index, context, page)
}

#[test]
fn frame_relationships_context_lookup_crosses_pages_without_borrowing_foreign_or_ambiguous_frames()
{
    let (mut index, context, page) = page();
    navigate(&mut index, "page-session", "root", "loader", None);
    let original = index.page_frame(page, None).unwrap();
    attach(
        &mut index,
        None,
        target("other-page", "native-acme-context", "page"),
        "other-session",
    );
    navigate(
        &mut index,
        "other-session",
        "other-root",
        "other-loader",
        None,
    );
    let other = index.event_frame("other-session", "other-root").unwrap();
    assert_eq!(index.context_frame(context, "other-root"), Some(other));
    assert_eq!(index.event_frame("page-session", "other-root"), None);
    let foreign = index.register_context("foreign-context").unwrap();
    attach(
        &mut index,
        None,
        target("foreign-page", "foreign-context", "page"),
        "foreign-session",
    );
    navigate(
        &mut index,
        "foreign-session",
        "foreign-root",
        "foreign-loader",
        None,
    );
    assert_eq!(index.context_frame(context, "other-root"), Some(other));
    assert!(index.context_frame(foreign, "foreign-root").is_some());
    assert_eq!(index.context_frame(context, "foreign-root"), None);
    assert_eq!(index.context_frame(foreign, "root"), None);
    assert_eq!(index.event_frame("page-session", "root"), Some(original));
    assert_eq!(
        event(
            &mut index,
            Some("other-session"),
            "Page.frameAttached",
            json!({"frameId":"root","parentFrameId":"other-root"})
        ),
        Err(ResourceError::Scope),
        "Native frame IDs cannot be reused across pages"
    );
    assert_eq!(
        index.context_frame(context, "root"),
        None,
        "An invalidated index cannot publish a relationship"
    );
}

#[test]
fn page_parent_target_is_owned_metadata_and_cannot_be_reassigned() {
    let (mut index, context, page) = page();
    let mut info = target("native-page", "native-acme-context", "page");
    info["parentId"] = json!("native-acme-tab");
    event(
        &mut index,
        None,
        "Target.targetInfoChanged",
        json!({"targetInfo":info}),
    )
    .unwrap();
    assert_eq!(
        index.page_parent_target(page).unwrap(),
        Some("native-acme-tab".into())
    );
    assert_eq!(index.page_parent_target(context), Err(ResourceError::Scope));
    info["parentId"] = json!("other-tab");
    assert_eq!(
        event(
            &mut index,
            None,
            "Target.targetInfoChanged",
            json!({"targetInfo":info})
        ),
        Err(ResourceError::Scope)
    );
}

#[test]
fn context_retirement_sessions_include_closing_descendants_without_borrowing_siblings() {
    let (mut index, context, page) = page();
    let sibling = index.register_context("native-sibling-context").unwrap();
    attach(
        &mut index,
        None,
        target("native-sibling-page", "native-sibling-context", "page"),
        "sibling-session",
    );
    attach(
        &mut index,
        None,
        target("native-service", "native-acme-context", "service_worker"),
        "service-session",
    );
    index.mark_closing(page).unwrap();
    assert!(
        index.diagnostic_scope("page-session").is_none(),
        "Public admission must still refuse a closing page"
    );
    assert_eq!(
        index.context_sessions(context).unwrap(),
        vec!["page-session", "service-session"]
    );
    assert_eq!(
        index.context_sessions(sibling).unwrap(),
        vec!["sibling-session"]
    );
    let stale = Reference {
        generation: Uuid::new_v4(),
        ..context
    };
    assert_eq!(index.context_sessions(stale), Err(ResourceError::Stale));
    index.retire(context).unwrap();
    assert_eq!(index.context_sessions(context), Err(ResourceError::Stale));
    assert_eq!(
        index.context_sessions(sibling).unwrap(),
        vec!["sibling-session"]
    );
}

#[test]
fn page_retirement_sessions_include_owned_children_and_preserve_context_workers() {
    let (mut index, context, page) = page();
    navigate(&mut index, "page-session", "root", "root-loader", None);
    attach(
        &mut index,
        Some("page-session"),
        target("child", "native-acme-context", "iframe"),
        "child-session",
    );
    navigate(
        &mut index,
        "child-session",
        "child",
        "child-loader",
        Some("root"),
    );
    attach(
        &mut index,
        Some("page-session"),
        target("dedicated", "native-acme-context", "worker"),
        "dedicated-session",
    );
    attach(
        &mut index,
        None,
        target("sibling", "native-acme-context", "page"),
        "sibling-session",
    );
    for (kind, session) in [
        ("service_worker", "service-session"),
        ("shared_worker", "shared-session"),
    ] {
        attach(
            &mut index,
            Some("page-session"),
            target(kind, "native-acme-context", kind),
            session,
        );
    }
    let child = index.target_reference("child").unwrap();
    index.mark_closing(child).unwrap();
    assert_eq!(
        index.page_sessions(page).unwrap(),
        vec!["child-session", "dedicated-session", "page-session"]
    );
    assert!(index.page_sessions(context).is_err());
    assert_eq!(
        index.page_sessions(Reference {
            generation: Uuid::new_v4(),
            ..page
        }),
        Err(ResourceError::Stale)
    );
    assert_eq!(
        index.page_sessions(Reference {
            connection_id: Uuid::new_v4(),
            ..page
        }),
        Err(ResourceError::Foreign)
    );
    index.retire(page).unwrap();
    assert_eq!(index.page_sessions(page), Err(ResourceError::Stale));
    assert_eq!(
        index.context_sessions(context).unwrap(),
        vec!["service-session", "shared-session", "sibling-session"]
    );
}

#[test]
fn event_wait_observation_owner_survives_closing_without_authorizing_dispatch() {
    let (mut index, context, page) = page();
    index.mark_closing(page).unwrap();
    assert_eq!(index.retained_page_context(page), Some(context));
    assert!(index.route(page, Some(Kind::Page)).is_err());
    assert_eq!(
        index.retained_page_context(Reference {
            generation: Uuid::new_v4(),
            ..page
        }),
        None
    );
    index.retire(page).unwrap();
    assert_eq!(index.retained_page_context(page), None);
}

#[test]
fn shared_and_service_workers_belong_to_the_context_across_creating_pages() {
    for (native_type, expected) in [
        ("shared_worker", WorkerKind::Shared),
        ("service_worker", WorkerKind::Service),
    ] {
        let (mut index, context, first) = page();
        attach(
            &mut index,
            None,
            target("second-page", "native-acme-context", "page"),
            "second-session",
        );
        let info = target("context-worker", "native-acme-context", native_type);
        event(
            &mut index,
            Some("page-session"),
            "Target.targetCreated",
            json!({"targetInfo":info}),
        )
        .unwrap();
        event(
            &mut index,
            Some("second-session"),
            "Target.targetInfoChanged",
            json!({"targetInfo":info}),
        )
        .unwrap();
        let worker = index.target_reference("context-worker").unwrap();
        assert_eq!(
            index.worker_scope(worker).unwrap(),
            (context, None, expected)
        );
        assert!(index.context_worker(worker));
        index.register_attachment(worker, "worker-session").unwrap();
        let world = world(&mut index, "worker-session", None, 1, "owned-worker-world");
        index.retire(first).unwrap();
        assert!(index.route(world, Some(Kind::World)).is_ok());
        assert_eq!(
            index.worker_scope(worker).unwrap(),
            (context, None, expected)
        );
        index.retire(context).unwrap();
        assert!(index.route(world, None).is_err());
    }
}

fn navigate(index: &mut Index, session: &str, frame: &str, loader: &str, parent: Option<&str>) {
    let mut value = json!({"id":frame,"loaderId":loader,"url":"https://example.com/private","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"Isolated","gatedAPIFeatures":[]});
    if let Some(parent) = parent {
        value["parentId"] = json!(parent);
    }
    event(
        index,
        Some(session),
        "Page.frameNavigated",
        json!({"frame":value,"type":"Navigation"}),
    )
    .unwrap();
}

fn world(
    index: &mut Index,
    session: &str,
    frame: Option<&str>,
    id: i64,
    unique: &str,
) -> Reference {
    let mut context =
        json!({"id":id,"uniqueId":unique,"origin":"https://example.com","name":"Acme world"});
    if let Some(frame) = frame {
        context["auxData"] = json!({"frameId":frame,"isDefault":true});
    }
    event(
        index,
        Some(session),
        "Runtime.executionContextCreated",
        json!({"context":context}),
    )
    .unwrap();
    index.nodes[&index.worlds[&(session.into(), id)]].reference
}

#[test]
fn popup_initial_commit_preserves_only_matching_realms_with_fresh_authority() {
    for origin in ["https://example.com", "https://other.example.com", "null"] {
        let (mut index, _, _) = page();
        let value = json!({"id":"main","loaderId":"initial","url":"","domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"InsecureScheme","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]});
        event(
            &mut index,
            Some("page-session"),
            "Page.frameNavigated",
            json!({"frame":value,"type":"Navigation"}),
        )
        .unwrap();
        let old = world(&mut index, "page-session", Some("main"), 1, "popup-world");
        index.nodes.get_mut(&old.id).unwrap().origin = Some(origin.into());
        navigate(&mut index, "page-session", "main", "committed", None);
        assert!(index.route(old, Some(Kind::World)).is_err());
        if origin == "https://example.com" {
            let fresh = index.nodes[&old.id].reference;
            assert_ne!(fresh, old);
            assert_eq!(
                index
                    .route(fresh, Some(Kind::World))
                    .unwrap()
                    .execution_unique_id
                    .as_deref(),
                Some("popup-world")
            );
            navigate(&mut index, "page-session", "main", "later", None);
            assert!(index.route(fresh, Some(Kind::World)).is_err());
        } else {
            assert!(!index.nodes.contains_key(&old.id));
        }
    }
}

#[test]
fn explicit_content_replacement_fences_frames_and_worlds_without_reusing_authority() {
    let (mut index, _, page) = page();
    navigate(&mut index, "page-session", "root", "loader", None);
    let frame = index.page_frame(page, None).unwrap();
    let old_world = world(
        &mut index,
        "page-session",
        Some("root"),
        7,
        "isolated-world",
    );
    navigate(
        &mut index,
        "page-session",
        "child",
        "child-loader",
        Some("root"),
    );
    let child = index.event_frame("page-session", "child").unwrap();
    let next = index.content_replaced(frame).unwrap();
    assert_ne!(frame, next);
    assert_eq!(
        index.page_frame(page, Some(frame)).unwrap_err(),
        ResourceError::Stale
    );
    assert_eq!(
        index.route(old_world, Some(Kind::World)).unwrap_err(),
        ResourceError::Stale
    );
    assert_eq!(
        index.page_frame(page, Some(child)).unwrap_err(),
        ResourceError::Stale
    );
    let current_world = index.world(next, "page-session", 7).unwrap();
    assert_ne!(old_world, current_world);
    assert_eq!(
        index
            .route(current_world, Some(Kind::World))
            .unwrap()
            .execution_unique_id
            .as_deref(),
        Some("isolated-world")
    );
    let mut foreign = page;
    foreign.connection_id = Uuid::new_v4();
    assert_eq!(
        index.page_frame(foreign, None).unwrap_err(),
        ResourceError::Foreign
    );
    assert_eq!(
        index.world(next, "another-session", 7).unwrap_err(),
        ResourceError::Scope
    );
}

#[test]
fn setup_refreshes_only_the_retained_frame_attachment_generation() {
    let (mut index, _, _) = page();
    navigate(&mut index, "page-session", "root", "root-loader", None);
    attach(
        &mut index,
        Some("page-session"),
        target("child", "native-acme-context", "iframe"),
        "child-session",
    );
    let old = index.target_reference("child").unwrap();
    navigate(
        &mut index,
        "child-session",
        "child",
        "child-loader",
        Some("root"),
    );
    let current = index.target_reference("child").unwrap();
    assert_ne!(old.generation, current.generation);
    assert_eq!(
        index.route(old, Some(Kind::Frame)).unwrap_err(),
        ResourceError::Stale
    );
    assert_eq!(
        index
            .attachment_current(old, Kind::Frame, "child-session")
            .unwrap(),
        current
    );
    assert_eq!(
        index
            .attachment_current(old, Kind::Frame, "page-session")
            .unwrap_err(),
        ResourceError::Scope
    );
    index.retire(current).unwrap();
    assert_eq!(
        index
            .attachment_current(old, Kind::Frame, "child-session")
            .unwrap_err(),
        ResourceError::Stale
    );
    index.lose();
    assert_eq!(
        index
            .attachment_current(old, Kind::Frame, "wrong-session")
            .unwrap_err(),
        ResourceError::Lost
    );
}

#[test]
fn worker_world_auxiliary_frame_metadata_does_not_acquire_page_scope() {
    let (mut index, _, _) = page();
    attach(
        &mut index,
        None,
        target("worker", "native-acme-context", "worker"),
        "worker-session",
    );
    let worker = index.target_reference("worker").unwrap();
    let context = world(
        &mut index,
        "worker-session",
        Some("unowned-frame-metadata"),
        1,
        "worker-world",
    );
    assert_eq!(index.nodes[&context.id].parent, Some(worker));
    assert!(
        index
            .route(context, Some(Kind::World))
            .unwrap()
            .frame
            .is_none()
    );
    assert!(index.list(Kind::Frame).is_empty());
}

#[test]
fn discovery_and_foreign_attachments_never_acquire_authority() {
    let mut index = Index::new(Uuid::new_v4());
    event(
        &mut index,
        None,
        "Target.targetCreated",
        json!({"targetInfo":target("foreign", "unowned-context", "page")}),
    )
    .unwrap();
    attach(
        &mut index,
        None,
        target("foreign", "unowned-context", "page"),
        "foreign-session",
    );
    event(
        &mut index,
        Some("foreign-session"),
        "Runtime.executionContextsCleared",
        json!({}),
    )
    .unwrap();
    assert!(index.nodes.is_empty());
    assert!(index.sessions.is_empty());
}

#[test]
fn native_sessions_route_only_checked_references_and_status_omits_private_ids() {
    let (index, _, page) = page();
    assert_eq!(
        index.route(page, Some(Kind::Page)).unwrap().session_id,
        "page-session"
    );
    let mut foreign = page;
    foreign.connection_id = Uuid::new_v4();
    assert_eq!(
        index.route(foreign, None).unwrap_err(),
        ResourceError::Foreign
    );
    assert_eq!(
        index.route(page, Some(Kind::Worker)).unwrap_err(),
        ResourceError::Scope
    );
    let status = index.snapshot().to_string();
    assert!(!status.contains("native-acme"));
    assert!(!status.contains("page-session"));
    assert!(!status.contains("example.com"));
}

#[test]
fn navigation_invalidates_worlds_and_frame_generations_but_preserves_page_identity() {
    let (mut index, _, page) = page();
    navigate(&mut index, "page-session", "main", "loader-1", None);
    let frame = index.nodes[&index.frames[&(page.id, "main".into())]].reference;
    let old_world = world(&mut index, "page-session", Some("main"), 1, "world-1");
    navigate(&mut index, "page-session", "main", "loader-1", None);
    assert!(index.route(old_world, None).is_ok());
    navigate(&mut index, "page-session", "main", "loader-2", None);
    assert_eq!(
        index.route(old_world, None).unwrap_err(),
        ResourceError::Stale
    );
    assert_eq!(index.route(frame, None).unwrap_err(), ResourceError::Stale);
    assert!(index.route(page, None).is_ok());
    let fresh = world(&mut index, "page-session", Some("main"), 1, "world-2");
    assert_ne!(fresh, old_world);
    assert_eq!(
        index
            .route(fresh, None)
            .unwrap()
            .execution_unique_id
            .as_deref(),
        Some("world-2")
    );
}

#[test]
fn out_of_process_frame_swap_changes_routing_and_retires_its_worlds() {
    let (mut index, _, page) = page();
    navigate(&mut index, "page-session", "main", "loader-main", None);
    event(
        &mut index,
        Some("page-session"),
        "Page.frameAttached",
        json!({"frameId":"child","parentFrameId":"main"}),
    )
    .unwrap();
    attach(
        &mut index,
        Some("page-session"),
        target("child", "native-acme-context", "iframe"),
        "iframe-old",
    );
    navigate(&mut index, "iframe-old", "child", "loader-child", None);
    let old = world(&mut index, "iframe-old", Some("child"), 1, "child-world");
    let old_frame = index.nodes[&index.frames[&(page.id, "child".into())]].reference;
    assert_eq!(
        index.route(old_frame, None).unwrap().session_id,
        "iframe-old"
    );
    event(
        &mut index,
        Some("iframe-old"),
        "Page.frameDetached",
        json!({"frameId":"child","reason":"swap"}),
    )
    .unwrap();
    assert_eq!(index.route(old, None).unwrap_err(), ResourceError::Stale);
    assert_eq!(
        index.route(old_frame, None).unwrap_err(),
        ResourceError::Stale
    );
    attach(
        &mut index,
        Some("page-session"),
        target("child", "native-acme-context", "iframe"),
        "iframe-new",
    );
    let fresh = index.nodes[&index.frames[&(page.id, "child".into())]].reference;
    assert_eq!(index.route(fresh, None).unwrap().session_id, "iframe-new");
}

#[test]
fn child_attachment_before_parent_swap_preserves_the_new_renderer_world() {
    let (mut index, _, page) = page();
    navigate(&mut index, "page-session", "root", "root-loader", None);
    navigate(
        &mut index,
        "page-session",
        "child",
        "old-loader",
        Some("root"),
    );
    let old = world(&mut index, "page-session", Some("child"), 1, "old-world");
    attach(
        &mut index,
        Some("page-session"),
        target("child", "native-acme-context", "iframe"),
        "child-session",
    );
    let fresh = world(&mut index, "child-session", Some("child"), 1, "new-world");
    event(
        &mut index,
        Some("page-session"),
        "Page.frameDetached",
        json!({"frameId":"child","reason":"swap"}),
    )
    .unwrap();
    assert_eq!(index.route(old, None).unwrap_err(), ResourceError::Stale);
    assert_eq!(
        index.route(fresh, None).unwrap().session_id,
        "child-session"
    );
    assert!(index.route(page, Some(Kind::Page)).is_ok());
}

#[test]
fn detached_renderer_suspends_the_frame_until_a_new_document_witness() {
    let (mut index, _, page) = page();
    navigate(&mut index, "page-session", "root", "root-loader", None);
    attach(
        &mut index,
        Some("page-session"),
        target("child", "native-acme-context", "iframe"),
        "child-session",
    );
    navigate(
        &mut index,
        "child-session",
        "child",
        "child-loader",
        Some("root"),
    );
    let old = index.target_reference("child").unwrap();
    let old_world = world(&mut index, "child-session", Some("child"), 1, "old-world");
    event(
        &mut index,
        Some("page-session"),
        "Target.detachedFromTarget",
        json!({"sessionId":"child-session","targetId":"child"}),
    )
    .unwrap();
    let suspended = index.target_reference("child").unwrap();
    assert_eq!(
        index.route(suspended, None).unwrap_err(),
        ResourceError::Stale
    );
    event(
        &mut index,
        None,
        "Target.targetInfoChanged",
        json!({"targetInfo":target("child", "native-acme-context", "iframe")}),
    )
    .unwrap();
    navigate(
        &mut index,
        "page-session",
        "child",
        "in-process-loader",
        Some("root"),
    );
    let current = index.target_reference("child").unwrap();
    assert_eq!(index.route(old, None).unwrap_err(), ResourceError::Stale);
    assert_eq!(
        index.route(old_world, None).unwrap_err(),
        ResourceError::Stale
    );
    assert_eq!(
        index.route(current, None).unwrap().session_id,
        "page-session"
    );
    assert!(index.route(page, Some(Kind::Page)).is_ok());
    event(
        &mut index,
        Some("page-session"),
        "Page.frameDetached",
        json!({"frameId":"child","reason":"remove"}),
    )
    .unwrap();
    assert_eq!(
        index.route(current, None).unwrap_err(),
        ResourceError::Stale
    );
}

#[test]
fn equal_numeric_world_ids_in_distinct_sessions_remain_isolated() {
    let (mut index, _, _) = page();
    attach(
        &mut index,
        None,
        target("other-page", "native-acme-context", "page"),
        "other-session",
    );
    let first = world(&mut index, "page-session", None, 1, "first-unique");
    let second = world(&mut index, "other-session", None, 1, "second-unique");
    event(
        &mut index,
        Some("page-session"),
        "Runtime.executionContextDestroyed",
        json!({"executionContextId":1,"executionContextUniqueId":"first-unique"}),
    )
    .unwrap();
    assert_eq!(index.route(first, None).unwrap_err(), ResourceError::Stale);
    assert_eq!(
        index.route(second, None).unwrap().session_id,
        "other-session"
    );
}

#[test]
fn workers_remain_workers_and_context_close_preserves_other_contexts() {
    let (mut index, context, _) = page();
    attach(
        &mut index,
        Some("page-session"),
        target("worker", "native-acme-context", "worker"),
        "worker-session",
    );
    let worker = index.nodes[&index.targets["worker"]].reference;
    let worker_world = world(&mut index, "worker-session", None, 1, "worker-unique");
    let other = index.register_context("other-context").unwrap();
    attach(
        &mut index,
        None,
        target("survivor", "other-context", "page"),
        "survivor-session",
    );
    let survivor = index.nodes[&index.targets["survivor"]].reference;
    assert_eq!(
        index.route(worker, Some(Kind::Page)).unwrap_err(),
        ResourceError::Scope
    );
    index.mark_closing(context).unwrap();
    attach(
        &mut index,
        Some("page-session"),
        target("late-worker", "native-acme-context", "worker"),
        "late-worker-session",
    );
    let late_worker = index.nodes[&index.targets["late-worker"]].reference;
    assert_eq!(
        index.route(late_worker, None).unwrap_err(),
        ResourceError::Stale
    );
    assert_eq!(
        index.route(worker_world, None).unwrap_err(),
        ResourceError::Stale
    );
    index.retire(context).unwrap();
    assert!(index.route(survivor, None).is_ok());
    assert!(index.resolve(other, Some(Kind::Context)).is_ok());
}

#[test]
fn closed_targets_cannot_be_resurrected_by_late_metadata() {
    let (mut index, _, page) = page();
    event(
        &mut index,
        None,
        "Target.targetDestroyed",
        json!({"targetId":"native-page"}),
    )
    .unwrap();
    assert_eq!(index.route(page, None).unwrap_err(), ResourceError::Stale);
    event(
        &mut index,
        None,
        "Target.targetInfoChanged",
        json!({"targetInfo":target("native-page", "native-acme-context", "page")}),
    )
    .unwrap();
    let survivor = index.register_context("surviving-context").unwrap();
    assert!(index.native(survivor, Kind::Context).is_ok());
    assert!(index.target_reference("native-page").is_none());
    let late = event(
        &mut index,
        None,
        "Target.targetCreated",
        json!({"targetInfo":target("native-page", "native-acme-context", "page")}),
    );
    assert_eq!(late.unwrap_err(), ResourceError::Stale);
    assert_eq!(
        index.register_context("new-context").unwrap_err(),
        ResourceError::Lost
    );
}

#[test]
fn wrong_context_or_world_identity_fences_the_epoch() {
    let (mut index, _, page) = page();
    index.register_context("other-context").unwrap();
    let moved = event(
        &mut index,
        None,
        "Target.targetInfoChanged",
        json!({"targetInfo":target("native-page", "other-context", "page")}),
    );
    assert_eq!(moved.unwrap_err(), ResourceError::Scope);
    assert_eq!(index.route(page, None).unwrap_err(), ResourceError::Lost);

    let (mut index, _, _) = self::page();
    world(&mut index, "page-session", None, 1, "unique");
    assert_eq!(
        event(
            &mut index,
            Some("page-session"),
            "Runtime.executionContextDestroyed",
            json!({"executionContextId":1,"executionContextUniqueId":"wrong"})
        )
        .unwrap_err(),
        ResourceError::Protocol
    );
}

#[test]
fn resource_and_retirement_bounds_never_forget_authority_fences() {
    let mut index = Index::new(Uuid::new_v4());
    for number in 0..MAX_RESOURCES {
        index
            .register_context(&format!("context-{number}"))
            .unwrap();
    }
    assert_eq!(
        index.register_context("overflow").unwrap_err(),
        ResourceError::Capacity
    );
    let mut index = Index::new(Uuid::new_v4());
    for number in 0..MAX_RETIRED {
        let context = index
            .register_context(&format!("context-{number}"))
            .unwrap();
        index.retire(context).unwrap();
    }
    let last = index.register_context("last").unwrap();
    assert_eq!(index.retire(last).unwrap_err(), ResourceError::Capacity);
    assert!(index.lost);
}
