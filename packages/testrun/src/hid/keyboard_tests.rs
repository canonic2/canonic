//! Boundary fixtures for keyboard ownership, pressed sets and interrupted input.
use super::*;
#[derive(Default)]
struct State {
    requests: Vec<Value>,
    sent: Vec<(u64, Vec<u8>)>,
    fail_at: Option<usize>,
    fail_release: bool,
    fail_remove: bool,
    registered: bool,
    lose_create_reply: bool,
    retain_after_remove: bool,
}
struct Fake(Arc<Mutex<State>>);
impl Reports for Fake {
    fn send(
        &mut self,
        service_id: u64,
        report: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut state = self.0.lock().unwrap();
            let release = report[0] == 1 && report[1..31].iter().all(|&b| b == 0);
            state.sent.push((service_id, report));
            if state.fail_at == Some(state.sent.len()) || (state.fail_release && release) {
                return Err(Error::new(
                    "DEVICE_DISCONNECTED",
                    "Fixture interrupted HID send",
                ));
            }
            Ok(())
        })
    }
    fn request(
        &mut self,
        payload: Dictionary,
        reply: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + '_>> {
        Box::pin(async move {
            let payload = serde_json::to_value(X::Dictionary(payload).to_plist()).unwrap();
            let mut state = self.0.lock().unwrap();
            state.requests.push(payload.clone());
            if payload.get("createService").is_some() {
                assert!(reply, "Registration requires its assigned service ID reply");
                state.registered = true;
                if state.lose_create_reply {
                    return Err(Error::new(
                        "DEVICE_DISCONNECTED",
                        "Fixture lost registration reply",
                    ));
                }
                // Exercise a device-assigned ID distinct from the requested ID.
                Ok(json!({"serviceID":0x100005555_u64}))
            } else if payload.get("connectedServices").is_some() {
                assert!(
                    reply,
                    "Removal must confirm absence in a service-list reply"
                );
                Ok(
                    json!({"connectedServices":if state.registered {vec![json!({"_ServiceID":0x100005555_u64})]} else {vec![]}}),
                )
            } else if state.fail_remove {
                Err(Error::new(
                    "DEVICE_DISCONNECTED",
                    "Fixture interrupted removal",
                ))
            } else {
                assert!(
                    !reply,
                    "Removal is one-way; waiting for its reply would time out"
                );
                if !state.retain_after_remove {
                    state.registered = false;
                }
                Ok(json!({}))
            }
        })
    }
}
fn fixture() -> (Arc<Hid>, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (Hid::new(Box::new(Fake(state.clone()))), state)
}
fn text(value: &str) -> Value {
    json!({"scope":{"kind":"display"},"text":value})
}
fn usages(report: &[u8]) -> Vec<u32> {
    (0..240)
        .filter(|&u| report[1 + u as usize / 8] & (1 << (u % 8)) != 0)
        .collect()
}
#[tokio::test]
async fn lazy_owned_keyboard_types_shifted_email_and_releases_every_key() {
    let (hid, state) = fixture();
    assert!(state.lock().unwrap().requests.is_empty());
    assert_eq!(hid.health()["hardwareKeyboardAttached"], false);
    let reply = hid.input("type_text", &text("Aa@.aa\n\t")).await.unwrap();
    assert_eq!(reply["focusVerified"], false);
    assert_eq!(reply["deliveryAcknowledged"], false);
    assert_eq!(reply["hardwareKeyboardAttached"], true);
    assert_eq!(reply["hardwareKeyboardJustAttached"], true);
    assert_eq!(hid.health()["hardwareKeyboardAttached"], true);
    let sent = state.lock().unwrap().sent.clone();
    // The first press and release is the throwaway Shift tap after registration.
    let expected = vec![
        vec![225],
        vec![],
        vec![225],
        vec![4, 225],
        vec![],
        vec![4],
        vec![],
        vec![225],
        vec![31, 225],
        vec![],
        vec![55],
        vec![],
        vec![4],
        vec![],
        vec![4],
        vec![],
        vec![40],
        vec![],
        vec![43],
        vec![],
    ];
    assert_eq!(
        sent.iter().map(|(_, r)| usages(r)).collect::<Vec<_>>(),
        expected
    );
    assert!(
        sent.iter()
            .all(|(id, r)| *id == 0x100005555 && r.len() == 39 && r[0] == 1)
    );
    let mut key = text("");
    key["key"] = json!("backspace");
    let reply = hid.input("key", &key).await.unwrap();
    assert_eq!(reply["hardwareKeyboardJustAttached"], false);
    assert_eq!(state.lock().unwrap().requests.len(), 1);
    assert_eq!(hid.health()["heldKeys"], false);
    hid.close().await.unwrap();
    assert_eq!(hid.health()["hardwareKeyboardAttached"], false);
    hid.close().await.unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.requests.len(), 3);
    assert_eq!(state.requests[1]["removeService"]["_0"], 0x100005555_u64);
    let properties = &state.requests[0]["createService"]["_0"];
    assert_eq!(properties["PrimaryUsagePage"], 1);
    assert_eq!(
        properties["_CoreDevice_codablePropertyStorage"]["DeviceUsagePairs"]["array"][0]["dictionary"]
            ["DeviceUsage"]["int"],
        6
    );
    assert_eq!(
        properties["_CoreDevice_codablePropertyStorage"]["UniversalControlVirtualService"]["bool"],
        true
    );
}
#[tokio::test]
async fn unsupported_text_and_semantic_options_never_register_or_send() {
    let (hid, state) = fixture();
    let mut replacement = text("Acme");
    replacement["replace"] = json!(true);
    for args in [text("Acmeé"), replacement] {
        assert_eq!(
            hid.input("type_text", &args).await.unwrap_err().code,
            "CAPABILITY_UNAVAILABLE"
        );
    }
    let mut args = text("Acme");
    args["element"] = json!({"snapshotId":"snapshot","elementId":"field"});
    assert!(hid.input("type_text", &args).await.is_err());
    assert!(state.lock().unwrap().requests.is_empty());
    assert!(state.lock().unwrap().sent.is_empty());
}
#[tokio::test]
async fn failed_shifted_keydown_releases_modifier_and_quarantines_session() {
    let (hid, state) = fixture();
    // Reports 1 and 2 are the throwaway Shift tap; report 4 is Shift+A.
    state.lock().unwrap().fail_at = Some(4);
    assert_eq!(
        hid.input("type_text", &text("Ab")).await.unwrap_err().code,
        "OUTCOME_UNKNOWN"
    );
    // close also waits for any cancellation cleanup task.
    hid.close().await.unwrap();
    {
        let state = state.lock().unwrap();
        assert_eq!(state.sent.len(), 5);
        assert!(usages(&state.sent[4].1).is_empty());
    }
    assert_eq!(
        hid.input("type_text", &text("b")).await.unwrap_err().code,
        "SESSION_UNCERTAIN"
    );
}
#[tokio::test]
async fn failed_setup_shift_tap_releases_and_sends_none_of_the_callers_keys() {
    let (hid, state) = fixture();
    // Report 1 is the throwaway Shift press after registration.
    state.lock().unwrap().fail_at = Some(1);
    assert_eq!(
        hid.input("type_text", &text("Ab")).await.unwrap_err().code,
        "OUTCOME_UNKNOWN"
    );
    hid.close().await.unwrap();
    {
        let state = state.lock().unwrap();
        assert_eq!(state.sent.len(), 2, "Shift press, then the release only");
        assert_eq!(usages(&state.sent[0].1), vec![keyboard::PRIME]);
        assert!(usages(&state.sent[1].1).is_empty());
    }
    assert_eq!(
        hid.input("type_text", &text("b")).await.unwrap_err().code,
        "SESSION_UNCERTAIN"
    );
}
#[tokio::test]
async fn cancellation_after_modifier_press_releases_keys_without_replay() {
    let (hid, state) = fixture();
    // Skip registration only for this cancellation fixture; cancel while Shift is held.
    *hid.keyboard.lock().unwrap() = Some(0x100005555);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            hid.input("type_text", &text("A"))
        )
        .await
        .is_err()
    );
    hid.close().await.unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.sent.len(), 2);
    assert_eq!(usages(&state.sent[0].1), vec![225]);
    assert!(usages(&state.sent[1].1).is_empty());
    assert_eq!(hid.health()["uncertain"], true);
}
#[tokio::test]
async fn cancellation_during_attachment_removes_keyboard_without_sending_keys() {
    let (hid, state) = fixture();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            hid.input("type_text", &text("A"))
        )
        .await
        .is_err()
    );
    assert!(state.lock().unwrap().sent.is_empty());
    hid.close().await.unwrap();
    let state = state.lock().unwrap();
    assert!(!state.registered);
    assert_eq!(state.requests[1]["removeService"]["_0"], 0x100005555_u64);
    assert_eq!(hid.health()["uncertain"], true);
}
#[tokio::test]
async fn lost_registration_reply_cannot_claim_owned_keyboard_cleanup() {
    let (hid, state) = fixture();
    state.lock().unwrap().lose_create_reply = true;
    assert_eq!(
        hid.input("type_text", &text("A")).await.unwrap_err().code,
        "OUTCOME_UNKNOWN"
    );
    assert_eq!(hid.close().await.unwrap_err().code, "CLEANUP_FAILED");
    let state = state.lock().unwrap();
    assert_eq!(state.requests.len(), 1);
    assert!(state.sent.is_empty());
    assert!(state.registered);
}
#[tokio::test]
async fn successful_remove_send_without_service_disappearance_is_not_clean() {
    let (hid, state) = fixture();
    *hid.keyboard.lock().unwrap() = Some(0x100005555);
    {
        let mut state = state.lock().unwrap();
        state.registered = true;
        state.retain_after_remove = true;
    }
    assert_eq!(hid.close().await.unwrap_err().code, "CLEANUP_FAILED");
    assert!(hid.keyboard.lock().unwrap().is_some());
    state.lock().unwrap().retain_after_remove = false;
    hid.close().await.unwrap();
    assert!(hid.keyboard.lock().unwrap().is_none());
}
#[tokio::test]
async fn failed_release_and_failed_removal_remain_cleanup_failures() {
    let (hid, state) = fixture();
    *hid.keyboard.lock().unwrap() = Some(0x100005555);
    state.lock().unwrap().fail_release = true;
    state.lock().unwrap().fail_remove = true;
    hid.send_keys(&[225]).await.unwrap();
    assert_eq!(hid.close().await.unwrap_err().code, "CLEANUP_FAILED");
    assert_eq!(hid.health()["heldKeys"], true);
    assert!(!hid.health()["releaseError"].is_null());
    state.lock().unwrap().fail_release = false;
    state.lock().unwrap().fail_remove = true;
    assert_eq!(hid.close().await.unwrap_err().code, "CLEANUP_FAILED");
    assert!(hid.keyboard.lock().unwrap().is_some());
    state.lock().unwrap().fail_remove = false;
    hid.close().await.unwrap();
}
#[test]
fn keys_after_registration_are_paced_slower_than_later_keys() {
    let warm = keyboard::pace(true, 0);
    let steady = keyboard::pace(false, 0);
    assert!(warm.hold > steady.hold && warm.gap > steady.gap);
    assert_eq!(keyboard::pace(true, 2), warm);
    assert_eq!(keyboard::pace(true, 3), steady);
    assert!(keyboard::ATTACH_SETTLE >= std::time::Duration::from_millis(600));
}
