//! Owned QA sessions workflows and platform coordination.
use super::arguments::{OpenSession, SessionRef, deadline};
use super::{Server, Session, id};
use crate::error::{Error, Result};
use crate::targets::Target;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

impl Server {
    #[cfg(unix)]
    pub(crate) async fn application_target(&mut self, args: Value) -> Result<String> {
        let reference = super::arguments::session_ref(&args)?;
        let target = self.live_target(&reference).await?;
        if !self.session(&reference)?.control {
            return Err(Error::new(
                "CONTROL_NOT_OWNED",
                "App management requires a controller",
            ));
        }
        Ok(target.identity.target_id)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Authorization and target preparation are one ordered acquisition transaction"
    )]
    pub(super) async fn open(&mut self, input: OpenSession) -> Result<Value> {
        if self.sessions.len() >= 64 {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Connection already owns 64 active sessions",
            ));
        }
        let run = self
            .runs
            .get(&input.run_id)
            .ok_or_else(|| Error::new("RUN_NOT_FOUND", "Run is not active"))?;
        if !run.goals.contains(&input.goal_id) {
            return Err(Error::new(
                "GOAL_NOT_DECLARED",
                "Goal does not belong to this run",
            ));
        }
        if input.mode != "control" && input.mode != "observe" {
            return Err(Error::invalid("mode must be control or observe"));
        }
        if input.mode == "control"
            && self
                .sessions
                .values()
                .any(|s| s.control && s.target.identity == input.target)
        {
            return Err(Error::new(
                "TARGET_BUSY",
                "Target already has a controlling session",
            ));
        }
        let kind = if input.target.target_id.starts_with("android:") {
            "android"
        } else if input.target.target_id.starts_with("device:") {
            "device"
        } else {
            "simulator"
        };
        let control_path = control_path(kind, &input.mode, input.control_path.as_deref())?;
        let android_capture = crate::android::streaming::Mode::parse(
            input.android_capture.as_ref(),
            kind == "android",
        )?;
        if android_capture == Some(crate::android::streaming::Mode::Stream)
            && input.mode != "control"
        {
            return Err(Error::invalid(
                "Streaming capture requires a controlling session",
            ));
        }
        let avd = kind == "android"
            && input
                .target
                .target_id
                .starts_with(crate::android::emulator::AVD_PREFIX);
        if input.boot && (!(kind == "simulator" || avd) || input.mode != "control")
            || input.setup && kind != "device"
            || input.window && !(avd && input.boot)
        {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Boot requires a simulator or AVD controller; window applies to a started AVD; setup applies to physical iOS only",
            ));
        }
        if avd && !input.boot {
            return Err(Error::new(
                "BOOT_REQUIRED",
                "An AVD target has no running instance; open it with boot to start one, or select its android:emulator- target",
            ));
        }
        deadline(input.timeout_ms)?;
        match kind {
            "android" => {
                self.discovery.refresh("android").await?;
            }
            "device" => {
                self.discovery.refresh("device").await?;
            }
            _ => {
                self.discovery.refresh("simulator").await?;
            }
        }
        let mut target = self.discovery.resolve(&input.target)?;
        let mut control_lock = if input.mode == "control" || input.setup {
            Some(crate::lease::ControlLease::acquire(
                &target.identity.target_id,
            )?)
        } else {
            None
        };
        if kind == "simulator" && target.state == "Shutdown" && input.boot {
            let directory = self
                .archive
                .as_ref()
                .and_then(super::journal::Archive::session_directory)
                .unwrap_or(&run.evidence);
            let booted = self
                .simulator_boots
                .start(&target, control_lock.take().unwrap(), directory)
                .await?;
            target = self
                .discovery
                .refresh("simulator")
                .await?
                .into_iter()
                .find(|current| current.identity.target_id == target.identity.target_id)
                .ok_or_else(|| Error::new("STALE_TARGET", "Simulator disappeared during boot"))?;
            if target.epoch != booted.epoch {
                return Err(Error::new(
                    "STALE_TARGET",
                    "Simulator boot changed during session admission",
                ));
            }
        }
        let avd_target = avd.then(|| target.identity.target_id.clone());
        if let Some(avd_target) = &avd_target {
            let directory = self
                .archive
                .as_ref()
                .and_then(super::journal::Archive::session_directory)
                .unwrap_or(&run.evidence)
                .to_path_buf();
            let started = self
                .emulator_starts
                .start(
                    &target,
                    control_lock.take().unwrap(),
                    &directory,
                    input.window,
                )
                .await;
            let serial_target = match started {
                Ok(serial_target) => serial_target,
                Err(error) => {
                    // A failed or partial start is stopped before the reply.
                    let mut error = error;
                    if let Err(cleanup) = self.emulator_starts.release_avd(avd_target).await {
                        let mut details = error.details.take().unwrap_or_else(|| json!({}));
                        details["cleanupError"] = json!(cleanup);
                        error.details = Some(details);
                    }
                    return Err(error);
                }
            };
            target = self
                .discovery
                .refresh("android")
                .await?
                .into_iter()
                .find(|current| current.identity.target_id == serial_target)
                .ok_or_else(|| {
                    Error::new("STALE_TARGET", "Started emulator is not listed by adb")
                })?;
            control_lock = Some(crate::lease::ControlLease::acquire(&serial_target)?);
        }
        let mut runner_setup = Value::Null;
        if kind == "device" {
            if input.setup {
                let executable = std::env::current_exe()
                    .map_err(|_| Error::new("INTERNAL", "Cannot locate bundled device runner"))?;
                let source = runner_source(
                    &executable,
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
                );
                runner_setup = self
                    .runners
                    .prepare(
                        &target.platform_id,
                        target.usb_id.ok_or_else(|| {
                            Error::new("USB_UNAVAILABLE", "Physical target has no USB connection")
                        })?,
                        input.runner_team_id.as_deref(),
                        &source,
                    )
                    .await?;
            }
            if input.mode == "control" && control_path == "xctest" || input.setup {
                let runner = self.runners.runner(&target.platform_id)?;
                let hello = runner
                    .call("hello", json!({}), None, Duration::from_secs(5))
                    .await?;
                for name in hello.data["capabilities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    // The runner reports its operations in its own vocabulary.
                    let name = match name {
                        "appManagement" => "app_management",
                        "longPress" => "long_press",
                        "text" => "type_text",
                        name => name,
                    };
                    crate::targets::capabilities::support(
                        &mut target,
                        name,
                        json!({"backend":"xctest"}),
                    );
                }
                crate::targets::capabilities::support(
                    &mut target,
                    "input",
                    json!({"backend":"xctest","scopes":["app","system"]}),
                );
                crate::targets::capabilities::support(
                    &mut target,
                    "type_text",
                    json!({"backend":"xctest","replace":false,"requiresVerifiedFocus":true,"elementTarget":true,"scopes":["app","system"]}),
                );
                if control_path == "xctest" {
                    target.capabilities.insert("key".into(), json!({"state":"unsupported","reason":"XCTest key chords are unavailable; select HID explicitly"}));
                }
                crate::targets::capabilities::support(
                    &mut target,
                    "screenshots",
                    json!({"backend":"xctest","profiles":["fast","evidence"]}),
                );
            }
            target.state = "Booted".into();
        }
        if target.state != "Booted" {
            return Err(Error::new(
                "TARGET_NOT_READY",
                "The target is not booted; connect with boot to start it",
            ));
        }
        if kind == "android" && input.mode == "control" {
            let adapter = crate::android::Android::default();
            let chords = adapter.key_chords(&target.platform_id).await;
            crate::targets::capabilities::android_key_chords(&mut target, chords);
            let api_level = adapter.api_level(&target.platform_id).await;
            crate::targets::capabilities::android_network(&mut target, api_level);
        }
        let session_id = id();
        let archive = if self.scoped_archive {
            Some(super::journal::Archive::for_session(
                &run.evidence,
                &session_id,
            )?)
        } else {
            None
        };
        let lease_id = id();
        #[cfg(target_os = "macos")]
        let (video, hid) = if control_path == "hid" {
            let source = self
                .device_media
                .acquire(&target.platform_id, &json!(target.identity))
                .await?;
            let admitted = async {
                source.snapshot().await?;
                source.open_input().await
            }
            .await;
            let hid = match admitted {
                Ok(hid) => hid,
                Err(error) => {
                    drop(source);
                    self.device_media
                        .release_unused_for(&target.platform_id)
                        .await?;
                    return Err(error);
                }
            };
            target.capabilities.insert(
                "input".into(),
                json!({"state":"supported","backend":"coredevice_hid","scope":"display"}),
            );
            crate::targets::capabilities::input(&mut target, "coredevice_hid");
            target.capabilities.insert(
                "screenshots".into(),
                json!({"state":"supported","profiles":if input.setup{vec!["fast","evidence"]}else{vec!["fast"]},"backend":"coredevice_video"}),
            );
            target.capabilities.insert(
                "preview".into(),
                json!({"state":"supported","formats":["hevc"],"backend":"coredevice_usb"}),
            );
            (Some(source), Some(hid))
        } else {
            (None, None)
        };
        #[cfg(not(target_os = "macos"))]
        if control_path == "hid" {
            return Err(Error::new("UNSUPPORTED_HOST", "HID control requires macOS"));
        }
        let mut response = json!({"sessionId":session_id,"runId":input.run_id,"goalId":input.goal_id,"state":"ready","target":target,"controlPath":control_path,"observationProfile":if control_path=="hid"{"fast"}else{"evidence"},"inputScope":if control_path=="hid"{Some(json!({"kind":"display"}))}else{None},"leaseId":if input.mode=="control" {Some(&lease_id)}else{None},"leaseVersion":1});
        if !runner_setup.is_null() {
            response["runnerSetup"] = runner_setup;
        }
        if kind == "simulator" {
            response["simulatorBoot"] = json!({"owned":self.simulator_boots.contains(&target.identity.target_id),"booted":target.state=="Booted","uiReadiness":"unknown"});
        }
        if let Some(avd_target) = &avd_target {
            response["emulatorStart"] = json!({"owned":self.emulator_starts.owns(&target.identity.target_id),"avdTargetId":avd_target,"targetId":target.identity.target_id,"dataPolicy":crate::android::emulator::DATA_POLICY,"window":input.window,"uiReadiness":"observation_ready"});
        }
        let reference = SessionRef {
            run_id: input.run_id.clone(),
            goal_id: input.goal_id.clone(),
            session_id: session_id.clone(),
        };
        let required = input.require_capabilities.clone();
        let android_capture = android_capture
            .filter(|mode| *mode == crate::android::streaming::Mode::Stream)
            .map(|mode| crate::android::streaming::Source::new(target.platform_id.clone(), mode));
        if let Some(source) = &android_capture
            && self.sessions.values().any(|session| {
                session.target.identity == target.identity
                    && session
                        .recordings
                        .iter()
                        .any(|id| self.recordings.active(id))
            })
        {
            source.disable("recording").await?;
        }
        if let Some(source) = &android_capture {
            response["androidCapture"] = source.status();
        } else if kind == "android" {
            response["androidCapture"] = json!({"requested":"screenshot","state":"screenshot"});
        }
        let mut session = Session {
            android_capture,
            archive,
            scheduler_slot: None,
            simulator_incarnation: std::sync::Arc::default(),
            simulator_input: std::sync::OnceLock::new(),
            simulator_source: std::sync::Arc::default(),
            read_gate: std::sync::Arc::default(),
            read_capabilities: std::sync::Arc::default(),
            closing: false,
            cleanup_error: None,
            cleanup: Default::default(),
            control_path,
            #[cfg(target_os = "macos")]
            video,
            #[cfg(target_os = "macos")]
            hid,
            batches: HashMap::new(),
            last_observation: std::sync::Arc::default(),
            _control_lock: control_lock,
            run_id: input.run_id,
            goal_id: input.goal_id,
            target,
            control: input.mode == "control",
            lease_id,
            journal: HashMap::new(),
            streams: HashSet::new(),
            recordings: HashSet::new(),
        };
        let admission = self.require_capabilities(&mut session, &required).await;
        self.sessions.insert(session_id.clone(), session);
        if let Err(error) = admission {
            self.close_session(&session_id).await.map_err(|cleanup| {
                Error::new(
                    "CLEANUP_FAILED",
                    "Capability admission failed and session cleanup is incomplete",
                )
                .details(json!({"cause":error,"cleanupError":cleanup}))
            })?;
            return Err(error);
        }
        response["target"] = json!(self.session(&reference)?.target);
        Ok(response)
    }

    pub(super) fn session(&self, reference: &SessionRef) -> Result<&Session> {
        let s = self
            .sessions
            .get(&reference.session_id)
            .ok_or_else(|| Error::new("SESSION_NOT_FOUND", "Session is not active"))?;
        if s.run_id != reference.run_id || s.goal_id != reference.goal_id {
            return Err(Error::new(
                "SESSION_NOT_OWNED",
                "Session belongs to another run or goal",
            ));
        }
        Ok(s)
    }

    pub(super) async fn live_target(&mut self, reference: &SessionRef) -> Result<Target> {
        let updates = self
            .session(reference)?
            .read_capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.sessions
            .get_mut(&reference.session_id)
            .unwrap()
            .target
            .capabilities
            .extend(updates);
        if self.session(reference)?.closing {
            return Err(Error::new(
                "SESSION_CLOSING",
                "Session cleanup is pending; inspect status or close again",
            ));
        }
        let kind = self.session(reference)?.target.kind.clone();
        if kind == "simulator" {
            let target = self.session(reference)?.target.clone();
            let validation = self.simulator_incarnation(reference, &target).await;
            if let Err(error) = validation {
                self.close_session(&reference.session_id).await?;
                return Err(error);
            }
            return Ok(target);
        }
        self.refresh_kind(&kind).await?;
        let resolved = self
            .discovery
            .resolve(&self.session(reference)?.target.identity);
        if resolved.is_err() {
            self.close_session(&reference.session_id).await?;
        }
        let mut target = resolved?;
        if target.kind == "android" {
            // Keep capabilities this session verified, such as key-chord support.
            target
                .capabilities
                .clone_from(&self.session(reference)?.target.capabilities);
        }
        if target.kind != "device" && target.state != "Booted" {
            self.close_session(&reference.session_id).await?;
            return Err(Error::new(
                "TARGET_NOT_READY",
                "Target is not booted; discover and open a new session",
            ));
        }
        Ok(target)
    }

    pub(super) async fn simulator_incarnation(
        &self,
        reference: &SessionRef,
        target: &Target,
    ) -> Result<crate::simulator::incarnation::Incarnation> {
        let session = self.session(reference)?;
        crate::simulator::device_set::validate_target(target)?;
        if let Some(identity) = session.simulator_incarnation.get() {
            identity.validate()?;
            return Ok(identity.clone());
        }
        let identity = crate::native::incarnation(&target.platform_id).await?;
        identity.matches_discovery(&target.epoch)?;
        let witness = session
            .simulator_incarnation
            .get_or_init(|| identity.clone());
        if witness != &identity {
            return Err(Error::new(
                "STALE_TARGET",
                "Simulator incarnation changed during concurrent admission",
            ));
        }
        witness.validate()?;
        Ok(witness.clone())
    }
    pub(super) async fn refresh_kind(&mut self, kind: &str) -> Result<()> {
        match kind {
            "android" => {
                self.discovery.refresh("android").await?;
            }
            "device" => {
                self.discovery.refresh("device").await?;
            }
            _ => {
                self.discovery.refresh("simulator").await?;
            }
        }
        Ok(())
    }

    pub(super) async fn status(&mut self, reference: SessionRef) -> Result<Value> {
        let session = self.session(&reference)?;
        if session.closing {
            return Ok(
                json!({"state":if session.cleanup_error.is_some(){"failed"}else{"closing"},"closed":false,"cleanupError":session.cleanup_error,"cleanup":session.cleanup}),
            );
        }
        let kind = self.session(&reference)?.target.kind.clone();
        self.refresh_kind(&kind).await?;
        let session = self.session(&reference)?;
        let resolved = self.discovery.resolve(&session.target.identity);
        match resolved {
            Ok(mut target) => {
                target.capabilities.clone_from(&session.target.capabilities);
                let runner_health = if target.kind == "device" {
                    match self.runners.runner(&target.platform_id) {
                        Ok(runner) => match runner
                            .call("hello", json!({}), None, Duration::from_secs(2))
                            .await
                        {
                            Ok(reply) => reply.data.get("inspection").cloned().unwrap_or(
                                json!({"state":"unknown","reason":"Runner does not report inspection health"}),
                            ),
                            Err(error) if error.code == "RUNNER_EXITED" && session.control_path == "xctest" => {
                                return Ok(json!({"state":"lost","reason":error.code,"cause":error,"target":target,"controlPath":session.control_path,"nextAction":"disconnect"}));
                            }
                            Err(error) => json!({"state":"unavailable","error":error}),
                        },
                        Err(error) if error.code == "RUNNER_EXITED" && session.control_path == "xctest" => {
                            return Ok(json!({"state":"lost","reason":error.code,"cause":error,"target":target,"controlPath":session.control_path,"nextAction":"disconnect"}));
                        }
                        Err(error) => json!({"state":"unavailable","error":error}),
                    }
                } else {
                    Value::Null
                };
                #[cfg(target_os = "macos")]
                let lock = match &session.video {
                    Some(video) => video.lock_status().await,
                    None => Value::Null,
                };
                #[cfg(not(target_os = "macos"))]
                let lock = Value::Null;
                let android_capture = session.android_capture.as_ref().map_or_else(
                    || {
                        if target.kind == "android" {
                            json!({"requested":"screenshot","state":"screenshot"})
                        } else {
                            Value::Null
                        }
                    },
                    |source| source.status(),
                );
                Ok(
                    json!({"state":"ready","target":target,"mode":if session.control {"control"} else {"observe"},"controlPath":session.control_path,"androidCapture":android_capture,"inputHealth":session_input_health(session),"inspectionHealth":runner_health,"lock":lock}),
                )
            }
            Err(error) => {
                self.close_session(&reference.session_id).await?;
                Ok(json!({"state":"lost","reason":error.code}))
            }
        }
    }
    pub(super) async fn close(&mut self, reference: SessionRef) -> Result<Value> {
        if self.sessions.contains_key(&reference.session_id) {
            self.session(&reference)?;
        }
        self.close_session(&reference.session_id).await?;
        Ok(json!({"closed":true,"cleanupFailures":[]}))
    }
}

fn runner_source(executable: &std::path::Path, source: &std::path::Path) -> std::path::PathBuf {
    executable
        .parent()
        .and_then(std::path::Path::parent)
        .map(|p| p.join("share/testrun/runner/ios"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| source.join("runner/ios"))
}

fn control_path(kind: &str, mode: &str, requested: Option<&str>) -> Result<String> {
    if kind != "device" {
        if requested.is_some() {
            return Err(Error::invalid(
                "controlPath applies to physical iOS devices only",
            ));
        }
        return Ok("native".into());
    }
    let path = requested.unwrap_or(if mode == "control" { "hid" } else { "xctest" });
    if !matches!(path, "hid" | "xctest") {
        return Err(Error::invalid("controlPath must be hid or xctest"));
    }
    if path == "hid" && mode != "control" {
        return Err(Error::invalid(
            "HID sessions require control mode; use observe mode for read-only inspection/video",
        ));
    }
    Ok(path.into())
}

fn session_input_health(_session: &Session) -> Value {
    #[cfg(target_os = "macos")]
    if let Some(hid) = &_session.hid {
        let mut health = hid.health();
        health["videoFresh"] = json!(
            _session
                .video
                .as_ref()
                .is_some_and(|s| s.geometry().is_ok())
        );
        return health;
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        Session {
            android_capture: None,
            archive: None,
            scheduler_slot: None,
            simulator_input: std::sync::OnceLock::new(),
            simulator_incarnation: std::sync::Arc::default(),
            simulator_source: std::sync::Arc::default(),
            read_gate: std::sync::Arc::default(),
            read_capabilities: std::sync::Arc::default(),
            closing: false,
            cleanup_error: None,
            cleanup: Default::default(),
            control_path: "native".into(),
            #[cfg(target_os = "macos")]
            video: None,
            #[cfg(target_os = "macos")]
            hid: None,
            _control_lock: None,
            run_id: "r".into(),
            goal_id: "g".into(),
            target: crate::targets::qa_fixture_target(),
            control: true,
            lease_id: "l".into(),
            journal: HashMap::new(),
            batches: HashMap::new(),
            last_observation: std::sync::Arc::default(),
            streams: HashSet::new(),
            recordings: HashSet::new(),
        }
    }
    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn permanent_source_failure_does_not_block_another_sessions_device_release() {
        let mut server = Server::new();
        let failed = format!("Acme-permanent-{}", uuid::Uuid::new_v4());
        let healthy = format!("Acme-independent-{}", uuid::Uuid::new_v4());
        for (id, serial, permanent) in [("failed", &failed, true), ("healthy", &healthy, false)] {
            let mut owned = session();
            owned.target.kind = "device".into();
            owned.target.platform_id = serial.clone();
            owned._control_lock = Some(crate::lease::ControlLease::acquire(serial).unwrap());
            owned.video = Some(if permanent {
                server.device_media.fixture_permanent_source_failure(serial)
            } else {
                server.device_media.fixture_failed_stream(serial)
            });
            server.sessions.insert(id.into(), owned);
        }
        let error = server.close_session("failed").await.unwrap_err();
        assert!(
            error
                .details
                .unwrap()
                .to_string()
                .contains("Fixture permanent source failure")
        );
        server.close_session("healthy").await.unwrap();
        assert!(!server.sessions.contains_key("healthy"));
        assert!(server.sessions["failed"].cleanup_error.is_some());
        assert!(
            crate::lease::ControlLease::inspect(&failed)
                .unwrap()
                .unwrap()
                .0
        );
        assert!(
            !crate::lease::ControlLease::inspect(&healthy)
                .unwrap()
                .unwrap()
                .0
        );
        assert!(
            !crate::lease::ControlLease::inspect(&format!("device-media:{healthy}"))
                .unwrap()
                .unwrap()
                .0
        );
        assert!(server.close_session("failed").await.is_err());
    }
    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn released_stream_failure_survives_successful_session_cleanup() {
        let mut server = Server::new();
        let serial = format!("Acme-stream-diagnostic-{}", uuid::Uuid::new_v4());
        let mut owned = session();
        owned.target.kind = "device".into();
        owned.target.platform_id = serial.clone();
        owned.video = Some(server.device_media.fixture_failed_stream(&serial));
        server.sessions.insert("s".into(), owned);
        server.close_session("s").await.unwrap();
        assert!(!server.sessions.contains_key("s"));
        let report = server.cleanup_status();
        assert_eq!(report[0]["complete"], true);
        assert_eq!(report[0]["progress"]["errors"], json!([]));
        assert_eq!(
            report[0]["progress"]["diagnostics"]["device_media"]["streamError"]["code"],
            "DEVICE_DISCONNECTED"
        );
        assert!(server.device_media.take_stream_error(&serial).is_none());
        assert!(
            !crate::lease::ControlLease::inspect(&format!("device-media:{serial}"))
                .unwrap()
                .unwrap()
                .0
        );
        server.shutdown_resumable().await.unwrap();
        assert_eq!(server.cleanup_status(), report);
    }
    #[tokio::test]
    async fn identity_admission_does_not_require_capture_or_input() {
        let (_fixture, identity) = crate::simulator::incarnation::tests::fixture();
        let mut session = session();
        session.target.kind = "simulator".into();
        session.simulator_incarnation.set(identity).unwrap();
        let expected = session.target.identity.clone();
        let mut server = Server::new();
        server.sessions.insert("s".into(), session);
        let reference = SessionRef {
            run_id: "r".into(),
            goal_id: "g".into(),
            session_id: "s".into(),
        };
        assert_eq!(
            server.live_target(&reference).await.unwrap().identity,
            expected
        );
        let session = server.session(&reference).unwrap();
        assert!(session.simulator_source.lock().unwrap().is_none());
        assert!(session.simulator_input.get().is_none());
    }
    #[tokio::test]
    async fn failed_close_remains_inspectable_and_cannot_turn_into_success() {
        let mut server = Server::new();
        let mut session = session();
        session.recordings.insert("missing-recording".into());
        server.sessions.insert("s".into(), session);
        let reference = || SessionRef {
            run_id: "r".into(),
            goal_id: "g".into(),
            session_id: "s".into(),
        };
        let first = server.close(reference()).await.unwrap_err();
        assert_eq!(first.code, "CLEANUP_FAILED");
        assert_eq!(
            first.details.as_ref().unwrap()["failures"][0]["code"],
            "NOT_FOUND"
        );
        assert_eq!(server.status(reference()).await.unwrap()["closed"], false);
        assert_eq!(
            server.live_target(&reference()).await.unwrap_err().code,
            "SESSION_CLOSING"
        );
        let again = server.close(reference()).await.unwrap_err();
        assert_eq!(again.details, first.details);
        // Closing an absent session cannot clear another session's failure.
        assert!(server.close_session("absent").await.is_ok());
        assert!(server.sessions.contains_key("s"));
    }
    #[tokio::test]
    async fn shutdown_finalizes_independent_runs_while_retaining_unverified_ownership() {
        let failed = tempfile::tempdir().unwrap();
        let healthy = tempfile::tempdir().unwrap();
        let mut server = Server::new();
        server.media = Some(crate::media::Media::start().await.unwrap());
        for (id, root) in [("r", failed.path()), ("healthy", healthy.path())] {
            server.runs.insert(
                id.into(),
                super::super::Run {
                    goals: HashSet::from(["g".into()]),
                    evidence: root.into(),
                    end_attempted: false,
                },
            );
        }
        let authority = format!("Acme-unverified-{}", uuid::Uuid::new_v4());
        let mut failed_session = session();
        failed_session.recordings.insert("missing-recording".into());
        failed_session._control_lock =
            Some(crate::lease::ControlLease::acquire(&authority).unwrap());
        server.sessions.insert("failed".into(), failed_session);
        let mut healthy_session = session();
        healthy_session.run_id = "healthy".into();
        server.sessions.insert("healthy".into(), healthy_session);
        assert_eq!(
            server.shutdown_resumable().await.unwrap_err().code,
            "CLEANUP_FAILED"
        );
        assert!(
            server.media.is_some(),
            "Resumable cleanup must retain shared owners"
        );
        assert!(healthy.path().join("summary.json").is_file());
        assert!(!failed.path().join("summary.json").exists());
        assert!(!server.runs.contains_key("healthy"));
        assert!(!server.sessions.contains_key("healthy"));
        assert!(server.sessions.contains_key("failed"));
        assert!(
            crate::lease::ControlLease::inspect(&authority)
                .unwrap()
                .unwrap()
                .0
        );
    }
    #[tokio::test]
    async fn exit_shutdown_closes_global_media_even_when_session_cleanup_fails() {
        let mut server = Server::new();
        let mut failed = session();
        failed.recordings.insert("missing-recording".into());
        server.sessions.insert("failed".into(), failed);
        server.media = Some(crate::media::Media::start().await.unwrap());
        let error = server.shutdown().await.unwrap_err();
        assert_eq!(error.code, "CLEANUP_FAILED");
        assert!(error.details.unwrap().to_string().contains("NOT_FOUND"));
        assert!(
            server.media.is_none(),
            "Exit must attempt global media cleanup"
        );
        assert!(server.sessions["failed"].cleanup_error.is_some());
        assert_eq!(server.cleanup_status()[0]["complete"], false);
    }
    #[test]
    fn runner_resources_resolve_for_local_and_source_installs() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("local");
        let source = root.path().join("checkout");
        let executable = prefix.join("bin/testrun");
        assert_eq!(
            runner_source(&executable, &source),
            source.join("runner/ios")
        );
        let local = prefix.join("share/testrun/runner/ios");
        std::fs::create_dir_all(&local).unwrap();
        assert_eq!(runner_source(&executable, &source), local);
    }

    #[tokio::test]
    async fn recovered_cleanup_does_not_hide_failure_on_run_end_or_exit() {
        for (exit, prior) in [(false, false), (false, true), (true, true)] {
            let mut server = Server::new();
            let run = server
                .call("run.begin", json!({"goalIds":["g"]}))
                .await
                .unwrap();
            let healthy = server
                .call("run.begin", json!({"goalIds":["g"]}))
                .await
                .unwrap();
            let summary =
                std::path::Path::new(run["evidenceDir"].as_str().unwrap()).join("summary.json");
            let mut failed = session();
            failed.run_id = run["runId"].as_str().unwrap().into();
            if prior {
                failed.cleanup_error = Some(Error::new(
                    "CLEANUP_FAILED",
                    "Fixture recording failed before release",
                ));
            } else {
                failed.recordings.insert("missing-recording".into());
            }
            server.sessions.insert("failed".into(), failed);
            let result = if exit {
                server.shutdown().await
            } else {
                server
                    .call("run.end", json!({"runId":run["runId"]}))
                    .await
                    .map(|_| ())
            };
            assert_eq!(result.unwrap_err().code, "CLEANUP_FAILED");
            assert_eq!(server.sessions.contains_key("failed"), !prior);
            assert!(
                !summary.exists(),
                "A recovered release must not certify the failed run"
            );
            if exit {
                assert!(
                    std::path::Path::new(healthy["evidenceDir"].as_str().unwrap())
                        .join("summary.json")
                        .is_file()
                );
            } else {
                if let Some(session) = server.sessions.get_mut("failed") {
                    session.recordings.clear();
                }
                server
                    .call("run.end", json!({"runId":run["runId"]}))
                    .await
                    .unwrap();
                assert!(
                    summary.is_file(),
                    "An explicit retry can finalize the settled run"
                );
                server.shutdown().await.unwrap();
            }
        }
    }

    #[test]
    fn physical_testing_defaults_to_hid_and_inspection_stays_explicit() {
        assert_eq!(control_path("device", "control", None).unwrap(), "hid");
        assert_eq!(control_path("device", "observe", None).unwrap(), "xctest");
        assert_eq!(
            control_path("device", "control", Some("xctest")).unwrap(),
            "xctest"
        );
        assert!(control_path("simulator", "control", Some("hid")).is_err());
        assert!(control_path("device", "observe", Some("hid")).is_err());
    }
}
