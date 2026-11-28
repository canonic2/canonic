//! Retained derived analysis. Raw collectors remain the sole evidence owners.
pub(super) mod assertions;
mod dart;
pub(super) mod decode;
mod frameworks;
pub(super) mod investigation;
mod memory;
mod native_trace;
mod network_data;
mod observability;
mod performance;
mod reproduction;
mod runtime_checks;
mod scenario;
mod services;
mod symbolize;
mod test_results;
pub(crate) fn apple_json(bytes: &[u8]) -> Result<Value> {
    decode::bounded_json(bytes, model::MAX_RAW)
}
/// Largest VM Service reply a Dart capture receives. The collector reduces large replies
/// before retaining them: a small Debug Flutter app's allocation profile was about 5 MB
/// (782,000 tokens) and one second of its CPU samples about 17 MB.
pub(crate) const MAX_VM_REPLY: usize = 32 * 1024 * 1024;
/// One VM Service reply within [`MAX_VM_REPLY`], with a proportional token budget.
pub(crate) fn vm_service_json(bytes: &[u8]) -> Result<Value> {
    decode::bounded_json_tokens(bytes, MAX_VM_REPLY, 8_000_000)
}
pub(crate) mod model;
mod query;
mod rebuild;
mod state;
#[cfg(test)]
mod tests;
pub(crate) mod worker;
use super::*;
use model::{Index, MAX_INDEX, Metadata, PARSER, Receipt};
use rebuild::{REBUILD, index_generation};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Prepare {
    investigation_id: String,
    collector_id: String,
    submission_id: String,
    #[serde(default)]
    metadata: Metadata,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SelectAnalysis {
    investigation_id: String,
    analysis_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Compare {
    investigation_id: String,
    analysis_id: String,
    other_investigation_id: String,
    other_analysis_id: String,
}
pub(crate) struct AnalysisWork {
    dir: PathBuf,
    receipt: Receipt,
    bytes: Vec<u8>,
    cancelled: watch::Receiver<Option<String>>,
    began: Option<Instant>,
}
fn receipt_path(dir: &Path, id: &str) -> Result<PathBuf> {
    validate_id(id, "analysis-")?;
    Ok(dir.join(format!("{id}.json")))
}
fn index_path(dir: &Path, id: &str) -> Result<PathBuf> {
    validate_id(id, "analysis-")?;
    Ok(dir.join(format!("{id}.index.json")))
}
fn read_private(path: &Path, max: usize) -> Result<Vec<u8>> {
    artifacts::check_private_directory(path.parent().unwrap())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| Error::new("ANALYSIS_NOT_FOUND", "Retained analysis file unavailable"))?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= max as u64)
    {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Invalid retained analysis file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot read private analysis"))?;
    if bytes.len() > max {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Analysis exceeds retained bounds",
        ));
    }
    Ok(bytes)
}
fn load_receipt(dir: &Path, id: &str) -> Result<Receipt> {
    let path = receipt_path(dir, id)?;
    let bytes = read_private(&path, 128 * 1024)?;
    let record: Receipt = storage::versioned(&bytes, &path, "analysis receipt", 1)?;
    if record.id != id || record.raw_bytes > model::MAX_RAW as u64 || record.raw_sha256.len() != 64
    {
        return Err(Error::new(
            "STATE_IO",
            "Analysis receipt has invalid identity or bounds",
        ));
    }
    validate_id(&record.collector_id, "collector-")?;
    record.metadata.validate()?;
    Ok(record)
}
fn check_clocks(metadata: &Metadata, index: &Index) -> Result<()> {
    for clock in &metadata.clocks {
        if !index
            .clocks
            .iter()
            .any(|source| source.id == clock.id && source.unit == clock.unit)
        {
            return Err(Error::new(
                "CLOCK_MISMATCH",
                "Alignment must match an observed source clock and its exact unit",
            ));
        }
    }
    Ok(())
}
fn save(dir: &Path, receipt: &Receipt) -> Result<()> {
    storage::write(&receipt_path(dir, &receipt.id)?, receipt)
}
fn public_receipt(r: &Receipt) -> Value {
    super::privacy::ordinary(
        json!({"analysisId":r.id,"collectorId":r.collector_id,"phase":r.phase,"settled":!matches!(r.phase.as_str(),"preparing"|"running"|"stopping"|"cleanup_pending"),"reason":r.reason,"cost":r.cost,
        "rawSha256":r.raw_sha256,"bytes":r.raw_bytes,"format":r.format,"schemaVersion":1,"parser":PARSER,"indexSha256":r.index_sha256,"capturePhase":r.capture_phase,"captureReason":r.capture_reason,
        "privacy":"private_raw_and_derived","provenance":"retained collector; interpretation metadata caller_declared","observationComplete":r.phase=="complete"&&r.unsupported.is_empty()&&r.metadata.observation_complete&&r.metadata.source_loss.is_empty()&&r.capture_phase=="complete",
        "sourceLoss":r.metadata.source_loss,"unsupported":r.unsupported.iter().take(8).collect::<Vec<_>>(),"unsupportedCount":r.unsupported.len(),"unsupportedTruncated":r.unsupported.len()>8,"nextOperation":super::guidance::analysis_next(&r.id,!matches!(r.phase.as_str(),"preparing"|"running"|"stopping"|"cleanup_pending"),r.phase=="complete"),"limits":{"rawBytes":model::MAX_RAW,"expandedBytes":model::MAX_RAW,"indexBytes":MAX_INDEX,"rows":model::MAX_ROWS,"depth":64,"structuralTokens":300000,"wallTimeMs":10000,"symbolicationWallTimeMs":if r.metadata.symbolicate{10000}else{0}},"symbolication":if r.metadata.symbolicate{"explicit_uuid_arch_checked_atos; inspect frames and gaps"}else{"references_only"}}),
    )
}
impl Owners {
    pub(crate) fn prepare_analysis(
        &self,
        root: &Path,
        workspace: &str,
        args: &Value,
    ) -> Result<(Value, Option<AnalysisWork>)> {
        let p: Prepare = parse(args)?;
        p.metadata.validate()?;
        let dir = directory(root, workspace, &p.investigation_id)?;
        let aid = id("analysis-", &p.submission_id)?;
        let signature = digest(&serde_json::to_vec(args).unwrap());
        let path = receipt_path(&dir, &aid)?;
        let mut state = self.state.lock().unwrap();
        let record = self.load(&mut state, &dir)?;
        if path.exists() {
            let r = load_receipt(&dir, &aid)?;
            if r.signature != signature {
                return Err(Error::new(
                    "REQUEST_CONFLICT",
                    "Analysis submissionId has different inputs",
                ));
            }
            return Ok((
                public_receipt(&self.recover_analysis(&state, &dir, r)?),
                None,
            ));
        }
        let c = record
            .collectors
            .iter()
            .find(|c| c.id == p.collector_id)
            .ok_or_else(|| {
                Error::new(
                    "DIAGNOSTIC_NOT_FOUND",
                    "Collector does not belong to this investigation",
                )
            })?;
        if !c.phase.settled() || c.sha256.is_none() {
            return Err(Error::new(
                "DIAGNOSTIC_BUSY",
                "Wait for immutable finalized evidence before analysis",
            ));
        }
        validate_symbols(record, &dir, &p.metadata)?;
        let bytes = read_data(&dir, &c.id)?;
        if Some(digest(&bytes)).as_ref() != c.sha256.as_ref() {
            return Err(Error::new(
                "EVIDENCE_CHANGED",
                "Raw evidence changed before analysis",
            ));
        }
        let format = c.source.format().to_string();
        if format == "memory-series-v1" {
            validate_memory_probes(record, &dir, &bytes)?;
        }
        let receipt = Receipt {
            version: 1,
            id: aid.clone(),
            collector_id: c.id.clone(),
            signature,
            owner: self.incarnation.clone(),
            phase: "preparing".into(),
            raw_sha256: c.sha256.clone().unwrap(),
            raw_bytes: c.bytes,
            format,
            capture_phase: serde_json::to_value(c.phase)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            capture_reason: c.reason.clone(),
            binding: serde_json::to_value(&record.binding).unwrap(),
            metadata: p.metadata,
            index_sha256: None,
            unsupported: vec![],
            reason: None,
            cost: None,
        };
        analysis_capacity(&state, &dir)?;
        save(&dir, &receipt)?;
        let (sender, cancelled) = watch::channel(None);
        state.running.insert(dir.join(&aid), sender);
        Ok((
            public_receipt(&receipt),
            Some(AnalysisWork {
                dir,
                receipt,
                bytes,
                cancelled,
                began: None,
            }),
        ))
    }
    fn recover_analysis(&self, state: &State, dir: &Path, mut r: Receipt) -> Result<Receipt> {
        if matches!(
            r.phase.as_str(),
            "preparing" | "running" | "cleanup_pending"
        ) && !state.running.contains_key(&dir.join(&r.id))
        {
            r.phase = "failed".into();
            r.reason = Some(
                if r.owner != self.incarnation {
                    "OWNER_LOST"
                } else {
                    "FINALIZATION_LOST"
                }
                .into(),
            );
            if !self.offline {
                save(dir, &r)?;
            }
        }
        Ok(r)
    }
    pub(crate) async fn run_analysis(&self, mut work: AnalysisWork) {
        use futures_util::FutureExt;
        work.began = Some(Instant::now());
        work.receipt.phase = "running".into();
        let result = std::panic::AssertUnwindSafe(async {
            match save(&work.dir, &work.receipt) {
                Ok(()) => {
                    let mut index =
                        worker::run(&work.receipt.format, &work.bytes, &mut work.cancelled).await?;
                    if work.receipt.metadata.symbolicate {
                        symbolize::apply(
                            &work.dir,
                            &mut index,
                            &work.receipt.metadata,
                            &work.cancelled,
                        )
                        .await?;
                    }
                    Ok(index)
                }
                Err(e) => Err(e),
            }
        })
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            Err(Error::new(
                "ANALYSIS_FAILED",
                "Parser supervision stopped unexpectedly",
            ))
        });
        self.finish_analysis(work, result);
    }
    fn finish_analysis(&self, mut work: AnalysisWork, result: Result<Index>) {
        let result = result.and_then(|index| {
            check_clocks(&work.receipt.metadata, &index)?;
            let raw = read_data(&work.dir, &work.receipt.collector_id)?;
            if digest(&raw) != work.receipt.raw_sha256 {
                return Err(Error::new(
                    "EVIDENCE_CHANGED",
                    "Raw evidence changed during analysis",
                ));
            }
            let bytes = serde_json::to_vec(&index)
                .map_err(|_| Error::new("ANALYSIS_FAILED", "Cannot encode derived index"))?;
            if bytes.len() > MAX_INDEX {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "Derived index exceeds its budget",
                ));
            }
            artifacts::write(&index_path(&work.dir, &work.receipt.id)?, &bytes)?;
            Ok((digest(&bytes), bytes.len() as u64, index))
        });
        let mut state = self.state.lock().unwrap();
        match result {
            Ok((hash, index_bytes, index)) => {
                work.receipt.phase = "complete".into();
                work.receipt.index_sha256 = Some(hash.clone());
                work.receipt.unsupported = index.unsupported.clone();
                work.receipt.cost = work.began.map(|began| model::AnalysisCost {
                    run_wall_ms: began.elapsed().as_secs_f64() * 1000.0,
                    raw_bytes: work.receipt.raw_bytes,
                    index_bytes,
                });
                if state.analysis_indexes.len() >= 4 {
                    state.analysis_indexes.clear();
                }
                state
                    .analysis_indexes
                    .insert(work.dir.join(&work.receipt.id), (hash, Arc::new(index)));
            }
            Err(e) => {
                work.receipt.phase = if e.code == "ANALYSIS_CANCELLED" {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                work.receipt.reason = Some(e.code);
            }
        }
        let _ = save(&work.dir, &work.receipt);
        state.running.remove(&work.dir.join(&work.receipt.id));
    }
    fn retained_analysis(
        &self,
        state: &mut State,
        root: &Path,
        workspace: &str,
        investigation: &str,
        aid: &str,
    ) -> Result<(Receipt, Arc<Index>)> {
        let dir = directory(root, workspace, investigation)?;
        self.load(state, &dir)?;
        let receipt = self.recover_analysis(state, &dir, load_receipt(&dir, aid)?)?;
        if receipt.phase != "complete" {
            return Err(Error::new(
                "ANALYSIS_NOT_READY",
                "Inspect analysis status; queries require a complete derived index",
            ));
        }
        let raw = read_data(&dir, &receipt.collector_id)?;
        if digest(&raw) != receipt.raw_sha256 {
            return Err(Error::new("EVIDENCE_CHANGED", "Raw evidence changed"));
        }
        let path = index_path(&dir, aid)?;
        let mut bytes = read_private(&path, MAX_INDEX)?;
        if Some(digest(&bytes)) != receipt.index_sha256 {
            bytes = self.resume_rebuilt_index(&dir, aid, &receipt)?;
        }
        if let Some((hash, index)) = state.analysis_indexes.get(&dir.join(aid))
            && Some(hash) == receipt.index_sha256.as_ref()
        {
            return Ok((receipt, index.clone()));
        }
        if index_generation(&bytes, &path)? < model::PARSER_GENERATION {
            return Err(Error::new(REBUILD, "Derived index needs rebuilding")
                .details(json!({"investigationId":investigation,"analysisId":aid})));
        }
        let index: Index = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new("STATE_IO", "Derived index is damaged"))?;
        if index.version != 1 || index.parser != PARSER || index.rows.len() > model::MAX_ROWS {
            return Err(Error::new("STATE_IO", "Derived index is damaged"));
        }
        for row in &index.rows {
            row.validate()?;
        }
        let index = Arc::new(index);
        if state.analysis_indexes.len() >= 4 {
            state.analysis_indexes.clear();
        }
        state.analysis_indexes.insert(
            dir.join(aid),
            (receipt.index_sha256.clone().unwrap(), index.clone()),
        );
        Ok((receipt, index))
    }
    /// Answers an analysis inspection. An index written by an earlier parser is
    /// rebuilt from its verified raw evidence outside the state lock, and the
    /// request then continues from the rebuilt index.
    pub(crate) fn inspect_analysis(
        &self,
        root: &Path,
        workspace: &str,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        // One request reads at most MAX_SOURCES analyses; each rebuilds once.
        for _ in 0..=investigation::MAX_SOURCES {
            let stale = {
                let mut state = self.state.lock().unwrap();
                match self.answer_analysis(&mut state, root, workspace, name, args) {
                    Err(error) if error.code == REBUILD => error.details.unwrap_or_default(),
                    result => return result,
                }
            };
            let (Some(investigation_id), Some(analysis_id)) = (
                stale["investigationId"].as_str(),
                stale["analysisId"].as_str(),
            ) else {
                unreachable!("rebuild requests name their analysis")
            };
            self.rebuild_index(root, workspace, investigation_id, analysis_id)?;
        }
        Err(Error::new(
            "DIAGNOSTIC_BUSY",
            "Derived indexes kept changing while rebuilding; retry the request",
        ))
    }
    #[expect(
        clippy::too_many_lines,
        reason = "one dispatch over the analysis inspection tools and their shared validation"
    )]
    fn answer_analysis(
        &self,
        state: &mut State,
        root: &Path,
        workspace: &str,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        if matches!(name, "diagnostics_project" | "diagnostics_export") {
            let p: super::projection::Request = parse(args)?;
            if matches!(
                p.format.as_deref(),
                Some("investigation-json" | "investigation-markdown")
            ) {
                if name != "diagnostics_export"
                    || !matches!(p.scope, super::projection::Scope::All)
                    || p.kind.is_some()
                    || p.cursor.is_some()
                    || p.limit.is_some()
                {
                    return Err(Error::invalid(
                        "Investigation report export requires scope all without row filters; select report.privacy minimal or explicit private",
                    ));
                }
                let options = p.report.as_ref().ok_or_else(|| {
                    Error::invalid(
                        "Investigation report export requires report options ({} for defaults)",
                    )
                })?;
                let report = self.investigation_report(
                    state,
                    root,
                    workspace,
                    &p.investigation_id,
                    &p.analysis_id,
                    options,
                )?;
                let path = p
                    .path
                    .as_deref()
                    .ok_or_else(|| Error::invalid("Report export requires an absolute path"))?;
                return super::projection::export_report(
                    root,
                    path,
                    &report,
                    p.format.as_deref().unwrap(),
                );
            }
            if p.report.is_some() {
                return Err(Error::invalid(
                    "report options require format investigation-json or investigation-markdown",
                ));
            }
            if name == "diagnostics_project" && p.format.is_some() {
                return Err(Error::invalid("format is an export-only option"));
            }
            if p.format.as_deref() == Some("native_artifact") {
                let dir = directory(root, workspace, &p.investigation_id)?;
                self.load(state, &dir)?;
                let receipt = load_receipt(&dir, &p.analysis_id)?;
                if name != "diagnostics_export"
                    || !matches!(p.scope, super::projection::Scope::All)
                    || p.kind.is_some()
                    || p.cursor.is_some()
                    || p.limit.is_some()
                    || receipt.format != "native-trace-v1"
                {
                    return Err(Error::invalid(
                        "Native artifact export requires diagnostics_export, scope all and a native-trace-v1 analysis without row filters",
                    ));
                }
                let path = p
                    .path
                    .as_deref()
                    .ok_or_else(|| Error::invalid("Export requires path"))?;
                let raw = read_data(&dir, &receipt.collector_id)?;
                if digest(&raw) != receipt.raw_sha256 {
                    return Err(Error::new(
                        "EVIDENCE_CHANGED",
                        "Retained native evidence changed",
                    ));
                }
                let (bytes, encoding) = decode::native_trace::artifact_bytes(&raw)?;
                let mut result = super::projection::export_native(root, path, &bytes)?;
                result["encoding"] = json!(encoding);
                return Ok(result);
            }
            if p.format.as_deref().is_some_and(|v| v != "minimal-v1") {
                return Err(Error::invalid(
                    "Export format must be minimal-v1 or native_artifact",
                ));
            }
            let (receipt, index) = self.retained_analysis(
                state,
                root,
                workspace,
                &p.investigation_id,
                &p.analysis_id,
            )?;
            let report = super::projection::project(&receipt, &index, &p)?;
            if name == "diagnostics_export" {
                let path = p
                    .path
                    .as_deref()
                    .ok_or_else(|| Error::invalid("Export requires an explicit path"))?;
                return super::projection::export(root, path, &report);
            }
            let mut report = report;
            if report["nextCursor"].is_string() {
                // Same continuation shape as diagnostics_query.
                let mut next = crate::pagination::filters(args);
                next["cursor"] = report["nextCursor"].clone();
                next["limit"] = json!(p.limit.unwrap_or(50));
                report["nextOperation"] = json!({"tool":"diagnostics_project","arguments":next,"required":"preserve the same scope and kind"});
            }
            return Ok(report);
        }
        if name == "diagnostics_query" {
            let mut q: query::Query = parse(args)?;
            q.wire_query = crate::pagination::filters(args);
            if matches!(q.view.as_deref(), Some("investigation" | "report")) {
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
                    || q.assertions.is_some()
                {
                    return Err(Error::invalid(
                        "Composed investigation/report views use only their own options, offset and limit",
                    ));
                }
                if q.view.as_deref() == Some("report") {
                    if q.investigation.is_some() || q.cursor.is_some() || q.limit.is_some() {
                        return Err(Error::invalid(
                            "Report view uses report options without pagination",
                        ));
                    }
                    return self.investigation_report(
                        state,
                        root,
                        workspace,
                        &q.investigation_id,
                        &q.analysis_id,
                        q.report.as_ref().ok_or_else(|| {
                            Error::invalid("view report requires report options ({} for defaults)")
                        })?,
                    );
                }
                if q.report.is_some() {
                    return Err(Error::invalid("report options require view report"));
                }
                let options = q.investigation.as_ref().ok_or_else(|| {
                    Error::invalid(
                        "view investigation requires investigation options ({} for summary)",
                    )
                })?;
                let sources = self.investigation_sources(
                    state,
                    root,
                    workspace,
                    &q.investigation_id,
                    &q.analysis_id,
                    &options.analysis_ids,
                )?;
                let (scope, request, after) = query::position(&sources[0].0, &q)?;
                let start = query::page_ordinal(after.as_deref())?;
                let result = investigation::query(
                    &q.investigation_id,
                    &sources,
                    options,
                    start,
                    usize::try_from(request.limit.get()).expect("validated limit"),
                )?;
                let mut result = query::finish_page(result, &scope, &request, start)?;
                result["nextOperation"] = super::guidance::query_next(
                    &q.investigation_id,
                    &q.analysis_id,
                    result.get("nextCursor").cloned(),
                );
                if result["nextCursor"].is_string() {
                    let mut next = q.wire_query.clone();
                    next["cursor"] = result["nextCursor"].clone();
                    next["limit"] = json!(request.limit.get());
                    result["nextOperation"]["arguments"] = next;
                }
                return Ok(result);
            }
            if q.investigation.is_some() || q.report.is_some() {
                return Err(Error::invalid(
                    "investigation/report options require their matching query view",
                ));
            }
            let (receipt, index) = self.retained_analysis(
                state,
                root,
                workspace,
                &q.investigation_id,
                &q.analysis_id,
            )?;
            let mut result = query::query(&receipt, &index, &q)?;
            result["nextOperation"] = super::guidance::query_next(
                &q.investigation_id,
                &q.analysis_id,
                result.get("nextCursor").cloned(),
            );
            if result["nextCursor"].is_string() {
                let mut next = q.wire_query.clone();
                next["cursor"] = result["nextCursor"].clone();
                next["limit"] = json!(q.limit.unwrap_or(50));
                result["nextOperation"]["arguments"] = next;
            }
            return Ok(result);
        }
        if name == "diagnostics_compare" {
            let p: Compare = parse(args)?;
            let (a, ai) = self.retained_analysis(
                state,
                root,
                workspace,
                &p.investigation_id,
                &p.analysis_id,
            )?;
            let (b, bi) = self.retained_analysis(
                state,
                root,
                workspace,
                &p.other_investigation_id,
                &p.other_analysis_id,
            )?;
            return Ok(query::compare(&a, &ai, &b, &bi));
        }
        let p: SelectAnalysis = parse(args)?;
        let dir = directory(root, workspace, &p.investigation_id)?;
        self.load(state, &dir)?;
        let mut r = self.recover_analysis(state, &dir, load_receipt(&dir, &p.analysis_id)?)?;
        if name == "diagnostics_analysis_cancel"
            && let Some(sender) = state.running.get(&dir.join(&r.id))
        {
            sender.send_replace(Some("analysis_cancelled".into()));
            r.phase = "stopping".into();
        }
        Ok(public_receipt(&r))
    }
    fn investigation_sources(
        &self,
        state: &mut State,
        root: &Path,
        workspace: &str,
        inv: &str,
        primary: &str,
        additional: &[String],
    ) -> Result<Vec<investigation::Source>> {
        investigation::ids(primary, additional)?
            .iter()
            .map(|id| self.retained_analysis(state, root, workspace, inv, id))
            .collect()
    }
    fn investigation_report(
        &self,
        state: &mut State,
        root: &Path,
        workspace: &str,
        inv: &str,
        primary: &str,
        options: &investigation::Report,
    ) -> Result<Value> {
        let sources = self.investigation_sources(
            state,
            root,
            workspace,
            inv,
            primary,
            &options.analysis_ids,
        )?;
        let dir = directory(root, workspace, inv)?;
        let record = self.load(state, &dir)?;
        investigation::report(
            inv,
            &record.question,
            &serde_json::to_value(&record.binding).unwrap(),
            &sources,
            options,
        )
    }
}
pub(super) fn owned_analysis_files(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot inspect retained analyses"))?
    {
        let entry =
            entry.map_err(|_| Error::new("EVIDENCE_IO", "Cannot inspect retained analyses"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name
            .strip_suffix(".json")
            .filter(|s| s.starts_with("analysis-") && !s.ends_with(".index"))
        {
            let receipt = load_receipt(dir, id)?;
            for index in [
                rebuild::staged_name(&receipt.id),
                format!("{}.index.json", receipt.id),
            ] {
                if dir.join(&index).exists() {
                    names.push(index);
                }
            }
            // Preserve the owning receipt until its derived index is removed.
            names.push(name);
        }
    }
    Ok(names)
}

fn validate_symbols(
    record: &super::model::Investigation,
    dir: &Path,
    metadata: &Metadata,
) -> Result<()> {
    for symbol in &metadata.symbols {
        if !record.collectors.iter().any(|c| {
            c.id == symbol.evidence_id
                && c.phase.settled()
                && (!metadata.symbolicate || c.phase == Phase::Complete)
                && c.sha256.as_ref() == Some(&symbol.sha256)
        }) {
            return Err(Error::new(
                "SYMBOL_IDENTITY_MISMATCH",
                "Symbol reference must name finalized evidence in this investigation with its exact digest",
            ));
        }
        let bytes = read_data(dir, &symbol.evidence_id)?;
        if digest(&bytes) != symbol.sha256 {
            return Err(Error::new("EVIDENCE_CHANGED", "Symbol evidence changed"));
        }
    }
    Ok(())
}
fn validate_memory_probes(
    record: &super::model::Investigation,
    dir: &Path,
    bytes: &[u8],
) -> Result<()> {
    for (collector_id, sha256) in memory::references(bytes)? {
        if !record.collectors.iter().any(|c| {
            c.id == collector_id
                && c.phase.settled()
                && c.sha256.as_deref() == Some(sha256.as_str())
        }) {
            return Err(Error::new(
                "MEMORY_PROBE_MISMATCH",
                "Memory probe references must name finalized evidence in this investigation with exact digests",
            ));
        }
        if digest(&read_data(dir, &collector_id)?) != sha256 {
            return Err(Error::new(
                "EVIDENCE_CHANGED",
                "Retained memory probe evidence changed",
            ));
        }
    }
    Ok(())
}
fn analysis_capacity(state: &State, dir: &Path) -> Result<()> {
    if state.running.len() >= 16
        || state
            .running
            .keys()
            .filter(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .starts_with("analysis-")
            })
            .count()
            >= 4
    {
        return Err(Error::new(
            "DIAGNOSTIC_CAPACITY",
            "At most four active analyses and sixteen active diagnostic jobs",
        ));
    }
    if owned_analysis_files(dir)?
        .iter()
        .filter(|p| p.ends_with(".json") && !p.ends_with(".index.json"))
        .count()
        >= 16
    {
        return Err(Error::new(
            "DIAGNOSTIC_CAPACITY",
            "At most sixteen retained analyses per investigation",
        ));
    }
    Ok(())
}
