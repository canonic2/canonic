//! Pure bounded queries and compatibility decisions over immutable indexes.
use super::model::*;
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
mod paging;
pub(super) use paging::{finish_page, ordinal as page_ordinal, position};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Query {
    pub investigation_id: String,
    pub analysis_id: String,
    pub kind: Option<String>,
    pub clock_domain: Option<String>,
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub correlation_id: Option<String>,
    pub collection: Option<String>,
    pub cursor: Option<String>,
    #[serde(skip)]
    pub wire_query: Value,
    pub limit: Option<usize>,
    pub view: Option<String>,
    pub native_utid: Option<u64>,
    pub native_upid: Option<u64>,
    pub memory: Option<super::memory::Query>,
    pub performance: Option<super::performance::Query>,
    pub network_data: Option<super::network_data::Query>,
    pub scenario: Option<super::scenario::Query>,
    pub assertions: Option<super::assertions::Request>,
    pub investigation: Option<super::investigation::Request>,
    pub report: Option<super::investigation::Report>,
}
fn alignment(row: &Row, metadata: &Metadata) -> Option<Value> {
    let time = row.timestamp?;
    let domain = row.clock_domain.as_ref()?;
    let clock = metadata.clocks.iter().find(|c| &c.id == domain)?;
    let anchor = clock
        .anchors
        .iter()
        .min_by(|a, b| (a.source - time).abs().total_cmp(&(b.source - time).abs()))?;
    let scale = scale(&clock.unit)?;
    let delta = time - anchor.source;
    let value = (anchor.reference + delta) * scale;
    let uncertainty = (anchor.uncertainty + delta.abs() * clock.max_drift_ppm? * 1e-6) * scale;
    if !value.is_finite() || !uncertainty.is_finite() {
        return None;
    }
    Some(
        json!({"domain":clock.reference_domain,"value":value,"uncertainty":uncertainty,"unit":"seconds","method":"nearest declared anchor with drift bound","provenance":"caller_declared"}),
    )
}
fn symbol_status(row: &Row, metadata: &Metadata) -> Value {
    let build = row.data["buildId"].as_str();
    let architecture = row.data["architecture"].as_str();
    let matches: Vec<_> = metadata
        .symbols
        .iter()
        .filter(|s| {
            Some(s.build_id.as_str()) == build && Some(s.architecture.as_str()) == architecture
        })
        .collect();
    let resolved = metadata.symbolicate
        && row.data["resolved"] == true
        && row.data["symbolication"]["method"] == "atos_uuid_arch_verified"
        && metadata.symbols.iter().any(|s| {
            row.data["symbolication"]["artifactEvidenceId"] == s.evidence_id
                && row.data["symbolication"]["artifactSha256"] == s.sha256
        });
    json!({"state":if resolved{"resolved_uuid_arch_verified"}else if build.is_none()||architecture.is_none(){"identity_unknown"}else if matches.is_empty(){"unmatched"}else{"reference_matched"},"references":matches,"resolved":resolved,"reason":if resolved{"Explicit retained artifact UUID/architecture verified before bounded atos"}else{"Unresolved; references alone do not establish symbolication"}})
}
pub fn query(receipt: &Receipt, index: &Index, q: &Query) -> Result<Value> {
    let (scope, request, after) = position(receipt, q)?;
    if matches!(q.view.as_deref(), None | Some("rows" | "timeline")) {
        if q.collection.is_some()
            || q.memory.is_some()
            || q.performance.is_some()
            || q.network_data.is_some()
            || q.scenario.is_some()
            || q.assertions.is_some()
            || q.investigation.is_some()
            || q.report.is_some()
        {
            return Err(Error::invalid(
                "View-specific options require their matching view",
            ));
        }
        return paging::rows(receipt, index, q, &scope, &request, after.as_deref());
    }
    let offset = paging::ordinal(after.as_deref())?;
    let mut result = legacy_query(receipt, index, q, offset)?;
    if let Some(collection) = &q.collection
        && (q.view.as_deref() != Some("performance") || collection != "measurement_groups")
    {
        return Err(Error::invalid(
            "collection supports measurement_groups for the performance view",
        ));
    }
    if q.view.as_deref() == Some("performance") {
        if q.collection.as_deref() == Some("measurement_groups") {
            result
                .as_object_mut()
                .expect("performance page")
                .remove("rows");
            result["nextOffset"] = result["nextGroupOffset"].clone();
        } else {
            let total = result["measurementGroupCount"].clone();
            if let Some(groups) = result["measurementGroups"].as_array_mut() {
                groups.truncate(16);
            }
            let mut args = q.wire_query.clone();
            if !args.is_object() {
                args = json!({"investigationId":q.investigation_id,"analysisId":q.analysis_id,"view":"performance"});
            }
            args["collection"] = json!("measurement_groups");
            result["measurementGroups_total"] = total;
            result["measurementGroups_list"] = json!({"tool":"diagnostics_query","arguments":args});
        }
        result
            .as_object_mut()
            .expect("performance page")
            .remove("nextGroupOffset");
    }
    finish_page(result, &scope, &request, offset)
}
fn legacy_query(receipt: &Receipt, index: &Index, q: &Query, offset: usize) -> Result<Value> {
    let limit = q.limit.unwrap_or(50);
    let view = q.view.as_deref().unwrap_or("rows");
    if !matches!(
        view,
        "rows"
            | "timeline"
            | "call_tree"
            | "observability"
            | "flamegraph"
            | "memory"
            | "performance"
            | "network_data"
            | "scenario"
            | "assertions"
    ) {
        return Err(Error::invalid(
            "Query view must be rows/timeline/call_tree/flamegraph/memory/performance/network_data/scenario/assertions/observability",
        ));
    }
    if !(1..=100).contains(&limit)
        || q.from.is_some_and(|n| !n.is_finite())
        || q.to.is_some_and(|n| !n.is_finite())
        || q.from.zip(q.to).is_some_and(|(a, b)| a > b)
        || (q.from.is_some() || q.to.is_some()) && q.clock_domain.is_none()
    {
        return Err(Error::invalid(
            "Use limit 1..100 and a clockDomain for numeric time ranges",
        ));
    }
    if let Some(result) = assertions_view(receipt, index, q, view, offset, limit)? {
        return Ok(result);
    }
    if let Some(result) = scenario_view(receipt, index, q, view, offset, limit)? {
        return Ok(result);
    }
    if let Some(result) = network_data_view(receipt, index, q, view, offset, limit)? {
        return Ok(result);
    }
    if let Some(result) = performance_view(receipt, index, q, view, offset, limit)? {
        return Ok(result);
    }
    if let Some(result) = memory_view(receipt, index, q, view, offset, limit)? {
        return Ok(result);
    }
    row_view(receipt, index, q, view, offset, limit)
}

pub fn compare(a: &Receipt, ai: &Index, b: &Receipt, bi: &Index) -> Value {
    if ai.format == "reproduction-v1" || bi.format == "reproduction-v1" {
        return super::reproduction::compare(a, ai, b, bi);
    }
    let mut reasons = Vec::new();
    if ai.format != bi.format || ai.parser != bi.parser {
        reasons.push("format_or_parser");
    }
    for key in [
        "targetId",
        "appId",
        "buildId",
        "imageId",
        "processId",
        "processGeneration",
        "runtimeId",
        "runtimeGeneration",
    ] {
        if a.binding[key].is_null() || b.binding[key].is_null() || a.binding[key] != b.binding[key]
        {
            reasons.push(key);
        }
    }
    if a.metadata.sampling.is_none() || a.metadata.sampling != b.metadata.sampling {
        reasons.push("sampling");
    }
    if ai.clocks.is_empty() || ai.clocks != bi.clocks || a.metadata.clocks != b.metadata.clocks {
        reasons.push("clocks");
    }
    if ai
        .rows
        .iter()
        .chain(&bi.rows)
        .any(|r| matches!(r.kind.as_str(), "retention" | "allocation"))
        && (a.metadata.heap_id.is_none() || a.metadata.heap_id != b.metadata.heap_id)
    {
        reasons.push("heap_identity");
    }
    if !a.metadata.observation_complete
        || !b.metadata.observation_complete
        || !a.metadata.source_loss.is_empty()
        || !b.metadata.source_loss.is_empty()
        || a.capture_phase != "complete"
        || b.capture_phase != "complete"
        || !ai.unsupported.is_empty()
        || !bi.unsupported.is_empty()
    {
        reasons.push("incomplete_observation");
    }
    let definitions = |index: &Index| {
        index
            .rows
            .iter()
            .map(|r| (&r.kind, &r.unit, &r.definition))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|(k, u, d)| json!([k, u, d]))
            .collect::<Vec<_>>()
    };
    if definitions(ai) != definitions(bi) {
        reasons.push("measurement_definitions_or_units");
    }
    if ai.rows.iter().chain(&bi.rows).any(|r| {
        r.value.is_some() && (r.unit.is_none() || r.unit.as_deref() == Some("producer-unspecified"))
    }) {
        reasons.push("measurement_units_unknown");
    }
    let observed_state = if [a, b].iter().all(|r| {
        matches!(
            r.capture_reason.as_deref(),
            Some("source_complete" | "android_sampled_window")
        )
    }) {
        super::state::compare(ai, bi)
    } else {
        json!({"method":"android-state-observations/1","groups":[],"reason":"capture_identity_or_finalization_unverified; use finalized snapshot sequences for before/after differences"})
    };
    json!({"compatible":reasons.is_empty(),"incompatible":reasons,"observedAndroidState":observed_state,"observedDartSnapshots":super::dart::compare(ai,bi),"baseline":{"analysisId":a.id,"rawSha256":a.raw_sha256,"observations":ai.rows.len(),"measurements":summaries(ai)},"candidate":{"analysisId":b.id,"rawSha256":b.raw_sha256,"observations":bi.rows.len(),"measurements":summaries(bi)},
        "memoryClassObservations":super::memory::compare(a,ai,b,bi),"packageSizeObservations":super::performance::compare(a,ai,b,bi),"dataSnapshotObservations":super::network_data::compare(a,ai,b,bi),"observationDelta":if reasons.is_empty(){Some(bi.rows.len() as i64-ai.rows.len() as i64)}else{None},"method":"count of recorded rows with identical declared scope and definitions; no cross-process heap matching or causal conclusion","private":true})
}
fn summaries(index: &Index) -> Value {
    let mut groups: std::collections::BTreeMap<(&str, &str, &str, &str), Vec<f64>> =
        std::collections::BTreeMap::new();
    for row in &index.rows {
        if let (Some(value), Some(unit), Some(definition)) =
            (row.value, row.unit.as_deref(), row.definition.as_deref())
        {
            groups
                .entry((&row.kind, &row.name, unit, definition))
                .or_default()
                .push(value);
        }
    }
    let total = groups.len();
    let rows:Vec<_>=groups.into_iter().take(200).map(|((kind,name,unit,definition),values)|{
        let count=values.len();let mean=values.iter().map(|n|n/count as f64).sum::<f64>();
        json!({"kind":kind,"name":name,"unit":unit,"definition":definition,"sampleCount":count,"min":values.iter().copied().reduce(f64::min),"max":values.iter().copied().reduce(f64::max),"mean":if mean.is_finite(){Some(mean)}else{None},"aggregation":"unweighted arithmetic mean of recorded scalar rows; no sum of overlapping spans"})
    }).collect();
    json!({"groups":rows,"totalGroups":total,"truncated":total>200})
}

fn assertions_view(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    view: &str,
    offset: usize,
    _limit: usize,
) -> Result<Option<Value>> {
    if view == "assertions" {
        if q.kind.is_some()
            || q.clock_domain.is_some()
            || q.from.is_some()
            || q.to.is_some()
            || q.correlation_id.is_some()
            || q.native_utid.is_some()
            || q.native_upid.is_some()
            || q.memory.is_some()
            || q.performance.is_some()
            || q.network_data.is_some()
            || q.scenario.is_some()
            || offset != 0
            || q.limit.is_some()
        {
            return Err(Error::invalid(
                "Assertions use only assertions.checks, without row filters or pagination",
            ));
        }
        let mut result = super::assertions::evaluate(
            receipt,
            index,
            q.assertions
                .as_ref()
                .ok_or_else(|| Error::invalid("view assertions requires checks"))?,
        )?;
        result["investigationId"] = json!(q.investigation_id);
        return Ok(Some(result));
    }
    if q.assertions.is_some() {
        return Err(Error::invalid("assertions options require view assertions"));
    }
    Ok(None)
}

fn scenario_view(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    view: &str,
    offset: usize,
    limit: usize,
) -> Result<Option<Value>> {
    if view == "scenario" {
        if q.kind.is_some()
            || q.clock_domain.is_some()
            || q.from.is_some()
            || q.to.is_some()
            || q.correlation_id.is_some()
            || q.native_utid.is_some()
            || q.native_upid.is_some()
            || q.memory.is_some()
            || q.performance.is_some()
            || q.network_data.is_some()
        {
            return Err(Error::invalid(
                "Scenario view uses scenario options, offset and limit",
            ));
        }
        let mut result = super::scenario::query(
            index,
            q.scenario
                .as_ref()
                .ok_or_else(|| Error::invalid("view scenario requires scenario options"))?,
            offset,
            limit,
        )?;
        result["investigationId"] = json!(q.investigation_id);
        result["analysisId"] = json!(q.analysis_id);
        result["collectorId"] = json!(receipt.collector_id);
        result["rawSha256"] = json!(receipt.raw_sha256);
        result["capturePhase"] = json!(receipt.capture_phase);
        result["captureReason"] = json!(receipt.capture_reason);
        result["sourceLoss"] = json!(receipt.metadata.source_loss);
        result["private"] = json!(true);
        return Ok(Some(result));
    }
    if q.scenario.is_some() {
        return Err(Error::invalid("scenario options require view scenario"));
    }
    Ok(None)
}

fn network_data_view(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    view: &str,
    offset: usize,
    limit: usize,
) -> Result<Option<Value>> {
    if view == "network_data" {
        if q.kind.is_some()
            || q.clock_domain.is_some()
            || q.from.is_some()
            || q.to.is_some()
            || q.correlation_id.is_some()
            || q.native_utid.is_some()
            || q.native_upid.is_some()
            || q.memory.is_some()
            || q.performance.is_some()
        {
            return Err(Error::invalid(
                "network_data view uses networkData options, offset and limit",
            ));
        }
        let mut result = super::network_data::query(
            index,
            q.network_data
                .as_ref()
                .ok_or_else(|| Error::invalid("network_data view requires networkData options"))?,
            offset,
            limit,
        )?;
        result["investigationId"] = json!(q.investigation_id);
        result["analysisId"] = json!(q.analysis_id);
        result["collectorId"] = json!(receipt.collector_id);
        result["rawSha256"] = json!(receipt.raw_sha256);
        result["capturePhase"] = json!(receipt.capture_phase);
        result["captureReason"] = json!(receipt.capture_reason);
        result["sourceLoss"] = json!(receipt.metadata.source_loss);
        result["private"] = json!(true);
        return Ok(Some(result));
    }
    if q.network_data.is_some() {
        return Err(Error::invalid(
            "networkData options require view network_data",
        ));
    }
    Ok(None)
}

fn performance_view(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    view: &str,
    offset: usize,
    limit: usize,
) -> Result<Option<Value>> {
    if view == "performance" {
        if q.kind.is_some()
            || q.clock_domain.is_some()
            || q.from.is_some()
            || q.to.is_some()
            || q.correlation_id.is_some()
            || q.native_utid.is_some()
            || q.native_upid.is_some()
            || q.memory.is_some()
        {
            return Err(Error::invalid(
                "Performance view uses performance options, offset and limit",
            ));
        }
        let mut result = super::performance::query(
            index,
            q.performance
                .as_ref()
                .ok_or_else(|| Error::invalid("view performance requires performance options"))?,
            offset,
            limit,
        )?;
        result["investigationId"] = json!(q.investigation_id);
        result["analysisId"] = json!(q.analysis_id);
        result["collectorId"] = json!(receipt.collector_id);
        result["rawSha256"] = json!(receipt.raw_sha256);
        result["capturePhase"] = json!(receipt.capture_phase);
        result["captureReason"] = json!(receipt.capture_reason);
        result["sourceLoss"] = json!(receipt.metadata.source_loss);
        result["private"] = json!(true);
        return Ok(Some(result));
    }
    if q.performance.is_some() {
        return Err(Error::invalid(
            "performance options require view performance",
        ));
    }
    Ok(None)
}

fn memory_view(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    view: &str,
    offset: usize,
    limit: usize,
) -> Result<Option<Value>> {
    if view == "memory" {
        if q.kind.is_some()
            || q.clock_domain.is_some()
            || q.from.is_some()
            || q.to.is_some()
            || q.correlation_id.is_some()
            || q.native_utid.is_some()
            || q.native_upid.is_some()
        {
            return Err(Error::invalid(
                "Memory view uses its own operation/class/object filters, offset and limit",
            ));
        }
        let mut result = super::memory::query(
            receipt,
            index,
            q.memory
                .as_ref()
                .ok_or_else(|| Error::invalid("view memory requires a memory operation"))?,
            offset,
            limit,
        )?;
        result["investigationId"] = json!(q.investigation_id);
        result["analysisId"] = json!(q.analysis_id);
        return Ok(Some(result));
    }
    if q.memory.is_some() {
        return Err(Error::invalid("memory options require view memory"));
    }
    Ok(None)
}

fn row_view(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    view: &str,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let supported: Vec<_> = index
        .rows
        .iter()
        .map(|r| r.kind.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let matches = selected_rows(index, q, view)?;
    if matches!(view, "call_tree" | "flamegraph" | "observability") {
        let mut result = if view == "observability" {
            super::observability::joins(&matches, offset, limit)?
        } else {
            super::native_trace::stacks(&matches, view, offset, limit)?
        };
        result["investigationId"] = json!(q.investigation_id);
        result["analysisId"] = json!(q.analysis_id);
        result["rawSha256"] = json!(receipt.raw_sha256);
        result["collectorId"] = json!(receipt.collector_id);
        result["unsupported"] = json!(index.unsupported);
        return Ok(result);
    }
    let mut output = Vec::new();
    let mut bytes = 0;
    for (ordinal, r) in matches.iter().skip(offset).take(limit) {
        let item = json!({"ordinal":ordinal,"record":r,"evidence":{"collectorId":receipt.collector_id,"rawSha256":receipt.raw_sha256,"source":r.source},"alignedTime":alignment(r,&receipt.metadata),"symbols":symbol_status(r,&receipt.metadata)});
        let size = serde_json::to_vec(&item).unwrap().len();
        if size > 65536 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "A projected row exceeds 64 KiB; use the explicit raw evidence reader",
            ));
        }
        if bytes + size > 262144 {
            break;
        }
        bytes += size;
        output.push(item);
    }
    let next = offset + output.len();
    Ok(
        json!({"investigationId":q.investigation_id,"analysisId":q.analysis_id,"parser":index.parser,"indexSha256":receipt.index_sha256,"binding":receipt.binding,"bindingProvenance":"caller_declared",
        "rows":output,"matched":matches.len(),"sampleCount":matches.iter().filter(|(_,r)|r.kind=="stack").count(),"sampleDefinition":"recorded stack rows; inspect sampleKind; report/backtrace rows are not CPU samples","aggregation":if view=="timeline"{"none; timestamp order in one producer clock"}else{"none; source-order rows"},"nextOffset":if next<matches.len(){Some(next)}else{None},"availableKinds":supported,"clocks":index.clocks,
        "observationComplete":receipt.metadata.observation_complete&&receipt.metadata.source_loss.is_empty()&&receipt.capture_phase=="complete"&&index.unsupported.is_empty(),
        "capturePhase":receipt.capture_phase,"captureReason":receipt.capture_reason,"sourceLoss":receipt.metadata.source_loss,"unsupported":index.unsupported,"conclusion":"observations_only; zero matches do not establish absence or health","private":true}),
    )
}
fn selected_rows<'a>(index: &'a Index, q: &Query, view: &str) -> Result<Vec<(usize, &'a Row)>> {
    if let Some(kind) = &q.kind {
        let probe = Row {
            kind: kind.clone(),
            name: "query".into(),
            timestamp: None,
            clock_domain: None,
            value: None,
            unit: None,
            definition: None,
            correlations: vec![],
            source: "/".into(),
            data: Value::Null,
        };
        probe.validate()?;
    }
    if let Some(domain) = &q.clock_domain
        && !index
            .rows
            .iter()
            .any(|r| r.clock_domain.as_ref() == Some(domain))
    {
        return Err(Error::new(
            "CLOCK_UNAVAILABLE",
            "Selected clock domain has no observations",
        ));
    }
    let selected_upid = q.native_upid.map(|id| id.to_string());
    let selected_utid = q.native_utid.map(|id| id.to_string());
    let threads = process_threads(index, selected_upid.as_deref());
    let mut matches: Vec<_> = index
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            let fields = &r.data["observation"]["fields"];
            selected_utid
                .as_ref()
                .is_none_or(|id| fields["utid"].as_str() == Some(id.as_str()))
                && selected_upid.as_ref().is_none_or(|id| {
                    fields["upid"].as_str() == Some(id.as_str())
                        || fields["utid"]
                            .as_str()
                            .is_some_and(|id| threads.contains(id))
                })
                && (view != "timeline" || r.timestamp.is_some())
                && q.kind.as_ref().is_none_or(|k| k == &r.kind)
                && q.clock_domain
                    .as_ref()
                    .is_none_or(|d| r.clock_domain.as_ref() == Some(d))
                && q.from.is_none_or(|n| r.timestamp.is_some_and(|t| t >= n))
                && q.to.is_none_or(|n| r.timestamp.is_some_and(|t| t <= n))
                && q.correlation_id
                    .as_ref()
                    .is_none_or(|id| r.correlations.contains(id))
        })
        .collect();
    if view == "timeline" {
        let domains: std::collections::BTreeSet<_> = matches
            .iter()
            .filter_map(|(_, r)| r.clock_domain.as_deref())
            .collect();
        if domains.len() > 1 {
            return Err(Error::invalid(
                "Select one clockDomain before ordering a timeline across unrelated producer clocks",
            ));
        }
        matches.sort_by(|(ai, a), (bi, b)| {
            a.timestamp
                .unwrap()
                .total_cmp(&b.timestamp.unwrap())
                .then(ai.cmp(bi))
        });
    }
    Ok(matches)
}

fn process_threads<'a>(
    index: &'a Index,
    selected_upid: Option<&str>,
) -> std::collections::BTreeSet<&'a str> {
    let threads: std::collections::BTreeSet<_> = if selected_upid.is_some() {
        index
            .rows
            .iter()
            .filter(|row| {
                row.kind == "thread"
                    && row.data["observation"]["fields"]["upid"].as_str() == selected_upid
            })
            .filter_map(|row| row.data["observation"]["fields"]["utid"].as_str())
            .collect()
    } else {
        Default::default()
    };
    threads
}
