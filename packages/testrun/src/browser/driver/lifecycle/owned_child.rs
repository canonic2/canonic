//! Allowlisted observations from our owned SDK entry; no process identity or payloads.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const PREFIX: &str = "testrun:owned-child ";
const MAX_CHILDREN: u8 = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Stream {
    index: u8,
    present: bool,
    closed: Option<bool>,
    destroyed: Option<bool>,
    readable_ended: Option<bool>,
    writable_finished: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    phase: String,
    owner: Option<u8>,
    streams: Option<Vec<Stream>>,
    index: Option<u8>,
    #[serde(rename = "nativeState")]
    native_state: Option<NativeState>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NativeState {
    exit_status_known: Option<bool>,
    native_handle_present: Option<bool>,
    closes_needed: Option<u8>,
    closes_got: Option<u8>,
}
impl NativeState {
    fn valid(&self) -> bool {
        self.closes_needed.is_none_or(|count| count <= 6)
            && self.closes_got.is_none_or(|count| count <= 6)
            && match (self.closes_needed, self.closes_got) {
                (Some(needed), Some(got)) => got <= needed,
                _ => true,
            }
    }
}

#[derive(Default, Clone, Debug)]
struct Child {
    observing: Option<u64>,
    exit: Option<u64>,
    close: Option<u64>,
    observing_streams: Option<serde_json::Value>,
    exit_streams: Option<serde_json::Value>,
    close_streams: Option<serde_json::Value>,
    observing_state: Option<serde_json::Value>,
    exit_state: Option<serde_json::Value>,
    close_state: Option<serde_json::Value>,
    latest_state: Option<serde_json::Value>,
    latest_state_at: Option<u64>,
    latest_streams: Option<serde_json::Value>,
    stream_events: BTreeMap<u8, BTreeMap<&'static str, u64>>,
}

#[derive(Default, Clone, Debug)]
pub(super) struct Children {
    items: BTreeMap<u8, Child>,
    invalid: u64,
    capacity: bool,
    unavailable: bool,
}

impl Children {
    pub(super) fn feed(&mut self, line: &[u8], elapsed: u64) -> Option<&'static str> {
        let text = line.strip_prefix(PREFIX.as_bytes())?;
        let Some(phase) = std::str::from_utf8(text)
            .ok()
            .and_then(|text| self.apply(text, elapsed))
        else {
            self.invalid = self.invalid.saturating_add(1);
            return None;
        };
        Some(phase)
    }

    fn apply(&mut self, text: &str, elapsed: u64) -> Option<&'static str> {
        let observation: Observation = serde_json::from_str(text).ok()?;
        if matches!(observation.phase.as_str(), "capacity" | "unavailable") {
            if observation.owner.is_some()
                || observation.streams.is_some()
                || observation.index.is_some()
                || observation.native_state.is_some()
            {
                return None;
            }
            return Some(if observation.phase == "capacity" {
                self.capacity = true;
                "owned_child_observation_capacity"
            } else {
                self.unavailable = true;
                "owned_child_observation_unavailable"
            });
        }
        let owner = observation
            .owner
            .filter(|owner| (1..=MAX_CHILDREN).contains(owner))?;
        match observation.phase.as_str() {
            "observing" | "native-exit" | "native-close" | "native-state" => {
                if observation.index.is_some() {
                    return None;
                }
                if observation
                    .native_state
                    .as_ref()
                    .is_some_and(|state| !state.valid())
                    || (observation.phase == "native-state" && observation.native_state.is_none())
                {
                    return None;
                }
                let native = observation
                    .native_state
                    .map(|state| serde_json::to_value(state).expect("Allowlisted child state"));
                let streams = observation.streams?;
                if streams.len() > 5
                    || streams
                        .iter()
                        .enumerate()
                        .any(|(index, stream)| usize::from(stream.index) != index)
                {
                    return None;
                }
                let snapshot = serde_json::json!(streams.iter().map(|stream| serde_json::json!({"index":stream.index,"present":stream.present,"closed":stream.closed,"destroyed":stream.destroyed,"readableEnded":stream.readable_ended,"writableFinished":stream.writable_finished})).collect::<Vec<_>>());
                let child = self.items.entry(owner).or_default();
                Some(match observation.phase.as_str() {
                    "observing" => {
                        if child.observing.is_none() {
                            child.observing_state = native;
                        }
                        child.observing.get_or_insert(elapsed);
                        child.observing_streams.get_or_insert(snapshot);
                        "owned_child_observing"
                    }
                    "native-exit" => {
                        if child.exit.is_none() {
                            child.exit_state = native;
                        }
                        child.exit.get_or_insert(elapsed);
                        child.exit_streams.get_or_insert(snapshot);
                        "owned_child_native_exit"
                    }
                    "native-close" => {
                        if child.close.is_none() {
                            child.close_state = native;
                        }
                        child.close.get_or_insert(elapsed);
                        child.close_streams.get_or_insert(snapshot);
                        "owned_child_native_close"
                    }
                    _ => {
                        child.latest_state_at = Some(elapsed);
                        child.latest_state = native;
                        child.latest_streams = Some(snapshot);
                        "owned_child_native_state"
                    }
                })
            }
            "stream-end" | "stream-finish" | "stream-close" => {
                self.record_stream_event(owner, observation, elapsed)
            }
            _ => None,
        }
    }

    fn record_stream_event(
        &mut self,
        owner: u8,
        observation: Observation,
        elapsed: u64,
    ) -> Option<&'static str> {
        if observation.streams.is_some() || observation.native_state.is_some() {
            return None;
        }
        let index = observation.index.filter(|index| *index < 5)?;
        let (event, phase) = match observation.phase.as_str() {
            "stream-end" => ("end", "owned_child_stream_end"),
            "stream-finish" => ("finish", "owned_child_stream_finish"),
            _ => ("close", "owned_child_stream_close"),
        };
        self.items
            .entry(owner)
            .or_default()
            .stream_events
            .entry(index)
            .or_default()
            .entry(event)
            .or_insert(elapsed);
        Some(phase)
    }

    pub(super) fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({"source":"owned-driver-instrumentation","observationsOnly":true,"maxChildren":MAX_CHILDREN,"maxStreamsPerChild":5,"invalidObservations":self.invalid,"capacityExceeded":self.capacity,"sourceUnavailable":self.unavailable,"children":self.items.iter().map(|(owner,child)|serde_json::json!({"owner":owner,"observingElapsedMs":child.observing,"nativeExitElapsedMs":child.exit,"nativeCloseElapsedMs":child.close,"observingStreams":child.observing_streams,"nativeExitStreams":child.exit_streams,"nativeCloseStreams":child.close_streams,"streamEvents":child.stream_events,"observingNativeState":child.observing_state,"nativeExitState":child.exit_state,"nativeCloseState":child.close_state,"latestNativeState":child.latest_state,"latestNativeStateElapsedMs":child.latest_state_at,"latestNativeStreams":child.latest_streams})).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn feed(children: &mut Children, value: Value, elapsed: u64) {
        children.feed(format!("{PREFIX}{value}").as_bytes(), elapsed);
    }
    fn stream(closed: bool) -> Value {
        json!({"index":0,"present":true,"closed":closed,"destroyed":closed,"readableEnded":closed,"writableFinished":null})
    }
    #[test]
    fn owned_child_lifecycle_native_state_does_not_invent_exit_and_retains_initial_snapshots() {
        let mut children = Children::default();
        let state = |got| json!({"exitStatusKnown":true,"nativeHandlePresent":false,"closesNeeded":5,"closesGot":got});
        feed(
            &mut children,
            json!({"phase":"observing","owner":1,"streams":[stream(false)],"nativeState":state(3)}),
            10,
        );
        let initial = children.snapshot();
        assert_eq!(initial["children"][0]["nativeExitElapsedMs"], Value::Null);
        feed(
            &mut children,
            json!({"phase":"native-state","owner":1,"streams":[stream(false)],"nativeState":state(4)}),
            20,
        );
        let latest = children.snapshot();
        assert_eq!(
            latest["children"][0]["observingNativeState"]["closesGot"],
            3
        );
        assert_eq!(latest["children"][0]["latestNativeState"]["closesGot"], 4);
        assert_eq!(latest["children"][0]["latestNativeStateElapsedMs"], 20);
        assert_eq!(latest["children"][0]["nativeExitElapsedMs"], Value::Null);
        for (elapsed, got) in [(30, 4), (40, 5)] {
            feed(
                &mut children,
                json!({"phase":"native-exit","owner":1,"streams":[stream(false)],"nativeState":state(got)}),
                elapsed,
            );
        }
        assert_eq!(
            children.snapshot()["children"][0]["nativeExitState"]["closesGot"],
            4
        );
        assert_eq!(initial["children"][0]["latestNativeState"], Value::Null);
    }
    #[test]
    fn owned_child_lifecycle_native_state_rejects_private_fields_invalid_counters_and_foreign_phases()
     {
        let mut children = Children::default();
        let cases = [
            json!({"phase":"native-state","owner":1,"streams":[]}),
            json!({"phase":"native-state","owner":1,"streams":[],"nativeState":{"pid":123}}),
            json!({"phase":"native-state","owner":1,"streams":[],"nativeState":{"exitStatusKnown":"Acme"}}),
            json!({"phase":"native-state","owner":1,"streams":[],"nativeState":{"closesNeeded":7}}),
            json!({"phase":"native-state","owner":1,"streams":[],"nativeState":{"closesGot":-1}}),
            json!({"phase":"native-state","owner":1,"streams":[],"nativeState":{"closesNeeded":3,"closesGot":4}}),
            json!({"phase":"stream-close","owner":1,"index":0,"nativeState":{}}),
            json!({"phase":"capacity","nativeState":{}}),
        ];
        for case in &cases {
            feed(&mut children, case.clone(), 10);
        }
        assert_eq!(children.snapshot()["invalidObservations"], cases.len());
        assert_eq!(children.snapshot()["children"], json!([]));
        assert!(!children.snapshot().to_string().contains("Acme"));
        feed(
            &mut children,
            json!({"phase":"native-state","owner":1,"streams":[],"nativeState":{}}),
            20,
        );
        assert_eq!(
            children.snapshot()["children"][0]["latestNativeState"]["exitStatusKnown"],
            Value::Null
        );
        assert_eq!(
            children.snapshot()["children"][0]["nativeExitElapsedMs"],
            Value::Null
        );
    }

    #[test]
    fn owned_child_lifecycle_retains_distinct_exit_and_close_snapshots() {
        let mut children = Children::default();
        feed(
            &mut children,
            json!({"phase":"observing","owner":1,"streams":[stream(false)]}),
            10,
        );
        let observing = children.snapshot();
        assert_eq!(observing["children"][0]["nativeExitElapsedMs"], Value::Null);
        feed(
            &mut children,
            json!({"phase":"native-exit","owner":1,"streams":[stream(false)]}),
            20,
        );
        let exited = children.snapshot();
        feed(
            &mut children,
            json!({"phase":"native-close","owner":1,"streams":[stream(true)]}),
            40,
        );
        feed(
            &mut children,
            json!({"phase":"native-exit","owner":1,"streams":[stream(true)]}),
            50,
        );
        let final_ = children.snapshot();
        assert_eq!(final_["children"][0]["nativeExitElapsedMs"], 20);
        assert_eq!(final_["children"][0]["nativeCloseElapsedMs"], 40);
        assert_eq!(
            final_["children"][0]["nativeExitStreams"][0]["closed"],
            false
        );
        assert_eq!(
            final_["children"][0]["nativeCloseStreams"][0]["closed"],
            true
        );
        assert_eq!(exited["children"][0]["nativeCloseElapsedMs"], Value::Null);
        assert_eq!(observing["children"][0]["nativeExitElapsedMs"], Value::Null);
    }

    #[test]
    fn owned_child_lifecycle_stream_events_do_not_infer_native_exit() {
        let mut children = Children::default();
        for (phase, elapsed) in [
            ("stream-end", 10),
            ("stream-finish", 20),
            ("stream-close", 30),
            ("stream-close", 40),
        ] {
            feed(
                &mut children,
                json!({"phase":phase,"owner":1,"index":3}),
                elapsed,
            );
        }
        let snapshot = children.snapshot();
        assert_eq!(
            snapshot["children"][0]["streamEvents"]["3"],
            json!({"end":10,"finish":20,"close":30})
        );
        assert_eq!(snapshot["children"][0]["nativeExitElapsedMs"], Value::Null);
        assert_eq!(snapshot["children"][0]["nativeCloseElapsedMs"], Value::Null);
        assert_eq!(snapshot["children"][0]["observingStreams"], Value::Null);
    }

    #[test]
    fn owned_child_lifecycle_rejects_private_fields_and_invalid_identities() {
        let mut children = Children::default();
        let cases = [
            json!({"phase":"native-exit","owner":0,"streams":[]}),
            json!({"phase":"native-exit","owner":5,"streams":[]}),
            json!({"phase":"stream-close","owner":1,"index":5}),
            json!({"phase":"stream-close","owner":1,"index":-1}),
            json!({"phase":"native-exit","owner":1,"streams":[],"pid":"Acme-secret"}),
            json!({"phase":"observing","owner":1,"streams":[{"index":0,"present":true,"path":"Acme-secret"}]}),
            json!({"phase":"native-exit","owner":1,"streams":[],"index":0}),
            json!({"phase":"stream-close","owner":1,"index":0,"streams":[]}),
            json!({"phase":"Acme-secret","owner":1}),
        ];
        for case in &cases {
            feed(&mut children, case.clone(), 10);
        }
        let snapshot = children.snapshot();
        assert_eq!(snapshot["invalidObservations"], cases.len());
        assert_eq!(snapshot["children"], json!([]));
        assert!(!snapshot.to_string().contains("Acme"));
    }

    #[test]
    fn owned_child_lifecycle_bounds_snapshots_and_preserves_unknown_flags() {
        let mut children = Children::default();
        feed(
            &mut children,
            json!({"phase":"observing","owner":1,"streams":vec![stream(false);6]}),
            1,
        );
        feed(
            &mut children,
            json!({"phase":"observing","owner":1,"streams":[{"index":1,"present":true}]}),
            2,
        );
        feed(
            &mut children,
            json!({"phase":"observing","owner":1,"streams":[{"index":0,"present":true,"closed":"Acme-private"}]}),
            3,
        );
        assert_eq!(children.snapshot()["invalidObservations"], 3);
        for owner in 1..=MAX_CHILDREN {
            feed(
                &mut children,
                json!({"phase":"observing","owner":owner,"streams":[{"index":0,"present":true}]}),
                4,
            );
        }
        let snapshot = children.snapshot();
        assert_eq!(snapshot["children"].as_array().unwrap().len(), 4);
        assert_eq!(
            snapshot["children"][0]["observingStreams"][0]["closed"],
            Value::Null
        );
        assert_eq!(
            snapshot["children"][0]["observingStreams"][0]["readableEnded"],
            Value::Null
        );
        assert!(!snapshot.to_string().contains("Acme"));
    }

    #[test]
    fn owned_child_lifecycle_reports_source_capacity_and_invalid_encoding() {
        let mut children = Children::default();
        feed(&mut children, json!({"phase":"capacity"}), 1);
        feed(&mut children, json!({"phase":"unavailable"}), 2);
        feed(&mut children, json!({"phase":"capacity","owner":1}), 3);
        let mut invalid = PREFIX.as_bytes().to_vec();
        invalid.push(255);
        children.feed(&invalid, 4);
        let snapshot = children.snapshot();
        assert_eq!(snapshot["capacityExceeded"], true);
        assert_eq!(snapshot["sourceUnavailable"], true);
        assert_eq!(snapshot["invalidObservations"], 2);
        assert_eq!(snapshot["children"], json!([]));
    }
}
