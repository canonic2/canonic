//! Bounded, prevalidated action sequences with journals and final or per-step observations.
use super::Server;
#[cfg(test)]
use super::Session;
use super::arguments::{SessionRef, field, only_fields, session_ref};
#[cfg(test)]
use super::journal;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use std::time::Instant;

use crate::catalog::BATCH_TOOLS;

/// Default batch deadline from admission; managed act batches use it.
pub(crate) const BATCH_TIMEOUT_MS: u64 = 120_000;
/// Most time one call may spend in requested delays and gestures, leaving the
/// rest of the deadline for dispatch, settling and screenshots.
const DURATION_BUDGET_MS: u64 = 90_000;

/// A delay holds time between steps and sends nothing to the device.
pub(crate) fn delay_step(name: &str) -> bool {
    name == "delay"
}

/// Refuses a call whose delays and gesture durations exceed 90 s or `timeout_ms`,
/// naming the step that crosses the limit.
pub(crate) fn duration_budget<'a>(
    steps: impl IntoIterator<Item = (&'a str, &'a Value)>,
    timeout_ms: u64,
) -> Result<()> {
    let limit = DURATION_BUDGET_MS.min(timeout_ms);
    let mut total = 0_u64;
    for (index, (name, arguments)) in steps.into_iter().enumerate() {
        if !matches!(name, "delay" | "swipe" | "long_press") {
            continue;
        }
        total = total.saturating_add(arguments["durationMs"].as_u64().unwrap_or(0));
        if total > limit {
            return Err(Error::invalid(format!(
                "Delays and gesture durations total {total} ms, over the {limit} ms limit for one call"
            ))
            .at_action(index, Some(name)));
        }
    }
    Ok(())
}

/// Admits a batch step; Android sessions have no preview or recording steps.
fn step_operation(name: &str, android: bool) -> Result<String> {
    if !BATCH_TOOLS.contains(&name) {
        return Err(Error::invalid("Operation is not allowed in a batch"));
    }
    if android && !crate::catalog::android_batch_tool(name) {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Android batches have no preview or recording steps; call the recording tools directly",
        ));
    }
    Ok(name.to_owned())
}

fn prepare(args: &Value, android: bool) -> Result<Vec<(String, Value)>> {
    let definitions = crate::catalog::internal_tools();
    let steps = args["steps"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 8)
        .ok_or_else(|| Error::invalid("A batch requires 1..8 steps"))?;
    let id = field(args, "commandId")?;
    if id.len() > 96 {
        return Err(Error::invalid("Batch commandId exceeds 96 bytes"));
    }
    steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            (|| {
                only_fields(step, &["tool", "arguments"])?;
                let name = step_operation(field(step, "tool")?, android)?;
                if delay_step(&name) {
                    // A delay carries no authority, scope or command identity.
                    let input = step["arguments"].clone();
                    crate::schema::validate(&crate::catalog::delay_schema(), &input)?;
                    return Ok((name, input));
                }
                let definition = definitions
                    .iter()
                    .find(|t| t["name"] == name.as_str())
                    .ok_or_else(|| Error::invalid("Batch operation is not registered"))?;
                let properties = &definition["inputSchema"]["properties"];
                let mut input = step["arguments"]
                    .as_object()
                    .cloned()
                    .ok_or_else(|| Error::invalid("Step arguments must be an object"))?;
                for key in ["runId", "goalId", "sessionId", "leaseId", "commandId"] {
                    if input.contains_key(key) {
                        return Err(Error::invalid(
                            "Step arguments cannot replace session authority or command identity",
                        ));
                    }
                    if properties.get(key).is_some() {
                        input.insert(
                            key.into(),
                            if key == "commandId" {
                                json!(format!("{id}:{index}"))
                            } else {
                                args[key].clone()
                            },
                        );
                    }
                }
                for key in ["scope", "display"] {
                    if properties.get(key).is_some()
                        && !input.contains_key(key)
                        && args.get(key).is_some()
                    {
                        input.insert(key.into(), args[key].clone());
                    }
                }
                let input = Value::Object(input);
                crate::schema::validate(&definition["inputSchema"], &input)?;
                Ok((name, input))
            })()
            .map_err(|error: Error| error.at_action(index, step["tool"].as_str()))
        })
        .collect::<Result<Vec<_>>>()
        .and_then(|steps| {
            duration_budget(
                steps.iter().map(|(name, input)| (name.as_str(), input)),
                args["timeoutMs"].as_u64().unwrap_or(BATCH_TIMEOUT_MS),
            )?;
            Ok(steps)
        })
}

impl Server {
    #[expect(
        clippy::too_many_lines,
        reason = "Batch journal, admission, stop-on-error and final observation share one bounded transaction"
    )]
    pub(super) async fn batch(&mut self, args: Value) -> Result<Value> {
        only_fields(
            &args,
            &[
                "runId",
                "goalId",
                "sessionId",
                "leaseId",
                "commandId",
                "scope",
                "display",
                "steps",
                "observe",
                "observeAfterEach",
                "observationProfile",
                "settle",
                "timeoutMs",
            ],
        )?;
        let reference = session_ref(&args)?;
        let command_id = field(&args, "commandId")?.to_owned();
        let signature = json!(format!("{:x}", Sha256::digest(args.to_string().as_bytes())));
        let session = self.session(&reference)?;
        if session.closing {
            return Err(Error::new(
                "SESSION_CLOSING",
                "Session cleanup has started; no batch was admitted",
            ));
        }
        session
            .authority()
            .admit(field(&args, "leaseId")?, std::time::Instant::now())?;
        if !session.control {
            return Err(Error::new(
                "CONTROL_NOT_OWNED",
                "Batch requires the current controller lease",
            ));
        }
        if let Some((previous, result)) = session.batches.get(&command_id) {
            if previous != &signature {
                return Err(Error::new(
                    "COMMAND_ID_CONFLICT",
                    "Batch command ID has different arguments",
                ));
            }
            return batch_result(result.clone());
        }
        if session.batches.len() >= 1000 {
            return Err(Error::new("RESOURCE_LIMIT", "Batch journal is full"));
        }
        let timeout = match args.get("timeoutMs") {
            None => BATCH_TIMEOUT_MS,
            Some(value) => value
                .as_u64()
                .filter(|ms| (1..=BATCH_TIMEOUT_MS).contains(ms))
                .ok_or_else(|| Error::invalid("timeoutMs must be an integer in 1..120000"))?,
        };
        let steps = prepare(&args, session.target.kind == "android")?;
        let each = args["observeAfterEach"] == true;
        let settle = match args.get("settle") {
            None => true,
            Some(Value::Bool(settle)) => *settle,
            Some(_) => return Err(Error::invalid("settle must be a boolean")),
        };
        if each && args["observe"] == false {
            return Err(Error::invalid("observeAfterEach requires observe"));
        }
        let profile = args
            .get("observationProfile")
            .cloned()
            .unwrap_or(json!(if each || session.control_path == "hid" {
                "fast"
            } else {
                "evidence"
            }));
        if profile != "fast" && profile != "evidence" {
            return Err(Error::invalid(
                "Observation profile must be evidence or fast",
            ));
        }
        if session
            .batches
            .values()
            .any(|(_, result)| result["status"] == "outcome_unknown")
            && steps.iter().any(|(name, _)| {
                matches!(
                    name.as_str(),
                    "tap"
                        | "swipe"
                        | "long_press"
                        | "key"
                        | "type_text"
                        | "app_launch"
                        | "app_terminate"
                        | "app_restart"
                )
            })
        {
            return Err(Error::new(
                "SESSION_UNCERTAIN",
                "A prior batch has an unknown outcome; inspect it before reacquiring control",
            ));
        }
        // Typed geometry and explicit app scope are validated before admission;
        // the fresh physical geometry and app bounds are checked at dispatch.
        for (index, (name, input)) in steps.iter().enumerate() {
            if delay_step(name) {
                continue;
            }
            let mutation = crate::mobile::Mutation {
                operation: name.clone(),
                arguments: input.clone(),
                target: session.target.clone(),
                command_id: field(input, "commandId").unwrap_or("").into(),
                timeout: Duration::from_secs(30),
            };
            let validation = (|| {
                if session.control_path == "hid"
                    && matches!(
                        name.as_str(),
                        "tap" | "swipe" | "long_press" | "key" | "type_text"
                    )
                {
                    #[cfg(target_os = "macos")]
                    crate::hid::validate(name, input)?;
                } else {
                    crate::mobile::validate(&mutation)?;
                }
                Ok(())
            })();
            validation.map_err(|error: Error| error.at_action(index, Some(name)))?;
        }
        let start = Instant::now();
        let mut result = json!({"commandId":command_id,"status":"outcome_unknown","steps":[],"observation":null,"observationToDispatchMs":session.last_observation.lock().unwrap_or_else(|e|e.into_inner()).map(|time|time.elapsed().as_millis())});
        self.record_batch(
            &reference.session_id,
            command_id.clone(),
            signature.clone(),
            result.clone(),
        )?;
        let deadline = start + Duration::from_millis(timeout);
        let lease = field(&args, "leaseId")?.to_owned();
        let step_count = steps.len();
        let mut baseline = None;
        let mut baseline_taken = false;
        for (index, (name, input)) in steps.into_iter().enumerate() {
            let step_start = Instant::now();
            let start_ms = step_start.duration_since(start).as_millis();
            let held = delay_step(&name)
                .then(|| Duration::from_millis(input["durationMs"].as_u64().unwrap_or(0)));
            let expired = past_deadline(step_start, held, deadline);
            let preflight = if !expired
                && settle
                && args["observe"] != false
                && !baseline_taken
                && input_step(&name)
            {
                match tokio::time::timeout(
                    deadline.saturating_duration_since(Instant::now()),
                    self.observation_baseline(&reference),
                )
                .await
                {
                    Ok(Ok(sample)) => {
                        baseline = sample;
                        baseline_taken = true;
                        Ok(())
                    }
                    result => {
                        let cause = match result {
                            Ok(Err(error)) => error,
                            Err(_) => Error::new(
                                "TIMEOUT",
                                "Batch deadline expired while reading its input baseline",
                            ),
                            Ok(Ok(_)) => unreachable!(),
                        };
                        Err(Error::new("BASELINE_UNAVAILABLE", "Input was not sent because its observation baseline could not be acquired")
                            .details(json!({"stage":"observation_baseline","inputDispatched":false,"cause":cause})))
                    }
                }
            } else {
                Ok(())
            };
            let expired = expired || past_deadline(Instant::now(), held, deadline);
            let dispatched = preflight.is_ok() && !expired && held.is_none();
            let output = if let Err(error) = preflight {
                Err(error)
            } else if expired {
                Err(Error::new(
                    "TIMEOUT",
                    format!(
                        "The {timeout} ms batch deadline would pass before this step completed; it was not sent"
                    ),
                ))
            } else if let Some(held) = held {
                self.hold(&reference, &lease, held).await
            } else {
                Box::pin(self.call(&name, input)).await
            };
            let mut entry = match output {
                Ok(data) => {
                    json!({"index":index,"tool":name,"ok":true,"startMs":start_ms,"elapsedMs":step_start.elapsed().as_millis(),"data":data})
                }
                Err(error) => {
                    // Only a dispatched input can leave its outcome unknown.
                    result["status"] = json!(if dispatched
                        && matches!(
                            error.code.as_str(),
                            "OUTCOME_UNKNOWN"
                                | "TIMEOUT"
                                | "DEVICE_DISCONNECTED"
                                | "SESSION_UNCERTAIN"
                        ) {
                        "outcome_unknown"
                    } else {
                        "failed"
                    });
                    json!({"index":index,"tool":name,"ok":false,"startMs":start_ms,"elapsedMs":step_start.elapsed().as_millis(),"error":error})
                }
            };
            let mut failed = entry["ok"] != true;
            // No screenshot follows a completed delay unless it is the last step.
            if each && !(held.is_some() && !failed && index + 1 != step_count) {
                self.batch_observe(&reference, &profile, settle, &mut entry, baseline.take())
                    .await;
                baseline_taken = false;
                if entry.get("observationError").is_some_and(fails_batch) {
                    if !failed {
                        result["status"] = json!("failed");
                    }
                    failed = true;
                }
                result["observation"] = entry["observation"].clone();
            }
            result["steps"].as_array_mut().unwrap().push(entry);
            // Successful per-step observation leaves no I/O before final publication.
            // Keep intermediate and failed steps durable before continuing or stopping.
            if !each || failed || index + 1 != step_count {
                self.record_batch(
                    &reference.session_id,
                    command_id.clone(),
                    signature.clone(),
                    result.clone(),
                )?;
            }
            if failed {
                break;
            }
        }
        if result["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["ok"] == true && !s.get("observationError").is_some_and(fails_batch))
        {
            result["status"] = json!("completed");
        }
        if !each && args["observe"] != false {
            let observation_start = Instant::now();
            let final_step = result["steps"].as_array().unwrap().last().unwrap();
            let reused = screenshot_step(final_step);
            let reused_elapsed = reused.then(|| final_step["elapsedMs"].clone());
            let observation = if reused {
                Ok(final_step["data"].clone())
            } else {
                Box::pin(self.observe_after_input(json!({"runId":reference.run_id,"goalId":reference.goal_id,"sessionId":reference.session_id,"profile":profile}), settle, baseline.take())).await
            };
            match observation {
                Ok(observation) => {
                    result["observation"] = observation;
                    if let Some(session) = self.sessions.get_mut(&reference.session_id) {
                        *session
                            .last_observation
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                    }
                }
                Err(error) => {
                    result["observationError"] = json!(error);
                    if result["status"] == "completed" && fails_batch(&result["observationError"]) {
                        result["status"] = json!("failed");
                    }
                }
            }
            result["observationElapsedMs"] =
                reused_elapsed.unwrap_or_else(|| json!(observation_start.elapsed().as_millis()));
        }
        result["elapsedMs"] = json!(start.elapsed().as_millis());
        self.record_batch(&reference.session_id, command_id, signature, result.clone())?;
        batch_result(result)
    }

    /// Holds a delay step; a starting handoff, lease change or session close
    /// ends it early so the handoff never waits for the delay.
    async fn hold(&self, reference: &SessionRef, lease: &str, held: Duration) -> Result<Value> {
        let authority = self.session(reference)?.authority();
        let credential = authority.admit(lease, Instant::now())?;
        tokio::select! {
            () = tokio::time::sleep(held) => Ok(json!({"durationMs":held.as_millis()})),
            error = authority.interrupted(&credential) => Err(Error::new(
                &error.code,
                format!("Delay ended early and later steps were not sent: {}", error.message),
            )),
        }
    }

    async fn batch_observe(
        &mut self,
        reference: &SessionRef,
        profile: &Value,
        settle: bool,
        entry: &mut Value,
        baseline: Option<crate::capture::settling::Sample>,
    ) {
        let start = Instant::now();
        let reused = screenshot_step(entry);
        let observation = if reused {
            Ok(entry["data"].clone())
        } else {
            Box::pin(self.observe_after_input(json!({"runId":reference.run_id,"goalId":reference.goal_id,"sessionId":reference.session_id,"profile":profile}), settle, baseline)).await
        };
        match observation {
            Ok(data) => entry["observation"] = data,
            Err(error) => entry["observationError"] = json!(error),
        }
        entry["observationElapsedMs"] = if reused {
            entry["elapsedMs"].clone()
        } else {
            json!(start.elapsed().as_millis())
        };
    }
}

/// Operations whose effects can be compared against a pre-input picture.
fn input_step(name: &str) -> bool {
    matches!(
        name,
        "tap"
            | "swipe"
            | "long_press"
            | "key"
            | "type_text"
            | "app_launch"
            | "app_restart"
            | "app_terminate"
            | "open_url"
    )
}

/// A step in progress is never cut off, so the deadline is checked before each
/// dispatch; a delay is started only when it ends by the deadline.
fn past_deadline(step_start: Instant, held: Option<Duration>, deadline: Instant) -> bool {
    step_start >= deadline || held.is_some_and(|held| step_start + held > deadline)
}

/// A missing observation stops the batch, except when the app in front
/// blocks screenshots: the device answered, and the inputs ran as reported.
fn fails_batch(error: &Value) -> bool {
    error["code"] != "CAPTURE_BLOCKED"
}

/// A successful screenshot step supplies the observation that follows it.
fn screenshot_step(entry: &Value) -> bool {
    entry["tool"] == "screenshot" && entry["ok"] == true
}

fn batch_result(result: Value) -> Result<Value> {
    if result["status"] == "completed" {
        Ok(result)
    } else {
        Err(Error::new(
            if result["status"] == "outcome_unknown" {
                "BATCH_OUTCOME_UNKNOWN"
            } else {
                "BATCH_FAILED"
            },
            "Batch stopped; inspect step results and observation before further input",
        )
        .details(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args() -> Value {
        json!({"runId":"r","goalId":"g","sessionId":"s","leaseId":"l","commandId":"batch","display":{"displayId":"main","geometryVersion":"100x200"},"scope":{"kind":"display"},"steps":[{"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}}]})
    }
    fn server() -> Server {
        let mut server = Server::new();
        server.sessions.insert(
            "s".into(),
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
            },
        );
        server
    }
    #[tokio::test]
    async fn refused_later_step_names_action_without_admitting_the_batch() {
        let mut server = server();
        let mut input = args();
        input["steps"] = json!([
            {"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}},
            {"tool":"type_text","arguments":{}}
        ]);
        let error = server.batch(input).await.unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT");
        assert!(error.message.contains("actions[1] (type_text)"));
        let details = error.details.unwrap();
        assert_eq!(details["actionIndex"], 1);
        assert_eq!(details["actionType"], "type_text");
        assert!(server.sessions["s"].batches.is_empty());
        assert!(server.sessions["s"].journal.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn failed_observation_baseline_records_failure_without_dispatching_input() {
        let root = tempfile::tempdir().unwrap();
        let archive = journal::Archive::create(root.path()).unwrap();
        let mut server = server();
        server.archive_outcomes(archive.clone());
        let serial = format!("Acme-baseline-{}", uuid::Uuid::new_v4());
        let mut sources = crate::device_media::Sources::default();
        let source = sources.fixture_failed_stream(&serial);
        let session = server.sessions.get_mut("s").unwrap();
        session.target.kind = "device".into();
        session.control_path = "hid".into();
        session.video = Some(source.clone());
        let mut input = args();
        input["observeAfterEach"] = json!(true);
        let result = server.batch(input).await.unwrap_err().details.unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["steps"].as_array().unwrap().len(), 1);
        assert_eq!(result["steps"][0]["error"]["code"], "BASELINE_UNAVAILABLE");
        assert_eq!(
            result["steps"][0]["error"]["details"]["inputDispatched"],
            false
        );
        assert_eq!(archive.status("batch").unwrap()["status"], "failed");
        assert!(
            archive.status("batch:0").is_err(),
            "No input journal was created"
        );
        source.close().await.unwrap();
    }
    #[tokio::test]
    async fn durable_batch_survives_session_removal_and_failed_admission_dispatches_nothing() {
        let root = tempfile::tempdir().unwrap();
        let archive = journal::Archive::create(root.path()).unwrap();
        let mut server = server();
        server.archive_outcomes(archive.clone());
        let mut input = args();
        input["observe"] = json!(false);
        input["steps"] = json!([{"tool":"recording_status","arguments":{"recordingId":"unowned"}}]);
        assert_eq!(
            server.batch(input.clone()).await.unwrap_err().code,
            "BATCH_FAILED"
        );
        server.sessions.clear();
        let status = archive.status("batch").unwrap();
        assert_eq!(status["status"], "failed");
        assert_eq!(status["steps"][0]["error"]["code"], "NOT_OWNED");
        let mut server = self::server();
        server.archive_outcomes(archive);
        std::fs::remove_dir_all(root.path().join("outcomes")).unwrap();
        assert_eq!(server.batch(input).await.unwrap_err().code, "EVIDENCE_IO");
        assert!(server.sessions["s"].batches.is_empty());
        assert!(server.sessions["s"].journal.is_empty());
    }

    #[tokio::test]
    async fn cancellation_persists_unknown_outcome_before_session_is_discarded() {
        let root = tempfile::tempdir().unwrap();
        let archive = journal::Archive::create(root.path()).unwrap();
        let mut server = server();
        server.archive_outcomes(archive.clone());
        server
            .record_command(
                "s",
                "cancelled".into(),
                json!("signature"),
                json!({"commandId":"cancelled","tool":"tap","state":"dispatched"}),
            )
            .unwrap();
        server.cancel_dispatched().await.unwrap();
        server.sessions.clear();
        let recovered = archive.status("cancelled").unwrap();
        assert_eq!(recovered["state"], "outcome_unknown");
        assert_eq!(recovered["reason"], "request_cancelled");
    }
    #[tokio::test]
    async fn per_step_capture_failure_stops_remaining_input() {
        let mut server = server();
        let mut input = args();
        input["observeAfterEach"] = json!(true);
        input["steps"] = json!([
            {"tool":"recording_status","arguments":{"recordingId":"unowned"}},
            {"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}}
        ]);
        let result = server
            .batch(input.clone())
            .await
            .unwrap_err()
            .details
            .unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["steps"].as_array().unwrap().len(), 1);
        assert!(result["steps"][0]["observationError"].is_object());
        assert!(result["steps"][0]["observationElapsedMs"].is_number());
        // The fixture target is absent; the failed generation check closes its session.
        assert!(!server.sessions.contains_key("s"));
    }
    #[tokio::test]
    async fn explicit_screenshot_is_reused_and_conflicting_observation_options_admit_nothing() {
        let mut server = server();
        let mut input = args();
        input["observeAfterEach"] = json!(true);
        input["observe"] = json!(false);
        assert_eq!(
            server.batch(input.clone()).await.unwrap_err().code,
            "INVALID_ARGUMENT"
        );
        assert!(server.sessions["s"].batches.is_empty());
        let reference = session_ref(&input).unwrap();
        let mut entry =
            json!({"tool":"screenshot","ok":true,"elapsedMs":12,"data":{"artifactId":"image"}});
        server
            .batch_observe(&reference, &json!("fast"), true, &mut entry, None)
            .await;
        assert_eq!(entry["observation"], entry["data"]);
        assert_eq!(entry["observationElapsedMs"], 12);
        assert!(entry.get("observationError").is_none());
    }
    #[tokio::test]
    async fn stops_after_failure_and_duplicate_ids_return_the_retained_result() {
        let mut server = server();
        let mut input = args();
        input["observe"] = json!(false);
        input["steps"] = json!([
            {"tool":"recording_status","arguments":{"recordingId":"unowned"}},
            {"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}}
        ]);
        let failed = server.batch(input.clone()).await.unwrap_err();
        assert_eq!(failed.code, "BATCH_FAILED");
        let result = failed.details.unwrap();
        assert_eq!(result["steps"].as_array().unwrap().len(), 1);
        assert_eq!(result["steps"][0]["error"]["code"], "NOT_OWNED");
        assert!(server.sessions["s"].journal.is_empty());
        assert_eq!(
            server
                .batch(input.clone())
                .await
                .unwrap_err()
                .details
                .unwrap(),
            result
        );
        input["steps"][0]["arguments"]["recordingId"] = json!("different");
        assert_eq!(
            server.batch(input).await.unwrap_err().code,
            "COMMAND_ID_CONFLICT"
        );
    }
    #[tokio::test]
    async fn unsupported_later_input_leaves_no_batch_or_action_admission() {
        for (kind, tool, arguments) in [
            (
                "simulator",
                "tap",
                json!({"point":{"x":0.5,"y":0.5},"scope":{"kind":"app","appId":"com.example.acme"}}),
            ),
            (
                "simulator",
                "type_text",
                json!({"text":"Acme","element":{"snapshotId":"snapshot","elementId":"field"}}),
            ),
            ("device", "type_text", json!({"text":"Acme","replace":true})),
            ("device", "key", json!({"key":"home"})),
        ] {
            let mut server = server();
            server.sessions.get_mut("s").unwrap().target.kind = kind.into();
            let mut input = args();
            if kind == "device" {
                input["scope"] = json!({"kind":"app","appId":"com.example.acme"});
            }
            input["observe"] = json!(false);
            input["steps"] = json!([
                {"tool":"recording_status","arguments":{"recordingId":"unowned"}},
                {"tool":tool,"arguments":arguments}
            ]);
            assert_eq!(
                server.batch(input).await.unwrap_err().code,
                "CAPABILITY_UNAVAILABLE"
            );
            assert!(server.sessions["s"].batches.is_empty());
            assert!(server.sessions["s"].journal.is_empty());
        }
    }

    #[tokio::test]
    async fn invalid_later_steps_admit_nothing_and_unknown_batches_block_new_input() {
        let mut server = server();
        let mut input = args();
        input["observe"] = json!(false);
        input["steps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tool":"tap","arguments":{"point":{"x":9,"y":0}}}));
        assert_eq!(
            server.batch(input).await.unwrap_err().code,
            "INVALID_ARGUMENT"
        );
        assert!(server.sessions["s"].batches.is_empty());
        server.sessions.get_mut("s").unwrap().batches.insert(
            "unknown".into(),
            (
                json!("signature"),
                json!({"status":"outcome_unknown","steps":[]}),
            ),
        );
        assert_eq!(
            server.batch(args()).await.unwrap_err().code,
            "SESSION_UNCERTAIN"
        );
        assert!(server.sessions["s"].journal.is_empty());
    }
    #[test]
    fn preflight_checks_every_step_and_keeps_authority_out_of_payloads() {
        let input = args();
        let prepared = prepare(&input, false).unwrap();
        assert_eq!(prepared[0].1["commandId"], "batch:0");
        assert_eq!(prepared[0].1["scope"], input["scope"]);
        let mut invalid = input.clone();
        invalid["steps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tool":"tap","arguments":{"point":{"x":2,"y":0}}}));
        assert!(prepare(&invalid, false).is_err());
        for field in ["leaseId", "runId", "commandId"] {
            let mut invalid = input.clone();
            invalid["steps"][0]["arguments"][field] = json!("other");
            assert!(prepare(&invalid, false).is_err());
        }
        let mut invalid = input;
        invalid["steps"][0]["tool"] = json!("session_close");
        assert!(prepare(&invalid, false).is_err());
    }
    fn delay(ms: u64) -> Value {
        json!({"tool":"delay","arguments":{"durationMs":ms}})
    }
    #[tokio::test]
    async fn delays_hold_time_without_input_and_report_step_start_times() {
        let mut server = server();
        let mut input = args();
        input["observe"] = json!(false);
        input["steps"] = json!([delay(30), delay(0)]);
        let result = server.batch(input).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["steps"][0]["startMs"], 0);
        assert!(result["steps"][0]["elapsedMs"].as_u64().unwrap() >= 30);
        assert!(result["steps"][1]["startMs"].as_u64().unwrap() >= 30);
        assert_eq!(result["steps"][1]["data"]["durationMs"], 0);
        assert!(server.sessions["s"].journal.is_empty());
        // Only the last delay is followed by a screenshot. The fixture target is
        // absent, so that attempt fails, which shows it was made.
        let mut input = args();
        input["commandId"] = json!("observed");
        input["observeAfterEach"] = json!(true);
        input["steps"] = json!([delay(1), delay(1)]);
        let result = server.batch(input).await.unwrap_err().details.unwrap();
        let first = &result["steps"][0];
        assert!(first.get("observation").is_none() && first.get("observationError").is_none());
        assert!(result["steps"][1]["observationError"].is_object());
    }
    #[tokio::test]
    async fn delay_bounds_are_refused_before_any_dispatch() {
        for (steps, timeout, index) in [
            (json!([delay(60_001)]), None, 0),
            (json!([delay(50_000), delay(40_001)]), None, 1),
            (
                json!([{"tool":"swipe","arguments":{"from":{"x":0.1,"y":0.1},"to":{"x":0.5,"y":0.5},"durationMs":600}}, delay(500)]),
                Some(1000),
                1,
            ),
        ] {
            let mut server = server();
            let mut input = args();
            input["steps"] = steps.clone();
            if let Some(timeout) = timeout {
                input["timeoutMs"] = json!(timeout);
            }
            let error = server.batch(input).await.unwrap_err();
            assert_eq!(error.code, "INVALID_ARGUMENT", "{steps}");
            assert_eq!(error.details.unwrap()["actionIndex"], index, "{steps}");
            assert!(server.sessions["s"].batches.is_empty());
        }
        let mut input = args();
        input["steps"] = json!([{"tool":"delay","arguments":{"durationMs":5,"leaseId":"l"}}]);
        assert!(prepare(&input, false).is_err());
        input["steps"] = json!([{"tool":"delay","arguments":{"durationMs":5}}]);
        assert_eq!(prepare(&input, true).unwrap()[0].0, "delay");
    }
    #[tokio::test]
    async fn starting_handoff_ends_a_delay_and_sends_no_later_step() {
        let mut server = server();
        let authority = server.sessions["s"].authority();
        let handoff = tokio::spawn({
            let authority = authority.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let now = Instant::now();
                let viewer = Some(crate::control::Owner::Viewer("acme-viewer".into()));
                authority
                    .plan(&authority.credential(now), viewer, now)
                    .unwrap();
            }
        });
        let mut input = args();
        input["observe"] = json!(false);
        input["steps"] =
            json!([delay(30_000), {"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}}]);
        let start = Instant::now();
        let error = server.batch(input).await.unwrap_err();
        handoff.await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(error.code, "BATCH_FAILED");
        let result = error.details.unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["steps"].as_array().unwrap().len(), 1);
        assert_eq!(
            result["steps"][0]["error"]["code"],
            "CONTROL_HANDOFF_PENDING"
        );
        assert!(server.sessions["s"].journal.is_empty());
    }
    #[test]
    fn deadline_admits_steps_before_it_and_only_delays_that_fit() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(100);
        let ms = Duration::from_millis;
        assert!(!past_deadline(start + ms(99), None, deadline));
        assert!(past_deadline(start + ms(100), None, deadline));
        assert!(!past_deadline(start + ms(60), Some(ms(40)), deadline));
        assert!(past_deadline(start + ms(60), Some(ms(41)), deadline));
    }
    #[tokio::test]
    async fn delay_past_the_deadline_stops_the_batch_without_unknown_outcome() {
        let mut server = server();
        let mut input = args();
        input["observe"] = json!(false);
        input["timeoutMs"] = json!(30);
        // Admitted by preflight (25 + 5 ms), but the second delay starts after
        // 25 ms have passed and would end after the deadline.
        input["steps"] = json!([delay(25), delay(5)]);
        let error = server.batch(input).await.unwrap_err();
        assert_eq!(error.code, "BATCH_FAILED");
        let result = error.details.unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["steps"][0]["ok"], true);
        assert_eq!(result["steps"][1]["error"]["code"], "TIMEOUT");
        assert!(result["steps"][1]["elapsedMs"].as_u64().unwrap() < 5);
    }
    #[test]
    fn batches_share_step_names_and_android_refuses_media_steps() {
        let mut input = args();
        assert_eq!(prepare(&input, true).unwrap()[0].0, "tap");
        assert_eq!(prepare(&input, false).unwrap()[0].0, "tap");
        for media in ["recording_start", "preview_start"] {
            input["steps"][0] = json!({"tool":media,"arguments":{}});
            assert_eq!(
                prepare(&input, true).unwrap_err().code,
                "CAPABILITY_UNAVAILABLE",
                "{media}"
            );
        }
        input["steps"][0] = json!({"tool":"android.tap","arguments":{}});
        assert_eq!(prepare(&input, true).unwrap_err().code, "INVALID_ARGUMENT");
    }
}
