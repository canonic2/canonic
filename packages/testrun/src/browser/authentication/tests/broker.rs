//! Trusted sign-in against a scripted page and a synthetic broker socket.
use super::*;
use crate::browser::{contract::Request, page::Action};
use std::sync::{Arc, Mutex};

const USERNAME: &str = "staff@example.com";
const OTP: &str = "424242";

/// A sign-in screen that accepts `PASSWORD`, then `OTP` when `otp_required`,
/// and records every entry.
#[derive(Default)]
struct Page {
    origin: String,
    document: u128,
    signed_in: bool,
    refused: bool,
    refusal: String,
    /// The signed-in account text, when not the default label.
    account: Option<String>,
    fills: Vec<(String, String)>,
    clicks: Vec<String>,
    /// Navigates once the broker has been asked, before entry.
    navigate_after_reads: Option<usize>,
    /// Navigates just before the first entry.
    navigate_at_entry: Option<usize>,
    entries: usize,
    reads: usize,
    policy: Option<&'static str>,
    otp_required: bool,
    otp_shown: bool,
    /// Submission navigates to this origin.
    redirect: Option<String>,
    /// Submission shows nothing the journey detects.
    stall: bool,
    /// A click on this selector fails.
    failing_click: Option<&'static str>,
    /// Active request-body withholdings, and entries made without one.
    withholding: usize,
    unwithheld_entries: usize,
}
fn frame(document: u128) -> Reference {
    Reference {
        connection_id: uuid::Uuid::nil(),
        id: uuid::Uuid::nil(),
        generation: uuid::Uuid::from_u128(document),
    }
}
struct Scripted(Arc<Mutex<Page>>);
impl Pages for Scripted {
    fn run(
        &mut self,
        request: Request,
        _: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + '_>> {
        let page = self.0.clone();
        Box::pin(async move {
            let mut page = page.lock().unwrap();
            match request {
                Request::Evaluate { .. } => {
                    page.reads += 1;
                    if page.navigate_after_reads == Some(page.reads) {
                        page.document += 1;
                    }
                    Ok(json!({"value":{"value":json!(page.origin).to_string()}}))
                }
                Request::PageRead {
                    read: Read::Count, ..
                } => Ok(json!({"frame":frame(page.document)})),
                Request::PageRead {
                    read: Read::Text,
                    locator: Some(locator),
                    ..
                } => Ok(
                    json!({"result":{"value":if locator.selector().unwrap()=="css=#account" {page.account.clone().unwrap_or_else(|| "Staff user · Acme".to_owned())} else {page.refusal.clone()}}}),
                ),
                Request::PageWait {
                    locator: Some(locator),
                    condition: Condition::Visible {},
                    ..
                } => {
                    let visible = match locator.selector().unwrap().as_str() {
                        "css=#account" => page.signed_in,
                        "css=#sign-in" => !page.signed_in,
                        "css=[role=alert]" => page.refused,
                        "css=#code" => page.otp_shown,
                        _ => false,
                    };
                    if visible {
                        Ok(json!({}))
                    } else {
                        Err("CONDITION_TIMEOUT".into())
                    }
                }
                Request::PageAction {
                    locator,
                    action,
                    frame: bound,
                    ..
                } => {
                    if page.navigate_at_entry == Some(page.entries) {
                        page.document += 1;
                    }
                    page.entries += 1;
                    if bound != Some(frame(page.document)) {
                        return Err("STALE_REFERENCE".into());
                    }
                    if page.withholding == 0 {
                        page.unwithheld_entries += 1;
                    }
                    let selector = locator.selector().unwrap();
                    match action {
                        Action::Fill { value } => {
                            page.fills.push((selector, value));
                        }
                        Action::Click {} => {
                            if page
                                .failing_click
                                .is_some_and(|failing| selector == failing)
                            {
                                return Err("TIMEOUT".into());
                            }
                            let entered = |field: &str, expected: &str| {
                                page.fills
                                    .iter()
                                    .any(|(name, value)| name == field && value == expected)
                            };
                            let accepted = if selector == "css=#verify" {
                                entered("css=#code", OTP)
                            } else {
                                entered("css=#password", PASSWORD)
                            };
                            page.clicks.push(selector);
                            if let Some(origin) = page.redirect.take() {
                                page.origin = origin;
                                page.document += 1;
                            }
                            if page.stall {
                                return Ok(json!({"dispatchCompleted":true}));
                            }
                            let challenge = accepted && page.otp_required && !page.otp_shown;
                            page.otp_shown = challenge;
                            page.signed_in = accepted && !challenge;
                            page.refused = !accepted;
                        }
                        _ => unreachable!(),
                    }
                    Ok(json!({"dispatchCompleted":true}))
                }
                request => panic!("unexpected {request:?}"),
            }
        })
    }
    fn secret_policy(&mut self, _: Reference) -> Result<(), Error> {
        match self.0.lock().unwrap().policy {
            Some(reason) => {
                let mut error = Error::from("SECRET_POLICY_UNENFORCEABLE");
                error.partial = Some(json!({"reason":reason}));
                Err(error)
            }
            None => Ok(()),
        }
    }
    fn withhold_request_bodies(&mut self, page: Reference) -> Result<Reference, Error> {
        self.0.lock().unwrap().withholding += 1;
        Ok(page)
    }
    fn release_request_bodies(
        &mut self,
        _: Reference,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.0.lock().unwrap().withholding -= 1;
        Box::pin(async {})
    }
}
fn page(origin: &str) -> (Scripted, Arc<Mutex<Page>>) {
    let state = Arc::new(Mutex::new(Page {
        origin: origin.into(),
        ..Default::default()
    }));
    (Scripted(state.clone()), state)
}

/// A synthetic broker answering with `reply` for each resolved account.
struct Broker {
    directory: tempfile::TempDir,
    requests: Arc<Mutex<Vec<Value>>>,
}
fn broker(reply: impl Fn(&Value) -> Value + Send + Sync + 'static) -> Broker {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = tokio::net::UnixListener::bind(directory.path().join("acme.sock")).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut line = String::new();
            tokio::io::BufReader::new(read)
                .read_line(&mut line)
                .await
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            seen.lock().unwrap().push(request.clone());
            let answer = if request["method"] == "describe" {
                json!({"protocol":PROTOCOL,"accounts":[{"account":"staff","origin":ORIGIN,"label":"Staff user"}]})
            } else {
                reply(&request)
            };
            write
                .write_all(format!("{answer}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    Broker {
        directory,
        requests,
    }
}
fn accepting() -> Broker {
    broker(|_| json!({"protocol":PROTOCOL,"values":{"username":USERNAME,"password":PASSWORD}}))
}
fn request<'a>(journey: &'a Journey, page: Reference) -> SignIn<'a> {
    SignIn {
        page,
        broker: "acme",
        account: "staff",
        journey,
        expected_identity: Some("Staff user"),
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn sign_in_enters_broker_values_only_into_the_journey_fields_and_never_returns_them() {
    let broker = accepting();
    let (mut pages, state) = page(ORIGIN);
    let journey = journey();
    let result = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(result["action"], "signed_in");
    assert_eq!(result["identity"]["matchesExpected"], true);
    assert!(!result.to_string().contains(PASSWORD));
    assert!(!result.to_string().contains(USERNAME));
    let state = state.lock().unwrap();
    assert_eq!(
        state.fills,
        [
            ("css=#username".to_owned(), USERNAME.to_owned()),
            ("css=#password".to_owned(), PASSWORD.to_owned())
        ]
    );
    assert_eq!(state.clicks, ["css=#submit"]);
    assert_eq!((state.unwithheld_entries, state.withholding), (0, 0));
    let resolved = broker.requests.lock().unwrap();
    assert_eq!(
        resolved.as_slice(),
        [json!({"protocol":PROTOCOL,"method":"resolve","origin":ORIGIN,"account":"staff"})]
    );
}

#[tokio::test]
async fn a_signed_in_page_with_the_expected_identity_is_left_alone_without_the_broker() {
    let broker = accepting();
    let (mut pages, state) = page(ORIGIN);
    state.lock().unwrap().signed_in = true;
    let journey = journey();
    let result = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(result["action"], "none");
    assert!(broker.requests.lock().unwrap().is_empty());
    let mut other = request(&journey, reference());
    other.expected_identity = Some("Workspace admin");
    let error = sign_in(&mut pages, broker.directory.path(), other, deadline())
        .await
        .unwrap_err();
    assert_eq!(error.code, "IDENTITY_MISMATCH");
    assert!(state.lock().unwrap().fills.is_empty());
}

#[tokio::test]
async fn other_origins_unknown_screens_and_unenforceable_secrecy_refuse_before_resolution() {
    let broker = accepting();
    let journey = journey();
    let (mut foreign, _) = page("https://attacker.example.net");
    let error = sign_in(
        &mut foreign,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "ORIGIN_MISMATCH");
    let (mut traced, state) = page(ORIGIN);
    state.lock().unwrap().policy = Some("trace");
    let error = sign_in(
        &mut traced,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "SECRET_POLICY_UNENFORCEABLE");
    let mut unknown_screen = journey.clone();
    unknown_screen.signed_out = css("#missing");
    let (mut elsewhere, _) = page(ORIGIN);
    let error = sign_in(
        &mut elsewhere,
        broker.directory.path(),
        request(&unknown_screen, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "NOT_ON_SIGN_IN");
    assert!(broker.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_navigation_while_resolving_refuses_before_any_entry() {
    let broker = accepting();
    let (mut pages, state) = page(ORIGIN);
    state.lock().unwrap().navigate_after_reads = Some(2);
    let journey = journey();
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "DOCUMENT_CHANGED");
    assert!(state.lock().unwrap().fills.is_empty());
}

#[tokio::test]
async fn refused_sign_ins_withhold_page_messages_that_repeat_a_value() {
    let broker =
        broker(|_| json!({"protocol":PROTOCOL,"values":{"username":USERNAME,"password":"wrong"}}));
    let journey = journey();
    let (mut pages, state) = page(ORIGIN);
    state.lock().unwrap().refusal = "Unknown password wrong for staff@example.com".into();
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "SIGN_IN_REFUSED");
    let partial = error.partial.unwrap();
    assert_eq!(partial["messageWithheld"], true);
    assert!(partial["message"].is_null());
    let (mut pages, state) = page(ORIGIN);
    state.lock().unwrap().refusal = "Incorrect sign-in".into();
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.partial.unwrap()["message"], "Incorrect sign-in");
}

#[tokio::test]
async fn broker_refusals_carry_known_codes_only_and_missing_brokers_need_a_session() {
    let journey = journey();
    let leaking = broker(|request| {
        if request["account"] == "staff" {
            json!({"protocol":PROTOCOL,"error":{"code":"SESSION_EXPIRED","message":PASSWORD}})
        } else {
            json!({"protocol":PROTOCOL,"error":{"code":"VENDOR_SPECIFIC"}})
        }
    });
    let (mut pages, _) = page(ORIGIN);
    let error = sign_in(
        &mut pages,
        leaking.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "SESSION_EXPIRED");
    assert!(!format!("{error:?}").contains(PASSWORD));
    let mut other = request(&journey, reference());
    other.account = "admin";
    let (mut pages, _) = page(ORIGIN);
    let error = sign_in(&mut pages, leaking.directory.path(), other, deadline())
        .await
        .unwrap_err();
    assert_eq!(error.code, "BROKER_REFUSED");
    let empty = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        empty.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let (mut pages, _) = page(ORIGIN);
    let error = sign_in(
        &mut pages,
        empty.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "SESSION_NOT_STARTED");
}

#[tokio::test]
async fn shared_broker_directories_are_untrusted() {
    use std::os::unix::fs::PermissionsExt;
    let broker = accepting();
    std::fs::set_permissions(
        broker.directory.path(),
        std::fs::Permissions::from_mode(0o770),
    )
    .unwrap();
    let journey = journey();
    let (mut pages, _) = page(ORIGIN);
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "BROKER_UNTRUSTED");
    assert!(broker.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn status_reads_state_and_identity_without_entry_and_brokers_list_labels_only() {
    let (mut pages, state) = page(ORIGIN);
    let journey = journey();
    let out = status(&mut pages, reference(), &journey, None, deadline())
        .await
        .unwrap();
    assert_eq!(
        (out["state"].as_str(), out["signedIn"].as_bool()),
        (Some("signed_out"), Some(false))
    );
    state.lock().unwrap().signed_in = true;
    let signed = status(&mut pages, reference(), &journey, Some("Admin"), deadline())
        .await
        .unwrap();
    assert_eq!(signed["identity"]["matchesExpected"], false);
    assert!(state.lock().unwrap().fills.is_empty());
    let (mut foreign, _) = page("https://other.example.com");
    let other = status(&mut foreign, reference(), &journey, None, deadline())
        .await
        .unwrap();
    assert_eq!(other["state"], "other_origin");
    let broker = accepting();
    let listed = brokers(broker.directory.path(), None, None, deadline())
        .await
        .unwrap();
    assert_eq!(listed["directory"], json!(broker.directory.path()));
    assert_eq!(
        listed["brokers"],
        json!([{"broker":"acme","status":"available","protocol":PROTOCOL,"accounts":[{"account":"staff","origin":ORIGIN,"label":"Staff user"}]}])
    );
    let missing = brokers(
        &broker.directory.path().join("absent"),
        None,
        None,
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(missing["brokers"], json!([]));
}

fn otp_journey() -> Journey {
    Journey {
        otp: Some(css("#code")),
        otp_submit: Some(css("#verify")),
        ..journey()
    }
}
fn with_otp() -> Broker {
    broker(
        |_| json!({"protocol":PROTOCOL,"values":{"username":USERNAME,"password":PASSWORD,"otp":OTP}}),
    )
}

#[tokio::test]
async fn a_one_time_code_is_entered_once_into_the_document_that_asks_for_it() {
    let broker = with_otp();
    let journey = otp_journey();
    let (mut pages, state) = page(ORIGIN);
    {
        let mut state = state.lock().unwrap();
        state.otp_required = true;
        // A same-origin navigation to the challenge replaces the document.
        state.redirect = Some(ORIGIN.into());
    }
    let result = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        (result["action"].as_str(), result["otpEntered"].as_bool()),
        (Some("signed_in"), Some(true))
    );
    assert!(!result.to_string().contains(OTP));
    let state = state.lock().unwrap();
    assert_eq!(
        state.fills.last(),
        Some(&("css=#code".to_owned(), OTP.to_owned()))
    );
    assert_eq!(state.clicks, ["css=#submit", "css=#verify"]);
    assert_eq!((state.unwithheld_entries, state.withholding), (0, 0));
}

#[tokio::test]
async fn a_challenge_on_another_origin_never_receives_the_code() {
    let broker = with_otp();
    let journey = otp_journey();
    let (mut pages, state) = page(ORIGIN);
    {
        let mut state = state.lock().unwrap();
        state.otp_required = true;
        state.redirect = Some("https://other.example.com".into());
    }
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "ORIGIN_MISMATCH");
    let partial = error.partial.unwrap();
    assert_eq!(
        (
            partial["submitted"].as_bool(),
            partial["otpEntered"].as_bool()
        ),
        (Some(true), Some(false))
    );
    let state = state.lock().unwrap();
    assert!(
        state
            .fills
            .iter()
            .all(|(field, value)| field != "css=#code" && value != OTP)
    );
    assert_eq!(state.withholding, 0);
}

#[tokio::test]
async fn challenges_without_a_broker_code_and_failed_steps_end_without_replay() {
    let journey = otp_journey();
    let broker = accepting();
    let (mut pages, state) = page(ORIGIN);
    state.lock().unwrap().otp_required = true;
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "MFA_REQUIRED");
    assert_eq!(error.partial.unwrap()["submitted"], true);
    assert_eq!(state.lock().unwrap().clicks, ["css=#submit"]);

    let broker = with_otp();
    let (mut pages, state) = page(ORIGIN);
    {
        let mut state = state.lock().unwrap();
        state.otp_required = true;
        state.failing_click = Some("css=#verify");
    }
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "TIMEOUT");
    let partial = error.partial.unwrap();
    assert_eq!(
        (partial["step"].as_str(), partial["submitted"].as_bool()),
        (Some("otp_submit"), Some(true))
    );
    assert!(!format!("{partial}").contains(OTP));
    assert_eq!(state.lock().unwrap().withholding, 0);

    let (mut pages, state) = page(ORIGIN);
    state.lock().unwrap().stall = true;
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        Instant::now() + Duration::from_millis(300),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "SIGN_IN_OUTCOME_UNKNOWN");
    assert_eq!(error.partial.unwrap()["nextAction"], "auth_status");
    assert_eq!(state.lock().unwrap().clicks, ["css=#submit"]);
}

#[tokio::test]
async fn entry_is_bound_to_the_checked_document() {
    let broker = accepting();
    let journey = journey();
    // The document is replaced before the first fill, then between fills.
    for (entry, step, filled) in [(0, "username", 0), (1, "password", 1)] {
        let (mut pages, state) = page(ORIGIN);
        state.lock().unwrap().navigate_at_entry = Some(entry);
        let error = sign_in(
            &mut pages,
            broker.directory.path(),
            request(&journey, reference()),
            deadline(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            (error.code, error.partial.unwrap()["step"].as_str()),
            ("STALE_REFERENCE", Some(step))
        );
        let state = state.lock().unwrap();
        assert_eq!(state.fills.len(), filled);
        assert!(state.fills.iter().all(|(_, value)| value != PASSWORD));
        assert_eq!(state.withholding, 0);
    }
}

#[tokio::test]
async fn identity_text_that_repeats_an_entered_value_is_withheld() {
    let broker = accepting();
    let journey = journey();
    let shows_username = || {
        let (pages, state) = page(ORIGIN);
        state.lock().unwrap().account = Some(format!("Signed in as {USERNAME}"));
        (pages, state)
    };
    let (mut pages, _) = shows_username();
    let mut unchecked = request(&journey, reference());
    unchecked.expected_identity = None;
    let result = sign_in(&mut pages, broker.directory.path(), unchecked, deadline())
        .await
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(result["identity"]["textWithheld"], true);
    assert!(result["identity"]["text"].is_null());
    assert!(!result.to_string().contains(USERNAME));
    let (mut pages, _) = shows_username();
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "IDENTITY_MISMATCH");
    assert_eq!(
        error.partial.as_ref().unwrap()["identity"]["textWithheld"],
        true
    );
    assert!(!format!("{error:?}").contains(USERNAME));
}

#[tokio::test]
async fn an_expected_identity_without_a_reader_refuses_before_resolution() {
    let broker = accepting();
    let mut journey = journey();
    journey.identity = None;
    let (mut pages, state) = page(ORIGIN);
    let error = sign_in(
        &mut pages,
        broker.directory.path(),
        request(&journey, reference()),
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "JOURNEY_IDENTITY_REQUIRED");
    assert!(broker.requests.lock().unwrap().is_empty());
    assert!(state.lock().unwrap().fills.is_empty());
    assert_eq!(
        validate_status(&journey, Some("Staff user")),
        Err("JOURNEY_IDENTITY_REQUIRED")
    );
}

#[tokio::test]
async fn a_broker_that_keeps_its_connection_open_after_replying_is_read_by_line() {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = tokio::net::UnixListener::bind(directory.path().join("acme.sock")).unwrap();
    let (held, mut hold) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut line = String::new();
        tokio::io::BufReader::new(read)
            .read_line(&mut line)
            .await
            .unwrap();
        write
            .write_all(
                format!(
                    "{}\n",
                    json!({"protocol":PROTOCOL,"values":{"password":PASSWORD}})
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        // The connection stays open until the test ends.
        let _ = (&mut hold).await;
        drop(write);
    });
    let values = resolve(
        directory.path(),
        "acme",
        "staff",
        ORIGIN,
        Instant::now() + Duration::from_secs(2),
    )
    .await
    .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(values.password.expose(), PASSWORD);
    drop(held);
}

#[tokio::test]
async fn broker_listing_contacts_only_the_requested_page() {
    let broker = accepting();
    for name in ["beta", "gamma"] {
        tokio::net::UnixListener::bind(broker.directory.path().join(format!("{name}.sock")))
            .map(std::mem::forget)
            .unwrap();
    }
    let first = brokers(broker.directory.path(), None, Some(1), deadline())
        .await
        .unwrap();
    assert_eq!(first["brokers"][0]["broker"], "acme");
    assert_eq!(first["brokers"].as_array().unwrap().len(), 1);
    assert_eq!(broker.requests.lock().unwrap().len(), 1);
    let rest = brokers(
        broker.directory.path(),
        first["nextCursor"].as_str().map(Into::into),
        Some(2),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(rest["brokers"][0]["broker"], "beta");
    assert_eq!(rest["brokers"][1]["broker"], "gamma");
    assert!(rest["nextCursor"].is_null());
    assert_eq!(broker.requests.lock().unwrap().len(), 1);
}
