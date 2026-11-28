//! Origin-bound trusted sign-in through a provider-neutral credential broker,
//! and sign-in state detection from explicit application journeys.
//!
//! A broker is a separate per-user process. It serves `testrun-broker-v1`
//! over a Unix socket in the private broker directory; Testrun reads
//! credential values from it and enters them through the page's own input
//! owner. Values never enter requests, results, errors or logs.
use super::contract::Reference;
use super::page::{Condition, Locator, Read};
use super::session::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::time::{Duration, Instant};

pub(crate) const PROTOCOL: &str = "testrun-broker-v1";
const MAX_BROKERS: usize = 32;
#[cfg(unix)]
const MAX_REPLY_BYTES: usize = 65536;
const MAX_VALUE_BYTES: usize = 4096;
const MAX_ACCOUNTS: usize = 64;
const MAX_LABEL_BYTES: usize = 256;
/// The longest wait for a sign-in outcome after submission.
const OUTCOME_WAIT: Duration = Duration::from_secs(30);
/// One visibility probe within an outcome or state wait.
const PROBE: Duration = Duration::from_millis(250);
/// Broker refusal codes a result may carry; broker messages are never relayed.
#[cfg(unix)]
const BROKER_CODES: [&str; 6] = [
    "ACCOUNT_UNKNOWN",
    "ORIGIN_MISMATCH",
    "SESSION_EXPIRED",
    "SESSION_NOT_STARTED",
    "ACCESS_DENIED",
    "UNAVAILABLE",
];

/// The application's sign-in screen, outcome detectors and identity reader.
/// Locators are document-independent; element references are refused.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journey {
    /// The exact origin whose main document may receive credential values.
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<Locator>,
    pub password: Locator,
    pub submit: Locator,
    /// Visible only while signed in.
    pub signed_in: Locator,
    /// Visible only on the signed-out sign-in screen: sign-out detection.
    pub signed_out: Locator,
    /// A visible sign-in refusal, such as an alert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Locator>,
    /// A one-time-code field; filled only when the broker supplies `otp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otp: Option<Locator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otp_submit: Option<Locator>,
    /// Text naming the signed-in account, compared with `expectedIdentity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<Locator>,
}
impl Journey {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        super::settings::validate_origin(&self.origin)?;
        if !(self.origin.starts_with("https://") || self.origin.starts_with("http://")) {
            return Err("INVALID_AUTH_ORIGIN");
        }
        for locator in [
            self.username.as_ref(),
            Some(&self.password),
            Some(&self.submit),
            Some(&self.signed_in),
            Some(&self.signed_out),
            self.failure.as_ref(),
            self.otp.as_ref(),
            self.otp_submit.as_ref(),
            self.identity.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if contains_element(locator) {
                return Err("JOURNEY_ELEMENT_REFERENCE");
            }
            locator.selector()?;
        }
        if self.otp.is_some() != self.otp_submit.is_some() {
            return Err("JOURNEY_OTP_INCOMPLETE");
        }
        Ok(())
    }
}
fn contains_element(locator: &Locator) -> bool {
    match locator {
        Locator::Element { .. } => true,
        Locator::Within { parent, child } => contains_element(parent) || contains_element(child),
        Locator::Filter { locator, .. } | Locator::Nth { locator, .. } => contains_element(locator),
        _ => false,
    }
}

pub(crate) fn validate_name(name: &str, code: &'static str) -> Result<(), &'static str> {
    (!name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    .then_some(())
    .ok_or(code)
}
pub(crate) fn validate_identity(expected: Option<&str>) -> Result<(), &'static str> {
    if expected.is_some_and(|expected| expected.is_empty() || expected.len() > MAX_LABEL_BYTES) {
        return Err("INVALID_EXPECTED_IDENTITY");
    }
    Ok(())
}

/// A broker-held value. It is never serialized, formatted or retained past
/// the fill that uses it, and its bytes are overwritten on drop.
pub(crate) struct Secret(String);
impl Secret {
    fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(..)")
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        std::mem::take(&mut self.0).into_bytes().fill(0);
    }
}
/// A broker reply buffer, overwritten when dropped, including when a
/// deadline cancels the read.
#[cfg(unix)]
struct Wiped(Vec<u8>);
#[cfg(unix)]
impl Drop for Wiped {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
#[derive(Debug)]
pub(crate) struct Values {
    username: Option<Secret>,
    password: Secret,
    otp: Option<Secret>,
}
impl Values {
    /// Whether `text` repeats any value, so a page message can be withheld.
    fn appears_in(&self, text: &str) -> bool {
        [
            Some(&self.password),
            self.username.as_ref(),
            self.otp.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|secret| !secret.expose().is_empty() && text.contains(secret.expose()))
    }
}

/// The private broker directory: `brokers/` in Testrun's per-user data root,
/// which the manager and its callers share.
pub(crate) fn broker_directory() -> Result<PathBuf, Error> {
    let root = crate::managed::data_root().map_err(|_| Error::from("SESSION_NOT_STARTED"))?;
    if !root.is_absolute() {
        return Err("SESSION_NOT_STARTED".into());
    }
    Ok(root.join("brokers"))
}

#[cfg(unix)]
fn private_owner(metadata: &std::fs::Metadata, directory: bool) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    // SAFETY: geteuid has no preconditions.
    let user = unsafe { libc::geteuid() };
    let kind = if directory {
        metadata.file_type().is_dir()
    } else {
        metadata.file_type().is_socket()
    };
    kind && metadata.uid() == user && (!directory || metadata.mode() & 0o077 == 0)
}

/// One request on a fresh connection to a verified broker of this user.
#[cfg(unix)]
async fn exchange(
    directory: &Path,
    name: &str,
    request: Value,
    deadline: Instant,
) -> Result<Value, Error> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    validate_name(name, "INVALID_BROKER_NAME")?;
    let untrusted = |check: &str| {
        let mut error = Error::from("BROKER_UNTRUSTED");
        error.partial = Some(
            json!({"broker":name,"check":check,"nextAction":"the broker's owner must recreate its private directory and socket"}),
        );
        error
    };
    let path = directory.join(format!("{name}.sock"));
    for (entry, is_directory, owned, readable) in [
        (
            directory,
            true,
            "directory-private-to-user",
            "directory-readable",
        ),
        (&path, false, "socket-owned-by-user", "socket-readable"),
    ] {
        match std::fs::symlink_metadata(entry) {
            Ok(metadata) if private_owner(&metadata, is_directory) => {}
            Ok(_) => return Err(untrusted(owned)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err("SESSION_NOT_STARTED".into());
            }
            Err(_) => return Err(untrusted(readable)),
        }
    }
    let work = async {
        let mut stream = tokio::net::UnixStream::connect(&path)
            .await
            .map_err(|_| Error::from("SESSION_NOT_STARTED"))?;
        // SAFETY: geteuid has no preconditions.
        let user = unsafe { libc::geteuid() };
        if stream
            .peer_cred()
            .map_err(|_| untrusted("peer-credentials"))?
            .uid()
            != user
        {
            return Err(untrusted("peer-is-user"));
        }
        let mut line = serde_json::to_vec(&request).map_err(|_| Error::from("INTERNAL"))?;
        line.push(b'\n');
        stream
            .write_all(&line)
            .await
            .map_err(|_| Error::from("BROKER_UNAVAILABLE"))?;
        stream
            .shutdown()
            .await
            .map_err(|_| Error::from("BROKER_UNAVAILABLE"))?;
        // One reply line, read in place so no copy outlives the wipe. The
        // broker may keep its end open after the line.
        let mut reply = Wiped(vec![0; MAX_REPLY_BYTES + 1]);
        let reply = &mut reply.0;
        let mut length = 0;
        let read = loop {
            match stream.read(&mut reply[length..]).await {
                Ok(0) => break Ok(()),
                Ok(count) => {
                    let newline = reply[length..length + count].contains(&b'\n');
                    length += count;
                    if newline || length > MAX_REPLY_BYTES {
                        break Ok(());
                    }
                }
                Err(error) => break Err(error),
            }
        };
        let end = reply[..length].iter().position(|byte| *byte == b'\n');
        if read.is_err() || end.unwrap_or(length) > MAX_REPLY_BYTES {
            return Err(if read.is_err() {
                "BROKER_UNAVAILABLE"
            } else {
                "BROKER_PROTOCOL"
            }
            .into());
        }
        let value = serde_json::from_slice::<Value>(&reply[..end.unwrap_or(length)])
            .map_err(|_| Error::from("BROKER_PROTOCOL"))?;
        if value["protocol"] != PROTOCOL {
            return Err("BROKER_PROTOCOL".into());
        }
        if let Some(code) = value.get("error") {
            let code = code["code"]
                .as_str()
                .and_then(|code| BROKER_CODES.iter().find(|known| **known == code))
                .copied()
                .unwrap_or("BROKER_REFUSED");
            return Err(code.into());
        }
        Ok(value)
    };
    tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Error::from("BROKER_TIMEOUT"))?
}
#[cfg(not(unix))]
async fn exchange(_: &Path, _: &str, _: Value, _: Instant) -> Result<Value, Error> {
    Err("BROKER_UNSUPPORTED".into())
}

/// A broker's public description: its origins and account labels.
fn description(name: &str, reply: &Value) -> Result<Value, Error> {
    let accounts = reply["accounts"].as_array().ok_or("BROKER_PROTOCOL")?;
    if accounts.len() > MAX_ACCOUNTS {
        return Err("BROKER_PROTOCOL".into());
    }
    let accounts = accounts
        .iter()
        .map(|account| {
            let name = account["account"].as_str().ok_or("BROKER_PROTOCOL")?;
            validate_name(name, "BROKER_PROTOCOL")?;
            let origin = account["origin"].as_str().ok_or("BROKER_PROTOCOL")?;
            super::settings::validate_origin(origin).map_err(|_| "BROKER_PROTOCOL")?;
            let label = account["label"]
                .as_str()
                .filter(|label| label.len() <= MAX_LABEL_BYTES)
                .unwrap_or(name);
            Ok(json!({"account":name,"origin":origin,"label":label}))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    Ok(json!({"broker":name,"status":"available","protocol":PROTOCOL,"accounts":accounts}))
}

/// The brokers in `directory`, each described or with its refusal code.
pub(crate) async fn brokers(
    directory: &Path,
    cursor: Option<String>,
    limit: Option<u64>,
    deadline: Instant,
) -> Result<Value, Error> {
    let mut names = Vec::new();
    match std::fs::read_dir(directory) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if let Some(name) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".sock"))
                    && validate_name(name, "INVALID_BROKER_NAME").is_ok()
                {
                    names.push(name.to_owned());
                }
                if names.len() > MAX_BROKERS {
                    return Err("BROKER_CAPACITY".into());
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("BROKER_UNTRUSTED".into()),
    }
    let page = |rows: Vec<(Vec<u8>, Value)>| {
        super::storage::keyset(
            "auth_broker_list",
            &json!({}),
            rows,
            |row| row,
            cursor.as_deref(),
            limit,
            "browser_send",
        )
    };
    // Only the requested page's brokers are contacted, all at once.
    let (selected, more) = page(
        names
            .into_iter()
            .map(|name| (name.clone().into_bytes(), Value::String(name)))
            .collect(),
    )?;
    let probe = deadline.min(Instant::now() + Duration::from_secs(2));
    let rows = futures_util::future::join_all(selected.iter().filter_map(Value::as_str).map(
        |name| async move {
            let row = match exchange(
                directory,
                name,
                json!({"protocol":PROTOCOL,"method":"describe"}),
                probe,
            )
            .await
            .and_then(|reply| description(name, &reply))
            {
                Ok(row) => row,
                Err(error) => json!({"broker":name,"status":"unavailable","code":error.code}),
            };
            (name.as_bytes().to_vec(), row)
        },
    ))
    .await;
    // Descriptions can exceed the byte budget that the names fit.
    let (items, truncated) = page(rows)?;
    Ok(
        json!({"brokers":items,"nextCursor":truncated.or(more),"directory":directory,"valuesReturned":false}),
    )
}

/// Resolves `account`'s values for `origin` from the named broker.
async fn resolve(
    directory: &Path,
    broker: &str,
    account: &str,
    origin: &str,
    deadline: Instant,
) -> Result<Values, Error> {
    let mut reply = exchange(
        directory,
        broker,
        json!({"protocol":PROTOCOL,"method":"resolve","origin":origin,"account":account}),
        deadline,
    )
    .await?;
    let mut take = |field: &str| -> Result<Option<Secret>, Error> {
        match reply["values"].get_mut(field).map(Value::take) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) if !value.is_empty() && value.len() <= MAX_VALUE_BYTES => {
                Ok(Some(Secret(value)))
            }
            Some(Value::String(value)) => {
                drop(Secret(value));
                Err("BROKER_PROTOCOL".into())
            }
            Some(_) => Err("BROKER_PROTOCOL".into()),
        }
    };
    let values = Values {
        username: take("username")?,
        password: take("password")?.ok_or("BROKER_PROTOCOL")?,
        otp: take("otp")?,
    };
    if reply["origin"]
        .as_str()
        .is_some_and(|value| value != origin)
    {
        return Err("ORIGIN_MISMATCH".into());
    }
    Ok(values)
}

/// The owned browser operations a journey composes. Implementations run them
/// through the backend's own input owner and refuse unenforceable secrecy.
pub(crate) trait Pages {
    fn run(
        &mut self,
        request: super::contract::Request,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + '_>>;
    /// Refuses when a trace, HAR capture or request callback could record the
    /// values entered into `page`.
    fn secret_policy(&mut self, page: Reference) -> Result<(), Error>;
    /// Withholds the bodies of requests that `page`'s context sends from now
    /// on, and skips their request-input capture. Returns the context.
    fn withhold_request_bodies(&mut self, page: Reference) -> Result<Reference, Error>;
    /// Ends one withholding once the backend has recorded the requests sent
    /// so far. It always ends, whatever the remaining deadline.
    fn release_request_bodies(
        &mut self,
        context: Reference,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

fn evaluate(page: Reference, expression: &str) -> super::contract::Request {
    super::contract::Request::Evaluate {
        scope: page,
        expression: format!("async () => JSON.stringify(await ({expression})())"),
        is_function: Some(true),
        argument: Default::default(),
        return_handle: false,
    }
}
fn evaluated(value: &Value) -> Result<Value, Error> {
    serde_json::from_str(value["value"]["value"].as_str().ok_or("BACKEND_PROTOCOL")?)
        .map_err(|_| Error::from("BACKEND_PROTOCOL"))
}
/// The main document's origin and frame reference. A frame reference names
/// one document, so actions bound to it refuse after a navigation.
async fn document(
    pages: &mut impl Pages,
    page: Reference,
    deadline: Instant,
) -> Result<(String, Reference), Error> {
    let origin = evaluated(
        &pages
            .run(evaluate(page, "() => location.origin"), deadline)
            .await?,
    )?;
    let read = pages
        .run(
            super::contract::Request::PageRead {
                page,
                frame: None,
                locator: None,
                read: Read::Count,
                max_bytes: 4096,
            },
            deadline,
        )
        .await?;
    let frame =
        Reference::deserialize(&read["frame"]).map_err(|_| Error::from("BACKEND_PROTOCOL"))?;
    Ok((origin.as_str().ok_or("BACKEND_PROTOCOL")?.to_owned(), frame))
}
async fn visible(
    pages: &mut impl Pages,
    page: Reference,
    locator: &Locator,
) -> Result<bool, Error> {
    let probe = Instant::now() + PROBE;
    match pages
        .run(
            super::contract::Request::PageWait {
                page,
                frame: None,
                locator: Some(locator.clone()),
                condition: Condition::Visible {},
            },
            probe,
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.code,
                "CONDITION_TIMEOUT" | "NOT_DISPATCHED" | "TIMEOUT"
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
async fn text(
    pages: &mut impl Pages,
    page: Reference,
    locator: &Locator,
    deadline: Instant,
) -> Result<String, Error> {
    let read = pages
        .run(
            super::contract::Request::PageRead {
                page,
                frame: None,
                locator: Some(locator.clone()),
                read: Read::Text,
                max_bytes: 4096,
            },
            deadline,
        )
        .await?;
    Ok(read["result"]["value"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(MAX_LABEL_BYTES)
        .collect())
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    SignedIn,
    SignedOut,
    Unknown,
}
async fn state(pages: &mut impl Pages, page: Reference, journey: &Journey) -> Result<State, Error> {
    if visible(pages, page, &journey.signed_in).await? {
        return Ok(State::SignedIn);
    }
    if visible(pages, page, &journey.signed_out).await? {
        return Ok(State::SignedOut);
    }
    Ok(State::Unknown)
}
/// The identity text and whether it contains `expected`; null without an
/// identity reader.
async fn observed_identity(
    pages: &mut impl Pages,
    page: Reference,
    journey: &Journey,
    expected: Option<&str>,
    deadline: Instant,
) -> Result<Value, Error> {
    let Some(locator) = &journey.identity else {
        return Ok(Value::Null);
    };
    let observed = text(pages, page, locator, deadline).await?;
    Ok(
        json!({"matchesExpected":expected.map(|expected| observed.contains(expected)),"text":observed}),
    )
}
/// The identity result, or the refusal when it does not match `expected`.
/// Text that repeats an entered value is withheld.
async fn identity(
    pages: &mut impl Pages,
    page: Reference,
    journey: &Journey,
    expected: Option<&str>,
    entered: Option<&Values>,
    deadline: Instant,
) -> Result<Value, Error> {
    let mut observed = observed_identity(pages, page, journey, expected, deadline).await?;
    if let Some(text) = observed["text"].as_str() {
        let withheld = entered.is_some_and(|values| values.appears_in(text));
        observed["textWithheld"] = json!(withheld);
        if withheld {
            observed["text"] = Value::Null;
        }
    }
    if observed["matchesExpected"] == false {
        let mut error = Error::from("IDENTITY_MISMATCH");
        error.partial = Some(
            json!({"page":page,"identity":{"text":observed["text"],"textWithheld":observed["textWithheld"],"expected":expected}}),
        );
        return Err(error);
    }
    Ok(observed)
}

/// Read-only sign-in detection on the page's current document.
pub(crate) async fn status(
    pages: &mut impl Pages,
    page: Reference,
    journey: &Journey,
    expected: Option<&str>,
    deadline: Instant,
) -> Result<Value, Error> {
    validate_status(journey, expected)?;
    let (origin, frame) = document(pages, page, deadline).await?;
    if origin != journey.origin {
        return Ok(
            json!({"page":page,"frame":frame,"state":"other_origin","signedIn":Value::Null,"origin":origin,"sideEffects":false}),
        );
    }
    let state = state(pages, page, journey).await?;
    let identity = if state == State::SignedIn {
        observed_identity(pages, page, journey, expected, deadline).await?
    } else {
        Value::Null
    };
    Ok(json!({"page":page,"frame":frame,"origin":origin,
        "state":match state {State::SignedIn=>"signed_in",State::SignedOut=>"signed_out",State::Unknown=>"unknown"},
        "signedIn":match state {State::SignedIn=>json!(true),State::SignedOut=>json!(false),State::Unknown=>Value::Null},
        "identity":identity,"sideEffects":false}))
}

/// Entry into `frame`'s document, which refuses once it was replaced.
fn fill(
    page: Reference,
    frame: Reference,
    locator: &Locator,
    secret: &Secret,
) -> super::contract::Request {
    super::contract::Request::PageAction {
        page,
        frame: Some(frame),
        locator: locator.clone(),
        action: super::page::Action::Fill {
            value: secret.expose().to_owned(),
        },
        force: false,
        postcondition: None,
    }
}
fn click(page: Reference, frame: Reference, locator: &Locator) -> super::contract::Request {
    super::contract::Request::PageAction {
        page,
        frame: Some(frame),
        locator: locator.clone(),
        action: super::page::Action::Click {},
        force: false,
        postcondition: None,
    }
}
/// A failure during or after entry: its cause code without its message,
/// which could repeat an entered value.
fn entry_error(error: Error, step: &str, page: Reference) -> Error {
    let mut result = Error::from(error.code);
    result.partial = Some(
        json!({"page":page,"step":step,"submitted":!matches!(step,"username"|"password"|"submit"),"valuesReturned":false,"replayed":false,"nextAction":"auth_status"}),
    );
    result
}

/// The sign-in request after validation.
pub(crate) struct SignIn<'a> {
    pub page: Reference,
    pub broker: &'a str,
    pub account: &'a str,
    pub journey: &'a Journey,
    pub expected_identity: Option<&'a str>,
}
impl SignIn<'_> {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        validate_sign_in(
            self.broker,
            self.account,
            self.journey,
            self.expected_identity,
        )
    }
}
/// Admission checks of `auth_sign_in`.
pub(crate) fn validate_sign_in(
    broker: &str,
    account: &str,
    journey: &Journey,
    expected: Option<&str>,
) -> Result<(), &'static str> {
    validate_name(broker, "INVALID_BROKER_NAME")?;
    validate_name(account, "INVALID_ACCOUNT_NAME")?;
    validate_status(journey, expected)
}
/// Admission checks of `auth_status`.
pub(crate) fn validate_status(
    journey: &Journey,
    expected: Option<&str>,
) -> Result<(), &'static str> {
    journey.validate()?;
    validate_identity(expected)?;
    if expected.is_some() && journey.identity.is_none() {
        return Err("JOURNEY_IDENTITY_REQUIRED");
    }
    Ok(())
}
/// The refusal for a collection, named by `reason`, that could record
/// entered values.
pub(crate) fn secret_policy(page: Reference, reason: Option<&'static str>) -> Result<(), Error> {
    let Some(reason) = reason else {
        return Ok(());
    };
    let mut error = Error::from("SECRET_POLICY_UNENFORCEABLE");
    error.partial = Some(
        json!({"page":page,"reason":reason,"dispatched":false,"nextAction":"stop the collection, then retry"}),
    );
    Err(error)
}
/// `auth_broker_list`, `auth_sign_in` or `auth_status` through a backend's
/// owned pages, on the private broker directory.
pub(crate) async fn execute(
    pages: &mut impl Pages,
    request: super::contract::Request,
    deadline: Instant,
) -> Result<Value, Error> {
    use super::contract::Request;
    match request {
        Request::AuthBrokerList { cursor, limit } => {
            brokers(&broker_directory()?, cursor, limit, deadline).await
        }
        Request::AuthSignIn {
            page,
            broker,
            account,
            journey,
            expected_identity,
        } => {
            let request = SignIn {
                page,
                broker: &broker,
                account: &account,
                journey: &journey,
                expected_identity: expected_identity.as_deref(),
            };
            sign_in(pages, &broker_directory()?, request, deadline).await
        }
        Request::AuthStatus {
            page,
            journey,
            expected_identity,
        } => {
            status(
                pages,
                page,
                &journey,
                expected_identity.as_deref(),
                deadline,
            )
            .await
        }
        _ => Err("INTERNAL".into()),
    }
}

/// Signs `page` in unless it already shows the expected identity. Values
/// are resolved only for the journey's origin, entered only into the same
/// main document, and a navigation in between refuses before any entry.
/// Requests the context sends during entry keep their bodies withheld.
pub(crate) async fn sign_in(
    pages: &mut impl Pages,
    directory: &Path,
    request: SignIn<'_>,
    deadline: Instant,
) -> Result<Value, Error> {
    request.validate()?;
    let SignIn {
        page,
        broker,
        account,
        journey,
        expected_identity,
    } = request;
    pages.secret_policy(page)?;
    let (origin, frame) = document(pages, page, deadline).await?;
    if origin != journey.origin {
        let mut error = Error::from("ORIGIN_MISMATCH");
        error.partial = Some(json!({"page":page,"origin":origin,"expectedOrigin":journey.origin}));
        return Err(error);
    }
    match state(pages, page, journey).await? {
        State::SignedIn => {
            let identity =
                identity(pages, page, journey, expected_identity, None, deadline).await?;
            return Ok(
                json!({"page":page,"action":"none","signedIn":true,"identity":identity,"broker":broker,"account":account,"origin":origin,"valuesReturned":false}),
            );
        }
        State::Unknown => {
            let mut error = Error::from("NOT_ON_SIGN_IN");
            error.partial = Some(json!({"page":page,"origin":origin,"frame":frame}));
            return Err(error);
        }
        State::SignedOut => {}
    }
    let values = resolve(directory, broker, account, &journey.origin, deadline).await?;
    // The decision binds the document: a navigation while resolving refuses.
    let (current_origin, current) = document(pages, page, deadline).await?;
    if current_origin != journey.origin || current != frame {
        return Err("DOCUMENT_CHANGED".into());
    }
    pages.secret_policy(page)?;
    let context = pages.withhold_request_bodies(page)?;
    let entered = enter(pages, page, frame, journey, &values, deadline).await;
    pages.release_request_bodies(context).await;
    let otp_entered = entered?;
    let identity = identity(
        pages,
        page,
        journey,
        expected_identity,
        Some(&values),
        deadline,
    )
    .await;
    drop(values);
    let identity = identity?;
    Ok(
        json!({"page":page,"action":"signed_in","signedIn":true,"identity":identity,"broker":broker,"account":account,"origin":journey.origin,"otpEntered":otp_entered,"valuesReturned":false,"replayed":false}),
    )
}

/// Enters the values into the checked document `frame`, submits once and
/// classifies the outcome; whether a one-time code was entered.
async fn enter(
    pages: &mut impl Pages,
    page: Reference,
    frame: Reference,
    journey: &Journey,
    values: &Values,
    deadline: Instant,
) -> Result<bool, Error> {
    if let (Some(locator), Some(username)) = (&journey.username, &values.username) {
        pages
            .run(fill(page, frame, locator, username), deadline)
            .await
            .map_err(|error| entry_error(error, "username", page))?;
    }
    pages
        .run(
            fill(page, frame, &journey.password, &values.password),
            deadline,
        )
        .await
        .map_err(|error| entry_error(error, "password", page))?;
    pages
        .run(click(page, frame, &journey.submit), deadline)
        .await
        .map_err(|error| entry_error(error, "submit", page))?;
    let wait_until = deadline.min(Instant::now() + OUTCOME_WAIT);
    let mut otp_entered = false;
    loop {
        let step = if otp_entered {
            "otp_outcome"
        } else {
            "outcome"
        };
        if visible(pages, page, &journey.signed_in)
            .await
            .map_err(|error| entry_error(error, step, page))?
        {
            return Ok(otp_entered);
        }
        if let Some(failure) = &journey.failure
            && visible(pages, page, failure)
                .await
                .map_err(|error| entry_error(error, step, page))?
        {
            let message = text(pages, page, failure, deadline)
                .await
                .unwrap_or_default();
            let withheld = values.appears_in(&message);
            let mut error = Error::from("SIGN_IN_REFUSED");
            error.partial = Some(
                json!({"page":page,"submitted":true,"message":(!withheld).then_some(message),"messageWithheld":withheld,"valuesReturned":false,"replayed":false}),
            );
            return Err(error);
        }
        if let Some(otp) = &journey.otp
            && !otp_entered
            && visible(pages, page, otp)
                .await
                .map_err(|error| entry_error(error, step, page))?
        {
            let Some(code) = &values.otp else {
                let mut error = Error::from("MFA_REQUIRED");
                error.partial = Some(
                    json!({"page":page,"submitted":true,"brokerSuppliesOtp":false,"replayed":false}),
                );
                return Err(error);
            };
            // Submission can navigate anywhere: the code goes only to the
            // journey's origin, in the document showing the field.
            let (origin, shown) = document(pages, page, deadline)
                .await
                .map_err(|error| entry_error(error, step, page))?;
            if origin != journey.origin {
                let mut error = Error::from("ORIGIN_MISMATCH");
                error.partial = Some(
                    json!({"page":page,"step":"otp","submitted":true,"origin":origin,"expectedOrigin":journey.origin,"otpEntered":false,"valuesReturned":false,"replayed":false,"nextAction":"auth_status"}),
                );
                return Err(error);
            }
            pages
                .run(fill(page, shown, otp, code), deadline)
                .await
                .map_err(|error| entry_error(error, "otp", page))?;
            let submit = journey
                .otp_submit
                .as_ref()
                .ok_or("JOURNEY_OTP_INCOMPLETE")?;
            pages
                .run(click(page, shown, submit), deadline)
                .await
                .map_err(|error| entry_error(error, "otp_submit", page))?;
            otp_entered = true;
            continue;
        }
        if Instant::now() >= wait_until {
            let mut error = Error::from("SIGN_IN_OUTCOME_UNKNOWN");
            error.partial = Some(
                json!({"page":page,"submitted":true,"otpEntered":otp_entered,"valuesReturned":false,"replayed":false,"nextAction":"auth_status"}),
            );
            return Err(error);
        }
    }
}

pub(crate) fn capabilities(available: bool) -> Value {
    json!({"available":available,"operations":["auth_broker_list","auth_sign_in","auth_status"],
        "broker":{"protocol":PROTOCOL,"transport":if cfg!(unix) {"per-user-unix-socket"} else {"unsupported"},
            "directory":"testrun-data-root-brokers","providers":"external","testrunBroker":false,"fields":["username","password","otp"]},
        "originBinding":"exact-main-document-origin","otpOriginBinding":"rechecked-before-entry","documentBinding":"entry-bound-to-checked-document",
        "secretPolicy":["no-active-trace","no-har-capture","no-request-callbacks"],"requestBodies":"withheld-during-entry",
        "signOutDetection":"journey-signedOut-locator","identity":"journey-identity-text-contains-expected",
        "accountSwitching":false,"valuesInResults":false,"outcomeWaitMs":OUTCOME_WAIT.as_millis()})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
/// The journey's schema, shared by `auth_sign_in` and `auth_status` as one
/// definition.
pub(crate) fn journey_schema() -> Value {
    let locator = json!({"$ref":"#/$defs/browserLocator"});
    object(
        json!({"origin":{"type":"string","minLength":1,"maxLength":2048,"description":"The only origin that may receive values"},
            "username":locator,"password":locator,"submit":locator,
            "signedIn":locator,"signedOut":locator,"failure":locator,"otp":locator,"otpSubmit":locator,"identity":locator}),
        &["origin", "password", "submit", "signedIn", "signedOut"],
    )
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let name = |description: &str| json!({"type":"string","minLength":1,"maxLength":64,"pattern":"^[a-z0-9-]+$","description":description});
    let journey = json!({"$ref":"#/$defs/authJourney"});
    let identity = json!({"type":"string","minLength":1,"maxLength":256,"description":"Text the identity element must contain"});
    let page = crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD);
    vec![
        object(
            json!({"operation":{"const":"auth_broker_list"},"cursor":page["cursor"],"limit":page["limit"]}),
            &["operation"],
        ),
        object(
            json!({"operation":{"const":"auth_sign_in"},"page":reference,"broker":name("A broker from auth_broker_list"),"account":name("One of its accounts"),"journey":journey,"expectedIdentity":identity}),
            &["operation", "page", "broker", "account", "journey"],
        ),
        object(
            json!({"operation":{"const":"auth_status"},"page":reference,"journey":journey,"expectedIdentity":identity}),
            &["operation", "page", "journey"],
        ),
    ]
}

#[cfg(test)]
mod tests;
