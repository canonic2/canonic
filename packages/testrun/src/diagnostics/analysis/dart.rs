//! Class snapshot differences with producer generation/clock and reset compatibility.
use super::model::{Index, Row};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub(super) fn compare(a: &Index, b: &Index) -> Value {
    fn groups(index: &Index) -> BTreeMap<String, Vec<&Row>> {
        let mut groups: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
        if index.format != "dart-diagnostics-v1" {
            return groups;
        }
        for row in &index.rows {
            let d = &row.data;
            if d["dartClassSnapshot"] != true
                || row.value.is_none()
                || d["vmMicros"].as_u64().is_none()
            {
                continue;
            }
            let identity = json!([
                d["targetId"],
                d["declaredAppId"],
                d["runtimeGeneration"],
                d["vmStartTime"],
                d["vmPid"],
                d["isolateId"],
                d["isolateStartTime"],
                d["rootLibrary"]["uri"],
                d["classIdentity"]["id"],
                d["classIdentity"]["name"]
            ]);
            if identity.as_array().unwrap().iter().any(Value::is_null) {
                continue;
            }
            let key = json!([
                identity,
                row.name,
                row.unit,
                row.definition,
                d["accumulatorReset"],
                d["buildId"],
                d["isolateGroupId"]
            ])
            .to_string();
            groups.entry(key).or_default().push(row);
        }
        groups
    }
    let mut results = vec![];
    let left = groups(a);
    let right = groups(b);
    if a.parser == b.parser {
        for (key, rows) in left {
            let Some(after) = right.get(&key) else {
                continue;
            };
            let clock = |r: &&Row| r.data["vmMicros"].as_u64().unwrap();
            let Some(before) = rows
                .iter()
                .filter(|r| r.data["snapshotPosition"] == "before")
                .max_by_key(|r| clock(r))
            else {
                continue;
            };
            let Some(later) = after
                .iter()
                .filter(|r| r.data["snapshotPosition"] == "after")
                .max_by_key(|r| clock(r))
            else {
                continue;
            };
            if clock(before) >= clock(later) || results.len() >= 200 {
                continue;
            }
            let delta = later.value.unwrap() - before.value.unwrap();
            let mut fields = serde_json::Map::new();
            for (field, unit) in [
                ("bytesCurrent", "bytes"),
                ("instancesCurrent", "objects"),
                ("accumulatedSize", "bytes"),
                ("instancesAccumulated", "objects"),
            ] {
                if let (Some(before), Some(after)) = (
                    before.data["observation"]["statistics"][field].as_f64(),
                    later.data["observation"]["statistics"][field].as_f64(),
                ) {
                    let delta = after - before;
                    fields.insert(field.into(),json!({"before":before,"after":after,"delta":if delta.is_finite(){Some(delta)}else{None},"unit":unit}));
                }
            }
            results.push(json!({"name":before.name,"unit":before.unit,"class":before.data["classIdentity"],"before":{"value":before.value,"source":before.source,"vmMicros":before.data["vmMicros"]},"after":{"value":later.value,"source":later.source,"vmMicros":later.data["vmMicros"]},"delta":if delta.is_finite(){Some(delta)}else{None},"statistics":fields,"objectIdentityMatched":false,"leakEstablished":false}));
        }
    }
    json!({"method":"dart-class-observations/1","groups":results,"limit":200,"meaning":"before/after class statistics in the same collector epoch with exact producer VM/isolate/root/class/reset identity and ordered VM clocks; fresh captures have different epochs, no object matching, retaining-size or leak conclusion","identityProvenance":"caller/producer declared; imported metadata does not authenticate device or build"})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn index(position: &str, time: u64, value: u64) -> Index {
        let packet = json!({"schemaVersion":1,"kind":"snapshot","targetId":"dart:acme","declaredAppId":"acme","runtimeGeneration":"socket-a","vmStartTime":1,"vmPid":42,"isolateId":"isolates/1","isolateStartTime":2,"rootLibrary":{"uri":"package:acme/main.dart"},"actionIds":[],"data":{"position":position,"vmMicros":time,"allocations":{"members":[{"class":{"id":"classes/1","name":"Retained"},"bytesCurrent":value}]}}});
        super::super::decode::decode("dart-diagnostics-v1", format!("{packet}\n").as_bytes())
            .unwrap()
    }
    #[test]
    fn diagnostic_dart_log_record_with_error_object_is_a_failure() {
        let record = |level: u64, error: Value| json!({"jsonrpc":"2.0","method":"streamNotify","params":{"streamId":"Logging","event":{"type":"Event","kind":"Logging","isolate":{"id":"isolates/1"},"logRecord":{"level":level,"message":{"valueAsString":"Acme caught exception"},"error":error}}}});
        let packet = json!({"schemaVersion":1,"kind":"events","targetId":"dart:acme","declaredAppId":"acme","isolateId":"isolates/1","actionIds":[],"data":{"events":[
            record(900, json!({"type":"@Instance","kind":"PlainInstance","class":{"name":"StateError"}})),
            record(800, json!({"type":"@Instance","kind":"Null"}))
        ]}});
        let index =
            super::super::decode::decode("dart-diagnostics-v1", format!("{packet}\n").as_bytes())
                .unwrap();
        let kinds: Vec<_> = index
            .rows
            .iter()
            .filter(|r| r.name == "Logging")
            .map(|r| r.kind.as_str())
            .collect();
        assert_eq!(kinds, ["failure", "event"]);
    }
    #[test]
    fn diagnostic_dart_class_comparison_refuses_fresh_epochs_restarts_resets_and_unordered_windows()
    {
        let before = index("before", 1000, 1024);
        let after = index("after", 2000, 2048);
        let result = compare(&before, &after);
        assert_eq!(result["groups"][0]["delta"], 1024.0);
        assert_eq!(result["groups"][0]["leakEstablished"], false);
        for (field, value) in [
            ("runtimeGeneration", json!("socket-b")),
            ("isolateStartTime", json!(3)),
            ("vmStartTime", json!(9)),
            ("accumulatorReset", json!(99)),
            ("vmMicros", json!(1000)),
        ] {
            let mut changed = after.clone();
            changed.rows[0].data[field] = value;
            assert!(
                compare(&before, &changed)["groups"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{field}"
            );
        }
        let mut missing = after;
        missing.rows[0].data["classIdentity"]["id"] = Value::Null;
        assert!(
            compare(&before, &missing)["groups"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
