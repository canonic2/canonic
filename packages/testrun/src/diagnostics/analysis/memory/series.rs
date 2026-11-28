//! App-owned repeated-action/resource observations, never automatic intervention.
use super::*;
use std::io::Read;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Probe {
    pub collector_id: String,
    pub sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Counter {
    resource: String,
    value: u64,
    unit: String,
    source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Sample {
    phase: String,
    iteration: u32,
    elapsed_micros: u64,
    action_id: Option<String>,
    #[serde(default)]
    probe_ids: Vec<String>,
    counters: Vec<Counter>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Series {
    schema_version: u32,
    runtime: String,
    process_generation: String,
    build_id: String,
    #[serde(default)]
    probe_refs: Vec<Probe>,
    samples: Vec<Sample>,
}
pub(super) fn references(bytes: &[u8]) -> Result<Vec<Probe>> {
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = vec![];
        flate2::read::MultiGzDecoder::new(bytes)
            .take((super::super::model::MAX_RAW + 1) as u64)
            .read_to_end(&mut out)
            .map_err(|_| invalid())?;
        out
    } else {
        bytes.to_vec()
    };
    let value = super::super::decode::bounded_json(&bytes, super::super::model::MAX_RAW)?;
    let refs: Vec<Probe> =
        serde_json::from_value(value.get("probeRefs").cloned().unwrap_or(json!([])))
            .map_err(|_| invalid())?;
    if refs.len() > 32 {
        return Err(invalid());
    }
    for p in &refs {
        crate::diagnostics::validate_id(&p.collector_id, "collector-")?;
        if p.sha256.len() != 64 || !p.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
    }
    Ok(refs)
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let series: Series = serde_json::from_value(super::super::decode::bounded_json(
        bytes,
        super::super::model::MAX_RAW,
    )?)
    .map_err(|_| invalid())?;
    references(bytes)?;
    if series.schema_version != 1 || series.samples.len() > 512 || series.samples.is_empty() {
        return Err(invalid());
    }
    for s in [
        &series.runtime,
        &series.process_generation,
        &series.build_id,
    ] {
        label(s, 256)?;
    }
    let ids: BTreeSet<_> = series
        .probe_refs
        .iter()
        .map(|p| p.collector_id.as_str())
        .collect();
    if ids.len() != series.probe_refs.len() {
        return Err(invalid());
    }
    let (mut baseline, mut iteration, mut pending, mut released, mut clock) =
        (false, 0u32, None::<String>, false, 0u64);
    index.clocks.push(super::super::model::Clock {
        id: "memory-series:elapsed".into(),
        unit: "microseconds".into(),
        reference_domain: None,
        max_drift_ppm: None,
        anchors: vec![],
    });
    for (i, sample) in series.samples.iter().enumerate() {
        if sample.elapsed_micros < clock
            || sample.elapsed_micros > 9007199254740991
            || sample.counters.len() > 32
            || sample.probe_ids.len() > 32
        {
            return Err(invalid());
        }
        clock = sample.elapsed_micros;
        for id in &sample.probe_ids {
            if !ids.contains(id.as_str()) {
                return Err(invalid());
            }
        }
        match sample.phase.as_str() {
            "warmup" if !baseline && sample.iteration == 0 => {}
            "baseline" if !baseline && sample.iteration == 0 => {
                baseline = true;
            }
            "after_action"
                if baseline && pending.is_none() && sample.iteration == iteration + 1 =>
            {
                let action = sample.action_id.as_deref().ok_or_else(invalid)?;
                label(action, 256)?;
                iteration += 1;
                pending = Some(action.into());
                released = false;
            }
            "release"
                if pending.is_some()
                    && sample.iteration == iteration
                    && sample.action_id == pending =>
            {
                pending = None;
                released = true;
            }
            "cleanup" if released && sample.iteration == iteration => {
                released = false;
            }
            _ => {
                return Err(Error::new(
                    "MEMORY_SERIES_ORDER",
                    "Use warmup, one baseline, then ordered after_action/release and optional cleanup observations per iteration",
                ));
            }
        }
        counters(&series, sample, i, index)?;
    }
    if !baseline {
        return Err(invalid());
    }
    let summary = json!({"runtime":series.runtime,"processGeneration":series.process_generation,"buildId":series.build_id,"iterations":iteration,"actionAwaitingRelease":pending,"probeRefs":series.probe_refs,"referenceProvenance":"public analysis verifies finalized local collector digests; producer semantics and ownership remain unverified","interventionsPerformed":[],"leakEstablished":false});
    super::super::decode::push(
        index,
        super::super::decode::row(
            "event",
            "Memory resource series",
            "/".into(),
            json!({"appId":null,"memorySeriesSummary":summary}),
        ),
    )?;
    index.unsupported.push("Resource counters/release points are producer assertions; retained growth or persistent RSS alone does not prove a leak; require supported retention paths and expected release evidence".into());
    Ok(())
}
pub(super) fn query(index: &Index) -> Result<Value> {
    let summary = index
        .rows
        .iter()
        .find_map(|r| r.data.get("memorySeriesSummary"))
        .ok_or_else(|| {
            Error::new(
                "MEMORY_SERIES_UNAVAILABLE",
                "Select memory-series-v1 evidence",
            )
        })?;
    let mut baselines = BTreeMap::new();
    let mut rows = vec![];
    for (i, r) in index.rows.iter().enumerate() {
        let Some(m) = r.data.get("memoryResource") else {
            continue;
        };
        let key = (
            r.name.clone(),
            r.unit.clone().unwrap_or_default(),
            m["source"].to_string(),
        );
        if m["phase"] == "baseline" {
            baselines.insert(key, r.value.unwrap());
        } else if matches!(m["phase"].as_str(), Some("release" | "cleanup")) {
            rows.push(json!({"resource":r.name,"unit":r.unit,"source":m["source"],"phase":m["phase"],"iteration":m["iteration"],"value":r.value,"baselineDelta":baselines.get(&key).map(|b|r.value.unwrap()-b),"witnessOrdinal":i,"probeIds":m["probeIds"]}));
        }
    }
    Ok(
        json!({"summary":summary,"rows":rows,"method":"observed resource quantities after declared release; no leak attribution or allocator/GC causality","leakEstablished":false}),
    )
}

fn counters(series: &Series, sample: &Sample, i: usize, index: &mut Index) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (j, counter) in sample.counters.iter().enumerate() {
        if !matches!(
            counter.resource.as_str(),
            "views"
                | "textures"
                | "buffers"
                | "file_handles"
                | "sockets"
                | "subscriptions"
                | "timers"
                | "threads"
                | "child_processes"
                | "heap_objects"
                | "native_allocations"
                | "heap"
                | "rss"
                | "external"
        ) || !matches!(counter.unit.as_str(), "count" | "bytes")
            || counter.value > 9007199254740991
            || matches!(counter.resource.as_str(), "heap" | "rss" | "external")
                && counter.unit != "bytes"
            || !seen.insert((&counter.resource, &counter.unit))
        {
            return Err(invalid());
        }
        label(&counter.source, 256)?;
        let mut row = super::super::decode::row(
            "metric",
            &counter.resource,
            format!("/samples/{i}/counters/{j}"),
            json!({"appId":null,"memoryResource":{"phase":sample.phase,"iteration":sample.iteration,"runtime":series.runtime,"processGeneration":series.process_generation,"buildId":series.build_id,"source":counter.source,"probeIds":sample.probe_ids},"leakEstablished":false}),
        );
        row.timestamp = Some(sample.elapsed_micros as f64);
        row.clock_domain = Some("memory-series:elapsed".into());
        row.value = Some(counter.value as f64);
        row.unit = Some(counter.unit.clone());
        row.definition=Some("producer-observed resource quantity at a declared action/release point; no intervention, steady-state or leak inference".into());
        row.correlations = sample.action_id.iter().cloned().collect();
        super::super::decode::push(index, row)?;
    }
    Ok(())
}
