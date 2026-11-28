//! Bounded multi-analysis reads and durable reports; storage and parsers stay owned elsewhere.
use super::{model::*, query};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub(super) type Source = (Receipt, Arc<Index>);
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Request {
    pub analysis_ids: Vec<String>,
    pub operation: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Witness {
    pub analysis_id: String,
    pub ordinal: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Statement {
    pub statement: String,
    #[serde(default)]
    pub witnesses: Vec<Witness>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Build {
    pub id: String,
    pub architecture: String,
    pub mode: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Comparison {
    pub baseline_analysis_id: String,
    pub corrected_analysis_id: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Report {
    pub analysis_ids: Vec<String>,
    pub privacy: Option<String>,
    pub exact_build: Option<Build>,
    pub reproduction: Vec<String>,
    pub facts: Vec<Statement>,
    pub hypotheses: Vec<Statement>,
    pub correction: Option<String>,
    pub comparison: Option<Comparison>,
    pub unresolved: Vec<String>,
}
fn invalid() -> Error {
    Error::invalid(
        "Select at most four distinct retained analyses; use diagnostics_query view investigation/report and read diagnostic-reports.md for bounded options",
    )
}
/// Most retained analyses one composed view reads.
pub(super) const MAX_SOURCES: usize = 4;
pub(super) fn ids(primary: &str, additional: &[String]) -> Result<Vec<String>> {
    let mut ids = vec![primary.to_owned()];
    let mut seen = BTreeSet::from([primary]);
    if additional.len() >= MAX_SOURCES {
        return Err(invalid());
    }
    for id in additional {
        super::super::validate_id(id, "analysis-")?;
        if !seen.insert(id) {
            return Err(invalid());
        }
        ids.push(id.clone());
    }
    Ok(ids)
}
fn label(s: &str) -> Result<()> {
    super::super::model::label(s, 1024)
}
fn bounded(v: Value) -> Result<Value> {
    if serde_json::to_vec(&v).unwrap().len() > 262144 {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Investigation result exceeds 256 KiB; reduce limit, notes or selected analyses",
        ));
    }
    Ok(v)
}
fn counts(index: &Index) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for r in &index.rows {
        *counts.entry(r.kind.as_str()).or_default() += 1;
    }
    counts
}
fn witness(source: &Source, ordinal: usize, private: bool) -> Result<Value> {
    let (r, index) = source;
    let row = index.rows.get(ordinal).ok_or_else(||Error::new("REPORT_WITNESS_UNAVAILABLE","Select an existing ordinal from diagnostics_query view rows in the named analysis; no replacement witness was used"))?;
    let mut v = json!({"analysisId":r.id,"collectorId":r.collector_id,"rawSha256":r.raw_sha256,"indexSha256":r.index_sha256,"ordinal":ordinal,"kind":row.kind});
    if private {
        v["name"] = json!(row.name);
        v["source"] = json!(row.source);
        v["value"] = json!(row.value);
        v["unit"] = json!(row.unit);
        v["definition"] = json!(row.definition);
    }
    Ok(v)
}
fn source_summary(s: &Source, private: bool) -> Value {
    let (r, index) = s;
    let mut v = json!({"analysisId":r.id,"collectorId":r.collector_id,"rawSha256":r.raw_sha256,"indexSha256":r.index_sha256,"format":r.format,"capturePhase":r.capture_phase,"availableKinds":counts(index),"sourceLossCount":r.metadata.source_loss.len(),"unsupportedSectionCount":index.unsupported.len(),"observationComplete":false});
    if private {
        v["binding"] = r.binding.clone();
        let mut builds = vec![];
        let mut seen = BTreeSet::new();
        for (ordinal, row) in index.rows.iter().enumerate() {
            let mut fields = serde_json::Map::new();
            for key in [
                "buildId",
                "imageId",
                "architecture",
                "releaseId",
                "serviceName",
            ] {
                if let Some(text) = row.data[key].as_str() {
                    fields.insert(key.into(),if text.len()<=256{json!(text)}else{json!({"sha256":super::super::digest(text.as_bytes()),"omitted":"identity longer than summary budget"})});
                }
            }
            if !fields.is_empty() && seen.insert(serde_json::to_string(&fields).unwrap()) {
                builds.push(json!({"ordinal":ordinal,"reported":fields,"provenance":"producer_reported; not verified by composition"}));
                if builds.len() == 8 {
                    break;
                }
            }
        }
        v["reportedBuilds"] = json!(builds);
        v["buildIdentityCoverage"] = json!(
            "at most eight distinct root build/image/architecture/service/version observations; nested native identities require witnessed raw/domain queries"
        );
        v["clocks"] = json!(index.clocks);
        v["sourceGaps"] = json!(index.unsupported.iter().take(8).collect::<Vec<_>>());
        v["sourceGapsTruncated"] = json!(index.unsupported.len() > 8);
        v["sourceLoss"] = json!(r.metadata.source_loss);
    }
    v
}
pub(super) fn query(
    investigation: &str,
    sources: &[Source],
    request: &Request,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    if !(1..=100).contains(&limit) {
        return Err(invalid());
    }
    let op = request.operation.as_deref().unwrap_or("summary");
    if !matches!(
        op,
        "summary"
            | "timeline"
            | "failures"
            | "stacks"
            | "resources"
            | "frames"
            | "retention"
            | "requests"
    ) {
        return Err(invalid());
    }
    let mut rows = vec![];
    for (s, (_, index)) in sources.iter().enumerate() {
        for (n, row) in index.rows.iter().enumerate() {
            let selected = match op {
                "summary" => false,
                "timeline" => row.timestamp.is_some(),
                "failures" => row.kind == "failure",
                "stacks" => row.kind == "stack",
                "resources" => matches!(row.kind.as_str(), "metric" | "process"),
                "frames" => row.kind == "frame",
                "retention" => matches!(row.kind.as_str(), "retention" | "allocation"),
                "requests" => row.kind == "span",
                _ => false,
            };
            if selected {
                rows.push((s, n, row));
            }
        }
    }
    if op == "timeline" {
        rows.sort_by(|(a, an, ar), (b, bn, br)| {
            a.cmp(b)
                .then(ar.clock_domain.cmp(&br.clock_domain))
                .then(ar.timestamp.unwrap().total_cmp(&br.timestamp.unwrap()))
                .then(an.cmp(bn))
        });
    }
    let matched = rows.len();
    let mut output = vec![];
    for (s, n, row) in rows.iter().skip(offset).take(limit) {
        let item = json!({"evidence":witness(&sources[*s],*n,true)?,"record":row,"clockScope":{"analysisId":sources[*s].0.id,"domain":row.clock_domain}});
        output.push(item);
    }
    let next = offset + output.len();
    let next_operation = if next < matched {
        json!({"tool":"diagnostics_query","arguments":{"investigationId":investigation,"analysisId":sources[0].0.id,"view":"investigation","investigation":{"analysisIds":request.analysis_ids,"operation":op},"offset":next,"limit":limit}})
    } else {
        json!({"tool":"diagnostics_query","view":"rows","purpose":"inspect a witness in its named analysis with the same investigationId"})
    };
    Ok(
        json!({"view":"investigation","investigationId":investigation,"operation":op,"sources":sources.iter().map(|s|source_summary(s,true)).collect::<Vec<_>>(),"rows":output,"matched":matched,"nextOffset":if next<matched{Some(next)}else{None},"ordering":"selected analysis order; timeline sorted within each native clock only; no cross-source clock order inferred","coverage":"selected retained analyses only; zero results do not prove absence or health","ranking":"source order; no heuristic or causal ranking","private":true,"nextOperation":next_operation}),
    )
}
pub(super) fn report(
    investigation: &str,
    question: &str,
    binding: &Value,
    sources: &[Source],
    request: &Report,
) -> Result<Value> {
    let privacy = request.privacy.as_deref().unwrap_or("minimal");
    if !matches!(privacy, "minimal" | "private") {
        return Err(invalid());
    }
    let private = privacy == "private";
    if request.reproduction.len() > 16
        || request.facts.len() > 16
        || request.hypotheses.len() > 16
        || request.unresolved.len() > 16
    {
        return Err(invalid());
    }
    for text in request
        .reproduction
        .iter()
        .chain(request.unresolved.iter())
        .chain(request.correction.iter())
    {
        label(text)?;
    }
    if let Some(b) = &request.exact_build {
        for t in [&b.id, &b.architecture, &b.mode] {
            label(t)?;
        }
        if !matches!(b.mode.as_str(), "debug" | "profile" | "release" | "unknown") {
            return Err(invalid());
        }
    }
    let statements = |items: &[Statement], facts: bool| -> Result<Vec<Value>> {
        items.iter().map(|s|{label(&s.statement)?;if s.witnesses.len()>8||facts&&s.witnesses.is_empty(){return Err(Error::invalid("Every reported fact requires 1..8 selected evidence witnesses; hypotheses may have no witnesses"));}let ws=s.witnesses.iter().map(|w|{let source=sources.iter().find(|s|s.0.id==w.analysis_id).ok_or_else(invalid)?;witness(source,w.ordinal,private)}).collect::<Result<Vec<_>>>()?;Ok(json!({"statement":if private{json!(s.statement)}else{json!("omitted_by_projection")},"attribution":if facts{"caller_observation; witness existence verified, statement truth not inferred"}else{"caller_hypothesis; not established cause"},"witnesses":ws}))}).collect()
    };
    let facts = statements(&request.facts, true)?;
    let hypotheses = statements(&request.hypotheses, false)?;
    let comparison = if let Some(pair) = &request.comparison {
        let a = sources
            .iter()
            .find(|s| s.0.id == pair.baseline_analysis_id)
            .ok_or_else(invalid)?;
        let b = sources
            .iter()
            .find(|s| s.0.id == pair.corrected_analysis_id)
            .ok_or_else(invalid)?;
        if a.0.id == b.0.id {
            return Err(invalid());
        }
        let value = query::compare(&a.0, &a.1, &b.0, &b.1);
        if private {
            value
        } else {
            json!({"baselineAnalysisId":a.0.id,"correctedAnalysisId":b.0.id,"compatible":value["compatible"],"measurementDetails":"omitted_by_projection; inspect diagnostics_compare privately","causalityEstablished":false})
        }
    } else {
        json!({"state":"not_supplied"})
    };
    let observed=sources.iter().map(|s|{let ordinals=s.1.rows.iter().enumerate().filter(|(_,r)|matches!(r.kind.as_str(),"failure"|"metric"|"span"|"stack"|"retention"|"frame")).take(8).map(|(n,_)|witness(s,n,private)).collect::<Result<Vec<_>>>()?;Ok(json!({"analysisId":s.0.id,"counts":counts(&s.1),"witnesses":ordinals,"meaning":"counts of recorded rows; not distinct incidents, prevalence, completeness or health"}))}).collect::<Result<Vec<_>>>()?;
    let build = if private {
        json!({"claim":request.exact_build,"provenance":"caller_declared; inspect each source binding and native record for actual build identity","verified":false})
    } else {
        json!({"supplied":request.exact_build.is_some(),"idSha256":request.exact_build.as_ref().map(|b|super::super::digest(b.id.as_bytes())),"identity":"omitted_by_projection","verified":false})
    };
    let text_list = |texts: &[String]| {
        if private {
            json!(texts)
        } else {
            json!({"count":texts.len(),"text":"omitted_by_projection"})
        }
    };
    bounded(
        json!({"schemaVersion":1,"projection":"investigation-report-v1","privacy":privacy,"investigationId":investigation,"question":if private{json!(question)}else{json!("omitted_by_projection")},"scope":if private{json!({"binding":binding,"provenance":"caller_declared","verified":false})}else{json!({"kind":"selected_analyses","verified":false,"binding":"omitted_by_projection"})},"exactBuild":build,"reproduction":text_list(&request.reproduction),"observed":observed,"facts":facts,"hypotheses":hypotheses,"correction":if private{json!(request.correction)}else{json!({"supplied":request.correction.is_some(),"text":"omitted_by_projection"})},"comparison":comparison,"unresolved":text_list(&request.unresolved),"sources":sources.iter().map(|s|source_summary(s,private)).collect::<Vec<_>>(),"limits":["selected retained observations only","source loss, sampling, retention and missing build/clock identity can limit findings","caller statements and imported scope are not authenticated conclusions","no causal proof or healthy-absence inference"],"omitted":if private{vec!["raw row data, bodies, heaps, traces and attachment contents"]}else{vec!["question","binding","names","paths","clock labels","measurement values/definitions","free text notes","build identity","comparison details","raw row data"]},"remoteTransmission":false,"nextOperation":{"tool":"diagnostics_export","formats":["investigation-json","investigation-markdown"],"scope":"all","purpose":"save a durable caller-owned report"}}),
    )
}
pub(crate) fn markdown(report: &Value) -> Result<Vec<u8>> {
    let mut out = String::from("# Investigation report\n\n");
    for key in [
        "schemaVersion",
        "projection",
        "privacy",
        "investigationId",
        "question",
        "scope",
        "exactBuild",
        "reproduction",
        "observed",
        "facts",
        "hypotheses",
        "correction",
        "comparison",
        "unresolved",
        "sources",
        "limits",
        "omitted",
    ] {
        out.push_str(&format!(
            "## {key}\n\n```json\n{}\n```\n\n",
            serde_json::to_string_pretty(&report[key])
                .map_err(|_| invalid())?
                .replace('`', "\\u0060")
                .replace('<', "\\u003c")
        ));
    }
    if out.len() > 524288 {
        return Err(Error::new(
            "EXPORT_BOUNDS",
            "Markdown report exceeds 512 KiB; reduce notes or selected analyses",
        ));
    }
    Ok(out.into_bytes())
}
pub(crate) fn schema(report: bool) -> Value {
    let object = |properties: Value, required: Vec<&str>| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let text = json!({"type":"string","minLength":1,"maxLength":1024});
    let id = json!({"type":"string","minLength":1,"maxLength":64});
    let ids = json!({"type":"array","maxItems":3,"uniqueItems":true,"items":id});
    if !report {
        return object(
            json!({"analysisIds":ids,"operation":{"enum":["summary","timeline","failures","stacks","resources","frames","retention","requests"],"default":"summary"}}),
            vec![],
        );
    }
    let witness = object(
        json!({"analysisId":id,"ordinal":{"type":"integer","minimum":0,"maximum":19999}}),
        vec!["analysisId", "ordinal"],
    );
    let statement = object(
        json!({"statement":text,"witnesses":{"type":"array","maxItems":8,"items":witness}}),
        vec!["statement"],
    );
    let texts = json!({"type":"array","maxItems":16,"items":text});
    object(
        json!({"analysisIds":ids,"privacy":{"enum":["minimal","private"],"default":"minimal"},"exactBuild":object(json!({"id":text,"architecture":text,"mode":{"enum":["debug","profile","release","unknown"]}}),vec!["id","architecture","mode"]),"reproduction":texts,"facts":{"type":"array","maxItems":16,"items":statement},"hypotheses":{"type":"array","maxItems":16,"items":statement},"correction":text,"comparison":object(json!({"baselineAnalysisId":id,"correctedAnalysisId":id}),vec!["baselineAnalysisId","correctedAnalysisId"]),"unresolved":texts}),
        vec![],
    )
}
#[cfg(test)]
mod tests;
