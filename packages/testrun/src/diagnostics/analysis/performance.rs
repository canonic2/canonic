//! Source-defined startup, frame, interaction and UI observations; no implicit capture.
mod inventory;
#[cfg(test)]
mod tests;
use super::model::{Clock, Index, Metadata};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn invalid() -> Error {
    Error::new(
        "INVALID_PERFORMANCE_ARTIFACT",
        "Invalid, ambiguous or unsupported performance observation",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
fn time(n: f64) -> Result<()> {
    if !n.is_finite() || !(0.0..=9007199254740991.0).contains(&n) {
        return Err(invalid());
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Capture {
    schema_version: u32,
    runtime: String,
    build_mode: String,
    instrumentation: String,
    clocks: Vec<Clock>,
    records: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    #[serde(rename_all = "camelCase")]
    Startup {
        run_id: String,
        state: String,
        state_evidence: String,
        milestones: Vec<Milestone>,
    },
    #[serde(rename_all = "camelCase")]
    Frame {
        id: String,
        clock: String,
        start: f64,
        end: f64,
        deadline: Option<f64>,
        presented: Option<f64>,
        dropped: Option<bool>,
        thread: String,
        definition: String,
    },
    #[serde(rename_all = "camelCase")]
    Stage {
        name: String,
        clock: String,
        start: f64,
        end: f64,
        thread: String,
        definition: String,
        action_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Interaction {
        action_id: String,
        points: Vec<Milestone>,
        evidence_refs: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Tree {
        id: String,
        tree_kind: String,
        framework: String,
        nodes: Vec<inventory::Node>,
    },
    #[serde(rename_all = "camelCase")]
    Gpu {
        name: String,
        value: f64,
        unit: String,
        definition: String,
        tool: String,
        device: String,
        capture_restrictions: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Milestone {
    name: String,
    clock: String,
    at: f64,
    uncertainty: f64,
    definition: String,
}

#[expect(
    clippy::too_many_lines,
    reason = "one strict record dispatch preserves source-specific validation and measurement semantics"
)]
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    let value = super::decode::bounded_json(bytes, super::model::MAX_RAW)?;
    if format == "package-size-v1" {
        return inventory::decode(value, index);
    }
    let capture: Capture = serde_json::from_value(value).map_err(|_| invalid())?;
    if capture.schema_version != 1
        || capture.records.len() > 4096
        || capture.clocks.is_empty()
        || !matches!(
            capture.runtime.as_str(),
            "android" | "apple" | "flutter" | "react_native" | "browser" | "native" | "other"
        )
        || !matches!(
            capture.build_mode.as_str(),
            "debug" | "profile" | "release" | "unknown"
        )
    {
        return Err(invalid());
    }
    label(&capture.instrumentation)?;
    Metadata {
        clocks: capture.clocks.clone(),
        ..Metadata::default()
    }
    .validate()?;
    // These are source clocks, not caller-supplied synchronization claims.
    if capture
        .clocks
        .iter()
        .any(|c| !c.anchors.is_empty() || c.reference_domain.is_some() || c.max_drift_ppm.is_some())
    {
        return Err(invalid());
    }
    index.clocks = capture.clocks;
    let base = json!({"runtime":capture.runtime,"buildMode":capture.build_mode,"instrumentation":capture.instrumentation,"identityProvenance":"producer_declared","appId":null,"causeEstablished":false});
    let mut identities = BTreeSet::new();
    for (i, record) in capture.records.into_iter().enumerate() {
        let path = format!("/records/{i}");
        match record {
            Record::Startup {
                run_id,
                state,
                state_evidence,
                milestones,
            } => {
                if !identities.insert(("startup", run_id.clone())) {
                    return Err(invalid());
                }
                label(&run_id)?;
                label(&state_evidence)?;
                if !matches!(state.as_str(), "cold" | "warm" | "hot" | "unknown") {
                    return Err(invalid());
                }
                points(
                    index,
                    &base,
                    &path,
                    "startup",
                    &run_id,
                    &milestones,
                    &[
                        "launch_requested",
                        "process_started",
                        "runtime_ready",
                        "bundle_ready",
                        "first_display",
                        "interactive",
                        "content_ready",
                    ],
                    json!({"state":state,"stateEvidence":state_evidence}),
                )?;
            }
            Record::Interaction {
                action_id,
                points: observations,
                evidence_refs,
            } => {
                if !identities.insert(("interaction", action_id.clone())) {
                    return Err(invalid());
                }
                label(&action_id)?;
                if evidence_refs.len() > 16 {
                    return Err(invalid());
                }
                for reference in &evidence_refs {
                    label(reference)?;
                }
                points(
                    index,
                    &base,
                    &path,
                    "interaction",
                    &action_id,
                    &observations,
                    &[
                        "input_dispatched",
                        "app_received",
                        "app_responded",
                        "frame_presented",
                    ],
                    json!({"evidenceRefs":evidence_refs,"evidenceMeaning":"producer-declared screenshot/video/QA references; not resolved, digest-verified or display synchronization proof"}),
                )?;
            }
            Record::Frame {
                id,
                clock,
                start,
                end,
                deadline,
                presented,
                dropped,
                thread,
                definition,
            } => {
                if !identities.insert(("frame", id.clone())) {
                    return Err(invalid());
                }
                label(&id)?;
                label(&thread)?;
                label(&definition)?;
                let scale = interval(index, &clock, start, end)?;
                if let Some(n) = deadline {
                    time(n)?;
                    if n < start {
                        return Err(invalid());
                    }
                }
                if let Some(n) = presented {
                    time(n)?;
                    if n < end {
                        return Err(invalid());
                    }
                }
                let mut data = base.clone();
                data["performance"] = json!({"category":"frames","id":id,"thread":thread,"deadline":deadline,"presented":presented,"dropped":dropped,"missedDeadline":deadline.map(|d|end>d),"refreshBudgetMicros":deadline.map(|d|(d-start)*scale),"displayMeaning":"presented/dropped are producer observations; completion alone does not prove display or count dropped frames"});
                emit(
                    index,
                    "frame",
                    "frame completion",
                    path,
                    data,
                    Some((start, &clock)),
                    Some(((end - start) * scale, "microseconds", &definition)),
                )?;
            }
            Record::Stage {
                name,
                clock,
                start,
                end,
                thread,
                definition,
                action_id,
            } => {
                label(&name)?;
                label(&thread)?;
                label(&definition)?;
                if let Some(id) = &action_id {
                    label(id)?;
                }
                let scale = interval(index, &clock, start, end)?;
                let mut data = base.clone();
                data["performance"] = json!({"category":"stages","thread":thread,"actionId":action_id,"overlap":"inclusive intervals; do not sum overlapping/pipelined stages"});
                emit(
                    index,
                    "span",
                    &name,
                    path,
                    data,
                    Some((start, &clock)),
                    Some(((end - start) * scale, "microseconds", &definition)),
                )?;
            }
            Record::Tree {
                id,
                tree_kind,
                framework,
                nodes,
            } => {
                if !identities.insert(("tree", id.clone())) {
                    return Err(invalid());
                }
                inventory::tree(index, &base, &path, &id, &tree_kind, &framework, nodes)?;
            }
            Record::Gpu {
                name,
                value,
                unit,
                definition,
                tool,
                device,
                capture_restrictions,
            } => {
                for s in [
                    &name,
                    &unit,
                    &definition,
                    &tool,
                    &device,
                    &capture_restrictions,
                ] {
                    label(s)?;
                }
                time(value)?;
                if !matches!(
                    unit.as_str(),
                    "nanoseconds" | "microseconds" | "milliseconds" | "bytes" | "count" | "percent"
                ) || unit == "percent" && value > 100.0
                {
                    return Err(invalid());
                }
                let mut data = base.clone();
                data["performance"] = json!({"category":"gpu","tool":tool,"device":device,"captureRestrictions":capture_restrictions,"automationValidated":false,"meaning":"explicit imported producer measurement, not inferred from CPU/raster work"});
                emit(
                    index,
                    "metric",
                    &name,
                    path,
                    data,
                    None,
                    Some((value, &unit, &definition)),
                )?;
            }
        }
    }
    index.unsupported.push("Producer performance evidence: cold/warm/hot state, instrumentation and display observations are declared, not independently established. Missing milestones/frames are unavailable, not zero. GPU capture and framework instrumentation are not installed or enabled.".into());
    Ok(())
}
fn interval(index: &Index, clock: &str, start: f64, end: f64) -> Result<f64> {
    time(start)?;
    time(end)?;
    if end < start {
        return Err(invalid());
    }
    let clock = index
        .clocks
        .iter()
        .find(|c| c.id == clock)
        .ok_or_else(invalid)?;
    Ok(super::model::scale(&clock.unit).ok_or_else(invalid)? * 1e6)
}
fn emit(
    index: &mut Index,
    kind: &str,
    name: &str,
    source: String,
    data: Value,
    timestamp: Option<(f64, &str)>,
    metric: Option<(f64, &str, &str)>,
) -> Result<()> {
    let mut r = super::decode::row(kind, name, source, data);
    if let Some((at, clock)) = timestamp {
        r.timestamp = Some(at);
        r.clock_domain = Some(clock.into());
    }
    if let Some((n, unit, definition)) = metric {
        time(n)?;
        r.value = Some(n);
        r.unit = Some(unit.into());
        r.definition = Some(definition.into());
    }
    super::decode::push(index, r)
}
#[expect(
    clippy::too_many_arguments,
    reason = "one bounded interval projection carries source and explicit semantics"
)]
fn points(
    index: &mut Index,
    base: &Value,
    path: &str,
    category: &str,
    id: &str,
    observations: &[Milestone],
    allowed: &[&str],
    extra: Value,
) -> Result<()> {
    if observations.is_empty() || observations.len() > allowed.len() {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    let mut previous: Option<&Milestone> = None;
    for (i, p) in observations.iter().enumerate() {
        if !allowed.contains(&p.name.as_str()) || !names.insert(&p.name) {
            return Err(invalid());
        }
        label(&p.definition)?;
        time(p.uncertainty)?;
        interval(index, &p.clock, p.at, p.at)?;
        let mut data = base.clone();
        data["performance"] = json!({"category":category,"id":id,"point":p.name,"uncertainty":p.uncertainty,"definition":p.definition,"context":extra});
        let pointer = if category == "startup" {
            "milestones"
        } else {
            "points"
        };
        emit(
            index,
            "marker",
            &p.name,
            format!("{path}/{pointer}/{i}"),
            data.clone(),
            Some((p.at, &p.clock)),
            None,
        )?;
        if let Some(prev) = previous {
            if allowed.iter().position(|s| *s == prev.name)
                >= allowed.iter().position(|s| *s == p.name)
            {
                return Err(invalid());
            }
            data["performance"]["from"] = json!(prev.name);
            if prev.clock == p.clock {
                let scale = interval(index, &p.clock, prev.at, p.at)?;
                let duration = (p.at - prev.at) * scale;
                let uncertainty = (p.uncertainty + prev.uncertainty) * scale;
                data["performance"]["intervalRangeMicros"] =
                    json!({"lower":(duration-uncertainty).max(0.0),"upper":duration+uncertainty});
                data["performance"]["fromSource"] = json!(format!("{path}/{pointer}/{}", i - 1));
                emit(
                    index,
                    "span",
                    &format!("{} → {}", prev.name, p.name),
                    format!("{path}/{pointer}/{i}"),
                    data,
                    None,
                    Some((
                        duration,
                        "microseconds",
                        "difference between consecutive reported same-clock milestones; uncertainty range retained; missing intermediate stages not inferred",
                    )),
                )?;
            } else {
                data["performance"]["unavailable"] =
                    json!("different source clocks; no interval or display latency inferred");
                emit(
                    index,
                    "event",
                    &format!("{} → {}", prev.name, p.name),
                    format!("{path}/{pointer}/{i}"),
                    data,
                    None,
                    None,
                )?;
            }
        }
        previous = Some(p);
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Query {
    pub operation: String,
    pub min_target_size: Option<f64>,
}
pub(super) fn query(index: &Index, q: &Query, offset: usize, limit: usize) -> Result<Value> {
    if !matches!(
        q.operation.as_str(),
        "summary"
            | "startup"
            | "frames"
            | "stages"
            | "interaction"
            | "trees"
            | "accessibility"
            | "gpu"
            | "sizes"
    ) || q.min_target_size.is_some() && q.operation != "accessibility"
        || q.min_target_size
            .is_some_and(|n| !n.is_finite() || n <= 0.0 || n > 1024.0)
    {
        return Err(Error::invalid(
            "Select a performance operation; minTargetSize 0..1024 is only for accessibility",
        ));
    }
    if q.operation == "accessibility" {
        return inventory::checks(index, q.min_target_size, offset, limit);
    }
    let category = |r: &super::model::Row| {
        r.data["performance"]["category"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                if r.kind == "frame" || r.name == "frame_duration" && r.data["stateSource"] == true
                {
                    Some("frames".into())
                } else if r.kind == "span" {
                    Some("stages".into())
                } else {
                    None
                }
            })
    };
    let rows: Vec<_> = index
        .rows
        .iter()
        .filter(|r| category(r).is_some_and(|c| q.operation == "summary" || c == q.operation))
        .collect();
    let mut groups: BTreeMap<(String, String, String, String, String), Vec<f64>> = BTreeMap::new();
    // Group by source/runtime/thread and exact definition. Never combine build/raster or
    // Android completed-frame durations into an invented displayed-frame/FPS total.
    for r in &rows {
        if let (Some(n), Some(unit), Some(definition)) = (r.value, &r.unit, &r.definition) {
            let context=json!({"runtime":r.data["runtime"],"buildMode":r.data["buildMode"],"thread":r.data["performance"]["thread"],"processGeneration":r.data["processGeneration"],"isolateId":r.data["isolateId"],"runtimeGeneration":r.data["runtimeGeneration"],"state":r.data["performance"]["context"]["state"],"nativeFields":{"upid":r.data["observation"]["fields"]["upid"],"utid":r.data["observation"]["fields"]["utid"],"trackId":r.data["observation"]["fields"]["track_id"],"layerName":r.data["observation"]["fields"]["layer_name"]},"tracePid":r.data["pid"],"traceTid":r.data["tid"],"sizeBasis":r.data["performance"]["basis"],"assetPath":r.data["performance"]["path"]}).to_string();
            groups
                .entry((
                    category(r).unwrap(),
                    r.name.clone(),
                    unit.clone(),
                    definition.clone(),
                    context,
                ))
                .or_default()
                .push(n);
        }
    }
    let summaries:Vec<_>=groups.into_iter().map(|((category,name,unit,definition,context),mut values)|{
        values.sort_by(f64::total_cmp);let count=values.len();
        json!({"category":category,"name":name,"unit":unit,"definition":definition,"context":serde_json::from_str::<Value>(&context).unwrap(),"count":count,"min":values[0],"p50":values[(count-1)/2],"p95":values[(count*95).div_ceil(100)-1],"max":values[count-1],"quantileMethod":"nearest-rank (p50 uses lower median)","sum":null})
    }).collect();
    let matched = rows.len();
    let next = offset.saturating_add(limit).min(matched);
    let page: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();
    let mut result = json!({"operation":q.operation,"rows":page,"matched":matched,"nextOffset":if next<matched{Some(next)}else{None},"measurementGroups":summaries,"interpretation":"source-specific recorded rows; no inferred FPS, dropped-frame count, GPU timing, startup state, causality or accessibility conformance","unsupported":index.unsupported});
    // Measurement groups are another independently paginated collection.
    let groups = result["measurementGroups"].as_array().unwrap();
    let group_count = groups.len();
    let page: Vec<_> = groups.iter().skip(offset).take(limit).cloned().collect();
    result["measurementGroups"] = json!(page);
    result["measurementGroupCount"] = json!(group_count);
    result["nextGroupOffset"] = if offset.saturating_add(limit) < group_count {
        json!(offset + limit)
    } else {
        Value::Null
    };
    Ok(result)
}
pub(super) fn compare(
    a: &super::model::Receipt,
    ai: &Index,
    b: &super::model::Receipt,
    bi: &Index,
) -> Value {
    inventory::compare(a, ai, b, bi)
}
