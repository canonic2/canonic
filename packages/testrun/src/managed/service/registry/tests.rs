//! Pool decisions: shared targets, temporary claims, idle release, failed
//! release retries, media limits and draining.
use super::*;

fn open(registry: &mut Registry) -> String {
    registry
        .open(|| {
            Ok(Opening {
                directory: PathBuf::from("/example/state"),
                generation: 1,
            })
        })
        .unwrap()
}

fn idle(registry: &mut Registry, target: &str) {
    registry
        .pool_mut()
        .unwrap()
        .targets
        .get_mut(target)
        .unwrap()
        .last_use = Awake::now().earlier(IDLE_RELEASE);
}

#[test]
fn one_pool_serves_every_client_and_claims_never_conflict() {
    let mut registry = Registry::default();
    let id = open(&mut registry);
    // Opening again returns the same pool: there is nothing per client.
    assert_eq!(open(&mut registry), id);
    registry.claim("android:Acme-1").unwrap();
    registry.claim("android:Acme-1").unwrap();
    registry.claim("device:Acme-2").unwrap();
    registry.claim("browser:acme").unwrap();
    let pool = registry.pool().unwrap();
    assert_eq!(pool.devices(), ["android:Acme-1", "device:Acme-2"]);
    assert_eq!(pool.browsers(), ["browser:acme"]);
    registry.release_target("android:Acme-1");
    assert_eq!(registry.pool().unwrap().devices(), ["device:Acme-2"]);
}

#[test]
fn a_temporary_claim_leaves_unless_someone_connected_the_target() {
    let mut registry = Registry::default();
    open(&mut registry);
    let claimed = registry.claim_temporary("device:Acme").unwrap();
    assert!(registry.pool().unwrap().devices().is_empty());
    registry.settle_temporary(&claimed);
    assert!(!registry.pool().unwrap().holds("device:Acme"));
    // A connect during the call keeps the target once the call ends.
    let claimed = registry.claim_temporary("device:Acme").unwrap();
    registry.claim("device:Acme").unwrap();
    registry.settle_temporary(&claimed);
    assert_eq!(registry.pool().unwrap().devices(), ["device:Acme"]);
}

#[test]
fn idle_release_waits_for_operations_media_viewers_and_temporary_claims() {
    let mut registry = Registry::default();
    open(&mut registry);
    registry.claim("android:Acme").unwrap();
    registry.claim("browser:acme").unwrap();
    assert!(registry.idle().is_empty());
    idle(&mut registry, "android:Acme");
    idle(&mut registry, "browser:acme");
    // Browsers close on disconnect, never on idle.
    assert_eq!(registry.idle(), ["android:Acme"]);
    registry.begin_operation("op-1", "act", None, Kind::Mcp, Some("android:Acme"));
    assert!(registry.idle().is_empty());
    registry.end_operation("op-1");
    registry.viewer("android:Acme", true);
    idle(&mut registry, "android:Acme");
    assert!(registry.idle().is_empty());
    registry.viewer("android:Acme", false);
    idle(&mut registry, "android:Acme");
    registry
        .reserve_media(Media::Recording, "starting-1")
        .unwrap();
    registry.settle_media("starting-1", Some("acme-recording"), Some("android:Acme"));
    assert!(registry.idle().is_empty());
    registry.release_media("acme-recording");
    assert_eq!(registry.idle(), ["android:Acme"]);
    // Use from any client renews it.
    registry.admit(Some("android:Acme")).unwrap();
    registry.finish();
    assert!(registry.idle().is_empty());
}

#[test]
fn a_failed_release_keeps_only_its_target_and_backs_off_between_attempts() {
    let mut registry = Registry::default();
    open(&mut registry);
    registry.claim("device:Acme-locked").unwrap();
    registry.claim("android:Acme-ready").unwrap();
    assert!(registry.release_started("device:Acme-locked"));
    registry.release_failed("device:Acme-locked", json!({"code":"CLEANUP_FAILED"}));
    let release = registry.pending_release("device:Acme-locked").unwrap();
    assert_eq!(release.attempts, 1);
    // Not due before its delay, and never idle-released meanwhile.
    assert!(registry.due_releases().is_empty());
    idle(&mut registry, "device:Acme-locked");
    assert!(!registry.idle().contains(&"device:Acme-locked".to_owned()));
    let held = registry
        .pool_mut()
        .unwrap()
        .targets
        .get_mut("device:Acme-locked")
        .unwrap();
    held.release.as_mut().unwrap().failed = Awake::now().earlier(Duration::from_secs(2));
    assert_eq!(registry.due_releases(), ["device:Acme-locked"]);
    // One attempt at a time.
    assert!(registry.release_started("device:Acme-locked"));
    assert!(!registry.release_started("device:Acme-locked"));
    assert!(registry.due_releases().is_empty());
    registry.release_failed("device:Acme-locked", json!({"code":"CLEANUP_FAILED"}));
    assert_eq!(
        registry
            .pending_release("device:Acme-locked")
            .unwrap()
            .attempts,
        2
    );
    // The other device is unaffected.
    assert!(registry.pending_release("android:Acme-ready").is_none());
    registry.release_settled("device:Acme-locked");
    assert!(registry.pending_release("device:Acme-locked").is_none());
}

#[test]
fn retry_delays_double_up_to_their_ceiling() {
    let release = |attempts| Release {
        attempts,
        failed: Awake::now(),
        error: Value::Null,
        running: false,
    };
    assert_eq!(release(1).delay(), Duration::from_secs(2));
    assert_eq!(release(2).delay(), Duration::from_secs(4));
    assert_eq!(release(9).delay(), RETRY_CEILING);
}

#[test]
fn transitions_alias_the_pooled_device() {
    let mut registry = Registry::default();
    open(&mut registry);
    registry.claim("android:avd:Acme").unwrap();
    registry
        .transition("android:avd:Acme", "android:emulator-5554")
        .unwrap();
    let pool = registry.pool().unwrap();
    assert_eq!(
        pool.primary("android:emulator-5554").as_deref(),
        Some("android:avd:Acme")
    );
    registry.release_target("android:avd:Acme");
    assert!(!registry.pool().unwrap().holds("android:emulator-5554"));
}

#[test]
fn media_limits_are_manager_wide() {
    let mut registry = Registry::default();
    open(&mut registry);
    for index in 0..Media::Recording.limit() {
        registry
            .reserve_media(Media::Recording, &format!("starting-{index}"))
            .unwrap();
    }
    let refused = registry
        .reserve_media(Media::Recording, "starting-over")
        .unwrap_err();
    assert_eq!(refused.code, "RESOURCE_LIMIT");
    assert_eq!(refused.details.unwrap()["admitted"], false);
}

#[test]
fn a_draining_manager_connects_no_new_target_but_keeps_serving_pooled_ones() {
    let mut registry = Registry::default();
    open(&mut registry);
    registry.claim("android:Acme-1").unwrap();
    registry.claim("browser:acme").unwrap();
    assert!(
        registry
            .idle_after(Duration::ZERO)
            .iter()
            .all(|target| target != "browser:acme")
    );
    registry.draining = true;
    assert_eq!(
        registry.idle_after(Duration::ZERO),
        ["android:Acme-1", "browser:acme"]
    );
    registry.claim("android:Acme-1").unwrap();
    let refused = registry.claim("android:Acme-2").unwrap_err();
    assert_eq!(refused.code, "OWNER_DRAINING");
    assert!(refused.retryable);
}

#[test]
fn the_pool_is_empty_only_with_nothing_pooled_or_running() {
    let mut registry = Registry::default();
    assert!(registry.is_empty());
    open(&mut registry);
    assert!(registry.is_empty());
    registry.admit(None).unwrap();
    assert!(!registry.is_empty());
    registry.finish();
    registry.claim("android:Acme").unwrap();
    assert!(!registry.is_empty());
    registry.release_target("android:Acme");
    assert!(registry.is_empty());
    // An exiting manager opens no new pool.
    registry.close();
    registry.closing = true;
    assert_eq!(
        registry
            .open(|| unreachable!("no pool while exiting"))
            .unwrap_err()
            .code,
        "OWNER_EXITING"
    );
}
