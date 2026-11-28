//! Blocking exchanges with the user's manager through
//! `canonic-process-kit`: finding or starting a compatible manager,
//! replacing one of another protocol, one-shot requests, control requests
//! and MCP attachments. Callers run these on Tokio's blocking pool.
use super::super::{
    PROTOCOL,
    host::{self, absent, draining, outcome_unknown, overloaded, unavailable},
    storage, wire,
};
use crate::error::{Error, Result};
use canonic_process_kit::{
    self as runtime, Error as Failure,
    client::{self, Options, Upgrade},
    endpoint::Root,
    launch::Spawn,
    net::Stream,
    protocol::{Code, Control, Identity, Reply, Request},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

/// The data root and whether this request may start an absent manager.
pub(super) struct Target {
    data: PathBuf,
    root: Root,
    start: bool,
}

fn upgrade_pending(holders: Value) -> Error {
    let mut error = Error::new(
        "OWNER_UPGRADE_PENDING",
        "A manager from another Testrun protocol is still busy; it drains and retires once its devices are released",
    )
    .details(json!({"admitted":false,"holders":holders}));
    error.retryable = true;
    error
}

/// A library request ID for Testrun's request ID: the same text when the
/// library accepts it, otherwise a stable digest. Deduplication keys on
/// Testrun's own ID inside the payload.
fn request_id(id: &Value) -> String {
    match id.as_str() {
        Some(id)
            if !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c)) =>
        {
            id.to_owned()
        }
        Some(id) => format!("t{:x}", Sha256::digest(id.as_bytes()))[..41].to_owned(),
        None => uuid::Uuid::new_v4().to_string(),
    }
}

/// The reply wait can outlive a device queue. Keep a limit when the caller
/// supplied one or when this is a CLI request.
pub(super) fn result_timeout(message: &Value) -> Option<Duration> {
    message["deadlineMs"]
        .as_u64()
        .map(|remaining| Duration::from_millis(remaining) + Duration::from_secs(15))
        .or_else(|| (message["client"]["kind"] != "mcp").then_some(Duration::from_secs(900)))
}

#[cfg(test)]
#[test]
fn result_wait_keeps_cli_and_explicit_budgets() {
    assert_eq!(result_timeout(&json!({"client":{"kind":"mcp"}})), None);
    assert_eq!(
        result_timeout(&json!({"client":{"kind":"cli"}})),
        Some(Duration::from_secs(900))
    );
    assert_eq!(
        result_timeout(&json!({"client":{"kind":"mcp"},"deadlineMs":2000})),
        Some(Duration::from_secs(17))
    );
}

fn options(io: Duration) -> Options {
    Options {
        io,
        frame_limit: wire::MAX_REQUEST_FRAME,
        ..Options::default()
    }
}

fn control(action: &str, manager: Option<&Identity>, arguments: Value) -> Control {
    Control {
        control_version: runtime::protocol::CONTROL_VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        action: action.into(),
        requires: vec![],
        manager_id: manager.map(|identity| identity.manager_id.clone()),
        arguments,
    }
}

impl Target {
    pub(super) fn new(data: &Path, start: bool) -> Result<Self> {
        Ok(Self {
            data: data.to_owned(),
            root: host::root(data)?,
            start,
        })
    }

    /// Start `testrun managed-owner` detached, with this client's environment:
    /// native tool paths come from the client that started the manager. The
    /// device set travels with each request instead.
    fn launcher(&self) -> Result<Spawn> {
        storage::directory(&self.data)?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.data.join("owner.log"))
            .map_err(|_| Error::new("HOST_IO", "Cannot open the manager log"))?;
        let mut args: Vec<OsString> = vec!["managed-owner".into(), "--root".into()];
        args.push(self.data.clone().into_os_string());
        Ok(Spawn {
            args,
            environment: std::env::vars_os()
                .filter(|(name, _)| name != "TESTRUN_SIMULATOR_DEVICE_SET")
                .collect(),
            log,
            directory: self.data.clone(),
        })
    }

    /// The running manager's identity and whether it is draining, starting
    /// one when allowed and waiting out one that is exiting. Any protocol is
    /// accepted, and a draining manager too: its control subset still
    /// answers whatever the protocol.
    fn reach_any(&self) -> Result<(Identity, bool)> {
        let status = match self.status()? {
            Some(status) => status,
            None => return self.start_or_absent().map(|identity| (identity, false)),
        };
        let lifetime = &status["lifetime"];
        let exiting = lifetime["state"] == "exiting"
            || (lifetime["state"] == "draining" && lifetime["work"] == 0 && lifetime["held"] == 0);
        if exiting {
            return self.start_or_absent().map(|identity| (identity, false));
        }
        let identity =
            serde_json::from_value(status["identity"].clone()).map_err(|_| unavailable())?;
        Ok((identity, lifetime["state"] == "draining"))
    }

    /// The running manager's status, or `None` when none serves the root.
    /// A held lease without a socket is a manager still starting: wait for
    /// it, within the readiness budget, instead of reporting it absent.
    fn status(&self) -> Result<Option<Value>> {
        let probe = options(Duration::from_secs(3));
        let deadline = std::time::Instant::now() + Options::default().readiness;
        let mut delay = Duration::from_millis(2);
        loop {
            match client::control(&self.root, &control("status", None, Value::Null), &probe) {
                Ok(reply) if reply.code.is_none() => return Ok(Some(reply.data)),
                Ok(_) => return Err(unavailable()),
                Err(Failure::Starting) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_millis(100));
                }
                Err(Failure::Starting) => return Err(unavailable()),
                Err(Failure::Absent | Failure::Draining) => return Ok(None),
                Err(error) => return Err(host::failure(&error)),
            }
        }
    }

    /// Start the manager and wait for its readiness, or report it absent.
    fn start_or_absent(&self) -> Result<Identity> {
        if !self.start {
            return Err(absent());
        }
        let launcher = self.launcher()?;
        client::connect(&self.root, Some(&launcher), &Options::default()).map_err(|error| {
            match error {
                Failure::Draining => draining(),
                Failure::Starting => unavailable(),
                Failure::Exited(status) => host::exited(status, &self.data.join("owner.log")),
                error => host::failure(&error),
            }
        })
    }

    /// A manager of this protocol with `requires`, replacing one of another
    /// protocol when this request may start one: an empty one retires, a
    /// busy one is asked to drain.
    fn reach(&self, requires: &[String]) -> Result<Identity> {
        for _ in 0..3 {
            let (identity, draining_now) = self.reach_any()?;
            if identity.protocol != PROTOCOL {
                if !self.start {
                    return Err(Error::new(
                        "OWNER_INCOMPATIBLE",
                        "The running manager uses another protocol; retire it when empty or drain it",
                    )
                    .details(json!({"admitted":false,"identity":identity})));
                }
                // Already draining for an earlier replacement: wait for it.
                if draining_now {
                    return Err(upgrade_pending(Value::Null));
                }
                self.replace(requires)?;
                continue;
            }
            // A draining manager admits no new request; its successor will.
            if draining_now {
                return Err(draining());
            }
            if identity.require(PROTOCOL, requires).is_err() {
                let missing = requires
                    .iter()
                    .find(|capability| !identity.capabilities.contains(capability))
                    .cloned()
                    .unwrap_or_default();
                return Err(Error::new(
                    "CAPABILITY_MISSING",
                    format!("The manager does not provide {missing}"),
                )
                .details(json!({"admitted":false,"identity":identity})));
            }
            return Ok(identity);
        }
        Err(unavailable())
    }

    fn replace(&self, requires: &[String]) -> Result<()> {
        let probe = options(Duration::from_secs(3));
        match client::upgrade(&self.root, PROTOCOL, requires, &probe) {
            Ok(Upgrade::Retired) | Err(Failure::StaleManager) => Ok(()),
            Ok(Upgrade::Busy {
                compatible: true, ..
            }) => Ok(()),
            Ok(Upgrade::Busy {
                identity, blockers, ..
            }) => {
                let drained = client::control(
                    &self.root,
                    &control("drain", Some(&identity), Value::Null),
                    &probe,
                );
                match drained {
                    Ok(_) | Err(Failure::Draining) => Err(upgrade_pending(blockers)),
                    Err(error) => Err(host::failure(&error)),
                }
            }
            Err(Failure::Absent | Failure::Starting | Failure::Draining) => Ok(()),
            Err(error) => Err(host::failure(&error)),
        }
    }

    /// Deliver one product request once. Absence, a manager that is exiting
    /// and a refusal before admission are definite non-admission and allow
    /// another attempt with the same request ID; a lost acknowledgement or
    /// result never does.
    pub(super) fn submit(&self, message: &Value) -> Result<client::Admitted> {
        let requires: Vec<String> = message["requires"]
            .as_array()
            .map(|requires| {
                requires
                    .iter()
                    .filter_map(|capability| capability.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let mut request = Request {
            protocol: PROTOCOL,
            request_id: request_id(&message["requestId"]),
            requires,
            payload: message.clone(),
        };
        // An older manager reads the frame length before its capabilities
        // can be checked. Preflight only requests that need a larger frame.
        if matches!(
            canonic_mcp_kit::json::measure(&request, wire::LEGACY_MAX_FRAME, &|| Ok(())),
            Err(canonic_mcp_kit::json::EncodeError::TooLarge { .. })
        ) {
            if !request
                .requires
                .iter()
                .any(|capability| capability == wire::LARGE_FIXTURES)
            {
                request.requires.push(wire::LARGE_FIXTURES.into());
            }
            self.reach(&request.requires)?;
        }
        // Legacy-sized requests keep the direct send path. After either
        // admission path, retry only after definite non-admission.
        let admission_timeout = result_timeout(message).unwrap_or_else(|| Options::default().io);
        for _ in 0..3 {
            match client::submit(&self.root, &request, &options(admission_timeout)) {
                Ok(admitted) => return Ok(admitted),
                Err(
                    Failure::Draining | Failure::Absent | Failure::Starting | Failure::Incompatible,
                ) => {
                    self.reach(&request.requires)?;
                }
                Err(Failure::Refused(reply)) if reply.code == Some(Code::FrameTooLarge) => {
                    // The manager may have been replaced after preflight.
                    return Err(Error::new(
                        "INPUT_LIMIT",
                        "The manager refused the request frame size",
                    )
                    .details(json!({"admitted":false,"dispatched":false})));
                }
                Err(Failure::Refused(reply)) => {
                    return Err(result(&reply).err().unwrap_or_else(unavailable));
                }
                Err(failure) => {
                    return Err(outcome(Err(failure), &message["requestId"])
                        .err()
                        .unwrap_or_else(unavailable));
                }
            }
        }
        Err(unavailable())
    }

    /// One control, served by managers of any protocol; inspection carries
    /// a product request, whose protocol the manager checks. Extensions go
    /// straight to the manager; retire and drain name its incarnation.
    pub(super) fn control(&self, action: &str, arguments: Value) -> Result<Value> {
        let probe = options(Duration::from_secs(60));
        if !matches!(action, "retire" | "drain") {
            let request = control(action, None, arguments);
            let first = client::control(&self.root, &request, &probe);
            let reply = match first {
                Err(Failure::Absent | Failure::Starting | Failure::Draining) => {
                    // Start or wait for a manager when this request may,
                    // otherwise report it absent.
                    self.reach_any()?;
                    client::control(&self.root, &request, &probe)
                }
                reply => reply,
            };
            return match reply {
                Ok(reply) => result(&reply),
                Err(failure) => Err(host::failure(&failure)),
            };
        }
        let (identity, _) = self.reach_any()?;
        let (outcome, holders) = if action == "retire" {
            let reply = self.built_in("retire_if_empty", &identity, &probe)?;
            if reply["retired"] == true {
                return Ok(json!({"retired":true}));
            }
            (("retired", false), reply["blockers"].clone())
        } else {
            self.built_in("drain", &identity, &probe)?;
            let status = self.built_in("status", &identity, &probe)?;
            (("draining", true), status["blockers"].clone())
        };
        let mut holders = if holders.is_object() {
            holders
        } else {
            json!({})
        };
        holders[outcome.0] = json!(outcome.1);
        Ok(holders)
    }

    fn built_in(&self, action: &str, identity: &Identity, probe: &Options) -> Result<Value> {
        match client::control(
            &self.root,
            &control(action, Some(identity), Value::Null),
            probe,
        ) {
            Ok(reply) if reply.code.is_none() => Ok(reply.data),
            Ok(reply) if reply.code == Some(Code::StaleManager) => Err(absent()),
            Ok(reply) => result(&reply),
            // The manager exited meanwhile: nothing to retire or drain.
            Err(Failure::Absent | Failure::Draining) => Err(absent()),
            Err(error) => Err(host::failure(&error)),
        }
    }

    /// Open an MCP server's attachment: a handed-off connection the manager
    /// keeps until its EOF.
    pub(super) fn attach(&self, connection: &str) -> Result<Stream> {
        let request = Request {
            protocol: PROTOCOL,
            request_id: uuid::Uuid::new_v4().to_string(),
            requires: vec![],
            payload: json!({"kind":"attach","client":{"kind":"mcp","connection":connection}}),
        };
        for attempt in 0..3 {
            if attempt > 0 {
                self.reach(&[])?;
            }
            match client::open(&self.root, &request, &options(Duration::from_secs(10))) {
                Ok((_, stream)) => return Ok(stream),
                Err(Failure::HandoffRefused(reply)) => {
                    return Err(match reply.code {
                        Some(Code::Product) => wire::error(&reply.data),
                        Some(Code::Draining) => draining(),
                        _ => unavailable(),
                    });
                }
                Err(
                    Failure::Draining | Failure::Absent | Failure::Starting | Failure::Incompatible,
                ) => {}
                Err(Failure::Capacity) => return Err(overloaded()),
                Err(error) => return Err(host::failure(&error)),
            }
        }
        Err(unavailable())
    }
}

/// The product outcome of an admitted request's result.
pub(super) fn outcome(
    reply: std::result::Result<Reply, Failure>,
    request_id: &Value,
) -> Result<Value> {
    match reply {
        Ok(reply) => result(&reply),
        Err(Failure::Capacity) => Err(overloaded()),
        Err(Failure::OutcomeUnknown(_)) => Err(outcome_unknown(request_id)),
        Err(error) => Err(host::failure(&error)),
    }
}

/// A result frame's product outcome.
fn result(reply: &Reply) -> Result<Value> {
    match reply.code {
        None => Ok(reply.data.clone()),
        Some(Code::Product) => Err(wire::error(&reply.data)),
        Some(Code::UnknownAction) => Err(Error::new(
            "OWNER_PROTOCOL",
            "The running manager does not provide this control",
        )),
        Some(Code::FrameTooLarge) => Err(Error::new(
            "OUTPUT_LIMIT",
            "The manager's reply exceeded the frame limit",
        )),
        Some(code) => Err(Error::new(
            "OWNER_PROTOCOL",
            format!("The manager refused the request: {code:?}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_process_kit::{
        core::{Admission, IdleGrace},
        endpoint::Lease,
        protocol::ProductError,
        server::{self, Context, Handler},
    };
    use std::sync::Arc;

    struct MixedVersionBrowser;
    impl Handler for MixedVersionBrowser {
        fn call(
            &self,
            request: &Request,
            _: &Context<'_>,
        ) -> std::result::Result<Value, ProductError> {
            if request.payload["name"] == "browser_status" {
                return Ok(
                    json!({"operation":{"result":{"body":"x".repeat(wire::MAX_REPLY_FRAME)}}}),
                );
            }
            let args = &request.payload["args"]["request"];
            let argument = args["fixture"]["dataBase64"]
                .as_str()
                .or_else(|| args["argument"]["value"].as_str())
                .unwrap_or_default();
            Ok(json!({"bytes":argument.len()}))
        }
    }

    fn register_large_fixture(target: &Target) -> Result<Value> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let message = json!({
            "requestId":uuid::Uuid::new_v4(),
            "requires":["device_queues",wire::LARGE_FIXTURES],
            "name":"browser_send",
            "args":{"request":{"operation":"fixture_register","fixture":{
                "name":"acme.bin","mimeType":"application/octet-stream",
                "dataBase64":STANDARD.encode(vec![42; crate::browser::transfer::FIXTURE_BYTES]),
                "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Generated"}
            }}}
        });
        let admitted = target.submit(&message)?;
        outcome(admitted.result(), &message["requestId"])
    }

    fn legacy_browser_poll(target: &Target) -> runtime::Result<Reply> {
        client::call(
            &target.root,
            &Request {
                protocol: PROTOCOL,
                request_id: "acme-legacy-poll".into(),
                requires: vec!["device_queues".into()],
                payload: json!({"name":"browser_status","args":{"operationId":uuid::Uuid::new_v4()}}),
            },
            &Options {
                frame_limit: wire::LEGACY_MAX_FRAME,
                ..Options::default()
            },
        )
    }

    #[tokio::test]
    async fn oversized_manager_requests_negotiate_before_frame_dispatch() {
        for (request_frame_limit, extended) in [
            (wire::LEGACY_MAX_FRAME, false),
            (wire::MAX_REQUEST_FRAME, true),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("state");
            let lease = Lease::acquire(&host::root(&path).unwrap(), Duration::ZERO).unwrap();
            let mut capabilities = vec!["device_queues".into()];
            if extended {
                capabilities.push(wire::LARGE_FIXTURES.into());
            }
            let identity = Identity::new("acme".into(), PROTOCOL, capabilities).unwrap();
            let gate = Admission::new();
            let hold = gate.hold().unwrap();
            let serving = gate.clone();
            let running = tokio::task::spawn_blocking(move || {
                server::serve_with_request_limit(
                    lease,
                    identity,
                    serving,
                    Arc::new(MixedVersionBrowser),
                    &IdleGrace(Duration::from_secs(1)),
                    server::Options {
                        frame_limit: wire::MAX_REPLY_FRAME,
                        ..server::Options::default()
                    },
                    request_frame_limit,
                )
            });
            host::wait_for_fixture_socket(&path, &running).await;
            let (small, large, fixture, legacy_poll) = tokio::task::spawn_blocking(move || {
                let target = Target::new(&path, false).unwrap();
                let send = |argument: String| {
                    let connection = uuid::Uuid::new_v4();
                    let message = json!({
                        "requestId":uuid::Uuid::new_v4(),
                        "requires":["device_queues"],
                        "name":"browser_send",
                        "args":{
                            "targetId":format!("browser:{connection}"),
                            "request":{
                                "operation":"evaluate","expression":"value => value.length",
                                "scope":{"connectionId":connection,"id":uuid::Uuid::new_v4(),
                                    "generation":uuid::Uuid::new_v4()},
                                "argument":{"kind":"json","value":argument}
                            }
                        },
                    });
                    let admitted = target.submit(&message)?;
                    outcome(admitted.result(), &message["requestId"])
                };
                (
                    send("acme".into()),
                    send("x".repeat(wire::LEGACY_MAX_FRAME)),
                    register_large_fixture(&target),
                    legacy_browser_poll(&target),
                )
            })
            .await
            .unwrap();
            let _ = gate.drain();
            drop(hold);
            running.await.unwrap().unwrap();

            assert_eq!(small.unwrap()["bytes"], 4);
            let reply = legacy_poll.unwrap();
            assert_eq!(reply.accepted, None);
            assert_eq!(reply.code, Some(Code::FrameTooLarge));
            assert_eq!(result(&reply).unwrap_err().code, "OUTPUT_LIMIT");
            if extended {
                assert_eq!(large.unwrap()["bytes"], wire::LEGACY_MAX_FRAME);
                assert_eq!(
                    fixture.unwrap()["bytes"],
                    crate::browser::transfer::FIXTURE_BYTES.div_ceil(3) * 4
                );
            } else {
                for refused in [large, fixture] {
                    let error = refused.unwrap_err();
                    assert_eq!(error.code, "CAPABILITY_MISSING");
                    assert_eq!(error.details.unwrap()["admitted"], false);
                }
            }
        }
    }

    #[test]
    fn library_request_ids_keep_valid_ids_and_digest_others() {
        let valid = json!("mcp-0123abcd");
        assert_eq!(request_id(&valid), "mcp-0123abcd");
        let other = request_id(&json!("acme build 1/2"));
        assert_eq!(other.len(), 41);
        assert_eq!(other, request_id(&json!("acme build 1/2")));
        assert!(other.bytes().all(|c| c.is_ascii_alphanumeric()));
    }
}
