//! Bounded weighted call trees/flamegraph paths over retained native stacks.
use super::model::Row;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
type StackIdentity = (String, String, String, Vec<String>);
type StackGroup = (f64, Vec<usize>);
pub(super) fn stacks(
    rows: &[(usize, &Row)],
    view: &str,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let mut groups: BTreeMap<StackIdentity, StackGroup> = BTreeMap::new();
    let mut key_bytes = 0usize;
    for (ordinal, r) in rows {
        if r.kind != "stack" {
            continue;
        }
        let Some(frames) = r.data["observation"]["framesRootFirst"]
            .as_array()
            .or_else(|| r.data["frames"].as_array())
        else {
            continue;
        };
        if frames.is_empty() {
            continue;
        }
        let Some(value) = r.value else {
            continue;
        };
        let unit = r
            .unit
            .clone()
            .ok_or_else(|| Error::invalid("Stack weights require units"))?;
        let mut path = vec![];
        let scope=json!({"targetId":r.data["targetId"],"backend":r.data["backend"],"identity":r.data["identity"],"utid":r.data["observation"]["fields"]["utid"],"threadId":r.data["observation"]["fields"]["thread_id"],"producerThread":r.data["observation"]["thread"],"profile":r.data["profile"],"clockDomain":r.clock_domain,"component":r.data["frameworkEvidence"]["componentId"],"processGeneration":r.data["frameworkEvidence"]["processGeneration"],"runtimeGeneration":r.data["frameworkEvidence"]["runtimeGeneration"]}).to_string();
        for f in frames {
            path.push(f.to_string());
            if view == "call_tree" {
                let key = (
                    unit.clone(),
                    r.definition.clone().unwrap_or_default(),
                    scope.clone(),
                    path.clone(),
                );
                if !groups.contains_key(&key) {
                    key_bytes += key.0.len()
                        + key.1.len()
                        + key.2.len()
                        + key.3.iter().map(String::len).sum::<usize>();
                }
                let entry = groups.entry(key).or_default();
                entry.0 += value;
                if entry.1.len() < 32 {
                    entry.1.push(*ordinal);
                }
            }
            if groups.len() > 20000 || key_bytes > 8 * 1024 * 1024 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "Call tree exceeds 20000 nodes; narrow the query",
                ));
            }
        }
        if view == "flamegraph" {
            let key = (unit, r.definition.clone().unwrap_or_default(), scope, path);
            if !groups.contains_key(&key) {
                key_bytes += key.0.len()
                    + key.1.len()
                    + key.2.len()
                    + key.3.iter().map(String::len).sum::<usize>();
            }
            if groups.len() >= 20000 && !groups.contains_key(&key) || key_bytes > 8 * 1024 * 1024 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "Flamegraph exceeds its model budget; narrow the query",
                ));
            }
            let entry = groups.entry(key).or_default();
            entry.0 += value;
            if entry.1.len() < 32 {
                entry.1.push(*ordinal);
            }
        }
    }
    let matched = groups.len();
    let output:Vec<_>=groups.into_iter().skip(offset).take(limit).map(|((unit,definition,scope,frames),(value,witnesses))|{
        if !value.is_finite(){return Err(Error::new("ANALYSIS_BOUNDS","Stack weight overflow"));}
        let frames:Vec<Value>=frames.iter().map(|s|serde_json::from_str(s).unwrap()).collect();
        Ok(json!({"scope":serde_json::from_str::<Value>(&scope).unwrap(),"framesRootFirst":frames,"value":value,"unit":unit,"definition":definition,"witnessOrdinals":witnesses,"meaning":if view=="call_tree"{"inclusive path weight; ancestor/descendant rows overlap"}else{"exact recorded stack weight; missing/unwound stacks remain gaps"}}))
    }).collect::<Result<_>>()?;
    let next = offset + output.len();
    Ok(
        json!({"view":view,"rows":output,"matched":matched,"nextOffset":if next<matched{Some(next)}else{None},"symbolResolution":"producer frame identity only; names do not establish matching native symbols","offCpuInference":false,"privacy":"private","conclusion":"recorded weighted stacks only; units and measurement definitions stay separate"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_native_trace_weighted_paths_never_merge_threads_or_units() {
        let mut rows = vec![];
        for thread in [42, 43] {
            for unit in ["samples", "nanoseconds"] {
                rows.push(Row {kind:"stack".into(),name:"sample".into(),timestamp:None,clock_domain:None,value:Some(3.0),unit:Some(unit.into()),definition:Some("producer weight".into()),correlations:vec![],source:"/sample".into(),data:json!({"backend":"simpleperf","observation":{"fields":{"thread_id":thread},"framesRootFirst":[{"name":"main","image":"libacme"},{"name":"work","image":"libacme"}]}})});
            }
        }
        let references: Vec<_> = rows.iter().enumerate().collect();
        let tree = stacks(&references, "call_tree", 0, 100).unwrap();
        assert_eq!(tree["matched"], 8);
        let flame = stacks(&references, "flamegraph", 0, 100).unwrap();
        assert_eq!(flame["matched"], 4);
        assert!(
            flame["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["value"] == 3.0)
        );
    }
    #[test]
    fn diagnostic_native_trace_lifetime_filters_do_not_join_reused_os_pids() {
        use super::super::{
            decode,
            model::Receipt,
            query::{Query, query},
        };
        let artifact = b"native";
        use base64::Engine;
        let envelope = json!({"schemaVersion":1,"backend":"perfetto","report":{"identity":{},"gaps":[]},"actionIds":[],"artifact":{"encoding":"identity","sha256":crate::diagnostics::digest(artifact),"base64":base64::engine::general_purpose::STANDARD.encode(artifact)},"sections":[{"name":"processes","format":"csv","text":"upid,pid,name\n5,42,Acme\n6,42,Reused\n"},{"name":"threads","format":"csv","text":"utid,tid,upid,name\n1,42,5,main\n2,42,6,main\n"},{"name":"scheduling","format":"csv","text":"id,ts,dur,utid,state\n1,100,10,1,Running\n2,200,99,2,R\n"}]});
        let index = decode::decode("native-trace-v1", envelope.to_string().as_bytes()).unwrap();
        let receipt:Receipt=serde_json::from_value(json!({"version":1,"id":"analysis-owned","collectorId":"collector-owned","signature":"owned","owner":"owned","phase":"complete","rawSha256":"owned","rawBytes":1,"format":"native-trace-v1","capturePhase":"complete","binding":{},"metadata":{}})).unwrap();
        let q:Query=serde_json::from_value(json!({"investigationId":"diag-owned","analysisId":"analysis-owned","nativeUpid":5,"kind":"span"})).unwrap();
        let result = query(&receipt, &index, &q).unwrap();
        assert_eq!(result["rows"].as_array().unwrap().len(), 1);
        assert!(result["nextCursor"].is_null());
        assert_eq!(result["rows"][0]["record"]["value"], 10.0);
        let q:Query=serde_json::from_value(json!({"investigationId":"diag-owned","analysisId":"analysis-owned","nativeUtid":2,"kind":"span"})).unwrap();
        let result = query(&receipt, &index, &q).unwrap();
        assert_eq!(result["rows"].as_array().unwrap().len(), 1);
        assert!(result["nextCursor"].is_null());
        assert_eq!(result["rows"][0]["record"]["value"], 99.0);
    }
}
