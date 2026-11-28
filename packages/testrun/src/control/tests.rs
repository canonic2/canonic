//! Lease epochs, pending fencing, expiry and observer isolation.
use super::*;

#[test]
fn read_only_and_input_grant_registries_cannot_authorize_each_other() {
    let media = crate::grants::Grants::default();
    let input = crate::grants::Grants::default();
    let token = crate::grants::issue_grant(&media).unwrap();
    assert!(!crate::grants::consume_grant(&input, &token));
    assert!(crate::grants::consume_grant(&media, &token));
    assert!(!crate::grants::consume_grant(&media, &token));
}

#[tokio::test]
async fn handoff_fences_queued_work_but_waits_for_the_active_epoch() {
    let now = Instant::now();
    let authority = Authority::new("initial".into(), true);
    let active = authority.admit("initial", now).unwrap();
    let viewer = Some(Owner::Viewer("acme-viewer".into()));
    authority.plan(&active, viewer.clone(), now).unwrap();
    assert_eq!(
        authority.admit("initial", now).unwrap_err().code,
        "CONTROL_HANDOFF_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(1), authority.lost(&active))
            .await
            .is_err()
    );
    let next = authority.apply(&active, viewer, now).unwrap();
    authority.lost(&active).await;
    assert!(authority.admit(&next.id, now).is_err());
    authority.connect(&next, now).unwrap();
    assert_eq!(authority.admit(&next.id, now).unwrap(), next);
    authority.disconnect(&active);
    assert_eq!(authority.status(now)["owner"], "viewer");
    authority.disconnect(&next);
    assert_eq!(authority.status(now)["state"], "released");
    assert!(authority.admit(&next.id, now).is_err());
}

#[tokio::test]
async fn delays_end_when_a_handoff_starts_the_lease_changes_or_the_session_closes() {
    let now = Instant::now();
    let authority = Authority::new("initial".into(), true);
    let active = authority.admit("initial", now).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(1), authority.interrupted(&active))
            .await
            .is_err()
    );
    authority.plan(&active, None, now).unwrap();
    assert_eq!(
        authority.interrupted(&active).await.code,
        "CONTROL_HANDOFF_PENDING"
    );
    let next = authority.apply(&active, None, now).unwrap();
    assert_eq!(
        authority.interrupted(&active).await.code,
        "STALE_CONTROL_LEASE"
    );
    authority.close();
    assert_eq!(authority.interrupted(&next).await.code, "SESSION_CLOSING");
}

#[test]
fn viewer_heartbeat_cannot_revive_an_expired_lease_and_observers_cannot_take_over() {
    let now = Instant::now();
    let authority = Authority::new("initial".into(), true);
    let old = authority.credential(now);
    let viewer = Some(Owner::Viewer("acme-viewer".into()));
    authority.plan(&old, viewer.clone(), now).unwrap();
    let lease = authority.apply(&old, viewer, now).unwrap();
    authority.connect(&lease, now).unwrap();
    authority
        .heartbeat(&lease, now + Duration::from_secs(4))
        .unwrap();
    authority
        .admit(&lease.id, now + Duration::from_secs(8))
        .unwrap();
    assert!(
        authority
            .heartbeat(&lease, now + Duration::from_secs(10))
            .is_err()
    );
    assert_eq!(
        authority.status(now + Duration::from_secs(10))["reason"],
        "heartbeat_expired"
    );
    let observer = Authority::new("observer".into(), false);
    assert!(
        observer
            .plan(&observer.credential(now), Some(Owner::Agent), now)
            .is_err()
    );
    assert!(authority.plan(&old, Some(Owner::Agent), now).is_err());
}
