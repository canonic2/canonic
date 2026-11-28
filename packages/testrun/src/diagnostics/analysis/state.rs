//! Before/after scalar observations, separate from whole-capture compatibility and leak claims.
use super::model::{Index, Row};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn groups(index: &Index) -> BTreeMap<String, Vec<&Row>> {
    let mut groups: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for row in &index.rows {
        if row.kind != "metric"
            || row.data["stateSource"] != true
            || row.value.is_none()
            || row.name == "frame_duration"
        {
            continue;
        }
        let d = &row.data;
        let identity = [
            d["targetId"].clone(),
            d["appId"].clone(),
            d["userId"].clone(),
            d["uid"].clone(),
            d["bootId"].clone(),
            d["buildIdentity"]["versionCode"].clone(),
            d["buildIdentity"]["lastUpdateTime"].clone(),
            d["processGeneration"].clone(),
        ];
        if identity.iter().any(Value::is_null)
            || row.unit.is_none()
            || row.definition.is_none()
            || !d["counterWindow"]["uptimeSeconds"]
                .as_f64()
                .is_some_and(|n| n.is_finite() && n >= 0.0)
        {
            continue;
        }
        let key = json!([
            identity,
            row.name,
            row.unit,
            row.definition,
            d["counterWindow"]["clockTicksPerSecond"]
        ])
        .to_string();
        groups.entry(key).or_default().push(row);
    }
    groups
}
pub(super) fn compare(a: &Index, b: &Index) -> Value {
    if a.format != b.format || a.parser != b.parser {
        return json!({"method":"android-state-observations/1","groups":[],"reason":"format_or_parser_mismatch"});
    }
    let left = groups(a);
    let right = groups(b);
    let mut results = Vec::new();
    let mut excluded = 0;
    for (key, rows) in &left {
        let Some(other) = right.get(key) else {
            excluded += 1;
            continue;
        };
        let uptime = |r: &&Row| r.data["counterWindow"]["uptimeSeconds"].as_f64().unwrap();
        let before = rows
            .iter()
            .max_by(|a, b| uptime(a).total_cmp(&uptime(b)))
            .unwrap();
        let after = other
            .iter()
            .max_by(|a, b| uptime(a).total_cmp(&uptime(b)))
            .unwrap();
        let before_end = before.data["counterWindow"]["uptimeSeconds"]
            .as_f64()
            .unwrap();
        let after_start = other.iter().map(uptime).reduce(f64::min).unwrap();
        if !before_end.is_finite() || !after_start.is_finite() || before_end >= after_start {
            excluded += 1;
            continue;
        }
        if results.len() >= 200 {
            excluded += 1;
            continue;
        }
        let delta = after.value.unwrap() - before.value.unwrap();
        results.push(json!({"name":before.name,"unit":before.unit,"definition":before.definition,"processGeneration":before.data["processGeneration"],"before":{"value":before.value,"source":before.source,"uptimeSeconds":before_end,"label":before.data["label"]},"after":{"value":after.value,"source":after.source,"uptimeSeconds":after.data["counterWindow"]["uptimeSeconds"],"label":after.data["label"]},"delta":if delta.is_finite(){Some(delta)}else{None},"sampling":"last observed scalar in each nonoverlapping collection, grouped by exact packet target/app/user/UID/boot/build/process/measurement identity","causeEstablished":false,"leakEstablished":false}));
    }
    json!({"method":"android-state-observations/1","groups":results,"excludedBaselineGroups":excluded,"limit":200,"identityProvenance":"packet_producer_reported; raw receipts bind witnesses, imported identities are not OS authenticated","meaning":"differences between recorded observations, even when whole captures are partial; no continuity, steady-state, retained ownership, significance or causality established"})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn index(uptime: f64, value: f64, generation: &str) -> Index {
        let row = Row {
            kind: "metric".into(),
            name: "rss".into(),
            value: Some(value),
            unit: Some("bytes".into()),
            definition: Some("resident bytes".into()),
            timestamp: Some(uptime),
            clock_domain: Some("android-epoch".into()),
            correlations: vec![],
            source: "line:2/data/metrics/0".into(),
            data: json!({"stateSource":true,"targetId":"android:ACME","appId":"com.example.acme","userId":0,"uid":10042,"bootId":"boot-a","processGeneration":generation,"buildIdentity":{"versionCode":"42","lastUpdateTime":"one"},"counterWindow":{"uptimeSeconds":uptime,"clockTicksPerSecond":100}}),
        };
        Index {
            model_bytes: 0,
            version: 1,
            parser: "test".into(),
            format: "android-diagnostics-v1".into(),
            clocks: vec![],
            rows: vec![row],
            unsupported: vec!["partial".into()],
        }
    }
    #[test]
    fn android_state_before_after_compares_exact_nonoverlapping_observations_without_leak_claims() {
        let before = index(10.0, 100.0, "birth-a");
        let after = index(20.0, 150.0, "birth-a");
        let result = compare(&before, &after);
        assert_eq!(result["groups"][0]["delta"], 50.0);
        assert_eq!(result["groups"][0]["leakEstablished"], false);
        for candidate in [index(20.0, 150.0, "birth-b"), index(9.0, 150.0, "birth-a")] {
            assert!(
                compare(&before, &candidate)["groups"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
        let mut missing = after.clone();
        missing.rows[0].data["buildIdentity"]["versionCode"] = Value::Null;
        assert!(
            compare(&before, &missing)["groups"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
