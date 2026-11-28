//! Static outcome routing and bounded next actions, without source probes.
use serde_json::{Value, json};
pub(super) fn routes() -> Value {
    json!({
     "entry":{"mcp":"diagnostics_capabilities","cli":"testrun diagnostics capabilities","readiness":"static registration only; inspect the selected source prerequisite before collection"},
     "outcomes":[
     {"outcome":"choose a collector","tools":["diagnostics_capabilities","diagnostics_plan"],"guide":"diagnostic-workflows.md","rule":"smallest source that can answer the question; no fallback after uncertain dispatch"},
     {"outcome":"capture and settle","tools":["diagnostics_open","diagnostics_start","diagnostics_status","diagnostics_stop","diagnostics_close"],"rule":"same submissionId and exact inputs recover lost replies; status before retry; stop only owned collectors"},
     {"outcome":"analyze and cancel","tools":["diagnostics_analyze","diagnostics_analysis_status","diagnostics_analysis_cancel"],"rule":"finalized raw evidence; same submissionId recovers lost replies; cancellation preserves raw bytes"},
     {"outcome":"inspect native evidence","tools":["diagnostics_query","diagnostics_read"],"views":["rows","timeline","call_tree","flamegraph","memory","performance","network_data","scenario","assertions","observability"],"rule":"bounded explicit reads; preserve producer clocks, units, sampling and identity gaps"},
     {"outcome":"compose sources","tools":["diagnostics_query"],"view":"investigation","operations":["summary","timeline","failures","stacks","resources","frames","retention","requests"],"rule":"primary plus up to three additional analyses in the same investigation; timeline order within native clocks only"},
     {"outcome":"compare correction","tools":["diagnostics_compare","diagnostics_query"],"rule":"comparison compatibility and fixed assertions remain distinct from causal proof"},
     {"outcome":"retain or share a report","tools":["diagnostics_query","diagnostics_export"],"view":"report","formats":["investigation-json","investigation-markdown"],"guide":"diagnostic-reports.md","rule":"minimal privacy omits text/identities; explicit private includes them; caller statements remain attributed"},
     {"outcome":"inspect in human native tools","tools":["diagnostics_export"],"format":"native_artifact","rule":"explicit scope all; original private native artifact; viewer optional; no implicit launch/upload"}
     ],
     "documentation":{"tools":["docs_list","docs_search","docs_read"],"start":"diagnostic-workflows.md","reference":"diagnostic-reports.md"},
     "limits":{"selectedAnalyses":4,"rowsPerSource":20000,"queryBytes":262144,"queryRows":200,"reportBytes":524288,"statementCountPerSection":16,"witnessesPerStatement":8},
     "authority":"retained observations and caller declarations; no automatic remediation or inferred root cause"
    })
}
pub(super) fn collector_next(inv: &str, id: &str, settled: bool) -> Value {
    if settled {
        json!({"tool":"diagnostics_analyze","arguments":{"investigationId":inv,"collectorId":id},"required":"choose a stable submissionId; a failed/empty collector may be unavailable for analysis"})
    } else {
        json!({"tool":"diagnostics_status","arguments":{"investigationId":inv},"condition":"inspect until collector.settled; diagnostics_stop requests bounded early settlement"})
    }
}
pub(super) fn analysis_next(id: &str, settled: bool, complete: bool) -> Value {
    if !settled {
        json!({"tool":"diagnostics_analysis_status","arguments":{"analysisId":id},"required":"use the same investigationId; inspect until settled or request diagnostics_analysis_cancel"})
    } else if complete {
        json!({"tool":"diagnostics_query","arguments":{"analysisId":id,"view":"rows","limit":20},"required":"use the same investigationId; inspect availableKinds, source gaps and evidence witnesses"})
    } else {
        json!({"tool":"diagnostics_analysis_status","arguments":{"analysisId":id},"required":"use the same investigationId; inspect reason and choose a supported export/format or new corrected submission; do not recollect on replay"})
    }
}
pub(super) fn query_next(inv: &str, id: &str, next: Option<Value>) -> Value {
    if let Some(cursor) = next.filter(Value::is_string) {
        json!({"tool":"diagnostics_query","arguments":{"investigationId":inv,"analysisId":id,"cursor":cursor},"required":"preserve the same view, filters and selected analyses"})
    } else {
        json!({"tool":"diagnostics_query","arguments":{"investigationId":inv,"analysisId":id,"view":"report","report":{}},"purpose":"summarize selected evidence; report.privacy private explicitly includes text"})
    }
}
pub(super) fn source_report(value: &Option<Value>) -> Value {
    match value {
        Some(v) if serde_json::to_vec(v).unwrap().len() <= 4096 => v.clone(),
        Some(_) => {
            json!({"omitted":true,"reason":"ordinary metadata bounded to 4 KiB; use diagnostics_read for retained collector bytes"})
        }
        None => Value::Null,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_reports_routes_use_registered_tools_and_next_steps_preserve_read_scope() {
        let routes = routes();
        for route in routes["outcomes"].as_array().unwrap() {
            for name in route["tools"].as_array().unwrap() {
                assert!(
                    super::super::definition(name.as_str().unwrap()).is_some(),
                    "{name}"
                );
            }
        }
        assert_eq!(
            query_next("inv", "a", Some(json!("opaque")))["arguments"]["cursor"],
            "opaque"
        );
        assert_eq!(query_next("inv", "a", None)["arguments"]["view"], "report");
        assert_eq!(
            source_report(&Some(json!("private".repeat(2000))))["omitted"],
            true
        );
    }
}
