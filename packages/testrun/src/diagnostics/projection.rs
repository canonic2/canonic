//! Scope-filtered numeric projections and explicit no-overwrite report publication.
use super::{
    analysis::model::{Index, Receipt},
    model::Binding,
};
use crate::{
    artifacts,
    error::{Error, Result},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scope {
    #[default]
    App,
    All,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Request {
    pub investigation_id: String,
    pub analysis_id: String,
    #[serde(default)]
    pub scope: Scope,
    pub kind: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u64>,
    pub path: Option<PathBuf>,
    pub format: Option<String>,
    pub report: Option<super::analysis::investigation::Report>,
}
fn safe_unit(unit: Option<&str>) -> Option<&str> {
    unit.filter(|unit| {
        matches!(
            *unit,
            "bytes"
                | "KiB"
                | "MiB"
                | "GiB"
                | "seconds"
                | "milliseconds"
                | "microseconds"
                | "nanoseconds"
                | "count"
                | "percent"
                | "none"
        )
    })
}
/// A row's numeric fields and opaque source reference; free text is omitted.
fn projected_row(
    receipt: &Receipt,
    index: &Index,
    ordinal: usize,
    row: &super::analysis::model::Row,
) -> Value {
    let clock = row
        .clock_domain
        .as_ref()
        .and_then(|domain| index.clocks.iter().position(|clock| &clock.id == domain));
    let timestamp_unit = clock.and_then(|i| safe_unit(Some(index.clocks[i].unit.as_str())));
    json!({"ordinal":ordinal,"kind":row.kind,"timestamp":row.timestamp,"clock":clock,"timestampUnit":timestamp_unit,"value":row.value,"unit":safe_unit(row.unit.as_deref()),
        "source":{"collectorId":receipt.collector_id,"rawSha256":receipt.raw_sha256,"ordinal":ordinal},"definition":"omitted_by_projection"})
}
/// One page of projected rows in index order, keyed by row ordinal. The cursor
/// is bound to the index digest and the scope and kind filters.
pub(super) fn project(receipt: &Receipt, index: &Index, request: &Request) -> Result<Value> {
    use canonic_mcp_kit::page::{Builder, Limits, Position};
    let scope_name = match request.scope {
        Scope::App => "app",
        Scope::All => "all",
    };
    let page_scope = crate::pagination::scope(
        "diagnostics_project",
        &(
            &receipt.index_sha256,
            json!({"investigationId":request.investigation_id,"analysisId":request.analysis_id,"scope":scope_name,"kind":request.kind}),
        ),
    )?;
    let (page, after) = crate::pagination::request(
        &canonic_mcp_kit::page::arguments(request.cursor.as_deref(), request.limit),
        &page_scope,
    )?;
    let after = after
        .map(|key| {
            <[u8; 8]>::try_from(key.as_slice())
                .map(u64::from_be_bytes)
                .map_err(|_| canonic_mcp_kit::page::Error::InvalidCursor)
        })
        .transpose()?;
    let binding: Binding = serde_json::from_value(receipt.binding.clone())
        .map_err(|_| Error::new("STATE_FORMAT", "Invalid retained application scope"))?;
    binding.validate()?;
    if matches!(request.scope, Scope::App) && binding.app_id.is_none() {
        return Err(Error::new(
            "DIAGNOSTIC_SCOPE_REQUIRED",
            "Bind an appId, or explicitly select scope all for an artifact that is not app-isolated",
        ));
    }
    let mut eligible = Vec::new();
    let mut excluded = 0usize;
    for (ordinal, row) in index.rows.iter().enumerate() {
        row.validate()?;
        if matches!(request.scope, Scope::App) {
            let app = binding.app_id.as_deref();
            let direct = row.data["appId"].as_str() == app;
            let system =
                row.data["systemContext"] == true && row.data["relatedAppId"].as_str() == app;
            let generation = if system {
                row.data["relatedProcessGeneration"].as_str()
            } else {
                row.data["processGeneration"].as_str()
            };
            if !(direct || system)
                || binding
                    .process_generation
                    .as_deref()
                    .is_some_and(|expected| generation != Some(expected))
            {
                excluded += 1;
                continue;
            }
        }
        if request.kind.as_ref().is_none_or(|kind| kind == &row.kind) {
            eligible.push((ordinal, row));
        }
    }
    let mut builder = Builder::new(
        &page_scope,
        page.limit,
        Limits::STANDARD,
        crate::protocol::MAX_REPLY,
        "diagnostics_project",
    )?;
    let mut remaining = eligible
        .iter()
        .filter(|(ordinal, _)| after.is_none_or(|after| *ordinal as u64 > after))
        .peekable();
    while !builder.is_full() {
        let Some((ordinal, row)) = remaining.next() else {
            break;
        };
        if !builder.push(
            projected_row(receipt, index, *ordinal, row),
            &Position::Keyset((*ordinal as u64).to_be_bytes().to_vec()),
        )? {
            break;
        }
    }
    let rows = builder.finish(remaining.peek().is_some());
    let capture_phase = match receipt.capture_phase.as_str() {
        "complete" => "complete",
        "partial" => "partial",
        "failed" => "failed",
        _ => "unknown",
    };
    Ok(
        json!({"schemaVersion":1,"projection":"minimal-v1","investigationId":request.investigation_id,"analysisId":request.analysis_id,"rawSha256":receipt.raw_sha256,"indexSha256":receipt.index_sha256,
        "scope":{"kind":match request.scope{Scope::App=>"app",Scope::All=>"all"},"identityProvenance":"caller_and_producer_declared","verified":false,"excludedRows":excluded},
        "rows":rows.items,"matched":eligible.len(),"nextCursor":rows.next_cursor,"capturePhase":capture_phase,"sourceLossCount":receipt.metadata.source_loss.len(),"unsupportedSectionCount":index.unsupported.len(),
        "omitted":["names","free_text","data","urls","headers","bodies","paths","source_pointers","correlation_ids","clock_names","symbol_labels","measurement_definitions"],
        "privacy":"redacted_projection; numeric and timing data can remain sensitive","observationComplete":false,"conclusion":"projection_only; omitted or unobserved data is not a clean result"}),
    )
}
pub(super) fn export(root: &Path, destination: &Path, report: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(report)
        .map_err(|_| Error::new("EXPORT_FAILED", "Cannot encode redacted report"))?;
    publish(root, destination, &bytes, "minimal-v1", 256 * 1024)
}
pub(super) fn export_native(root: &Path, destination: &Path, bytes: &[u8]) -> Result<Value> {
    publish(
        root,
        destination,
        bytes,
        "native_artifact",
        super::native_trace::MAX_ARTIFACT,
    )
}
pub(super) fn export_report(
    root: &Path,
    destination: &Path,
    report: &Value,
    format: &str,
) -> Result<Value> {
    let bytes = if format == "investigation-markdown" {
        super::analysis::investigation::markdown(report)?
    } else {
        serde_json::to_vec_pretty(report)
            .map_err(|_| Error::new("EXPORT_FAILED", "Cannot encode investigation report"))?
    };
    let mut result = publish(root, destination, &bytes, format, 524288)?;
    result["privacy"] = json!(if report["privacy"] == "private" {
        "private_report; explicit text can contain secrets"
    } else {
        "minimal_report; identities and free text omitted; counts/digests remain sensitive"
    });
    Ok(result)
}
fn publish(
    root: &Path,
    destination: &Path,
    bytes: &[u8],
    format: &str,
    max: usize,
) -> Result<Value> {
    if !destination.is_absolute() || destination.file_name().is_none() {
        return Err(Error::invalid(
            "Export path must name an absolute file in an existing directory",
        ));
    }
    let parent =
        destination.parent().unwrap().canonicalize().map_err(|_| {
            Error::new("EXPORT_PATH", "Export parent must be an existing directory")
        })?;
    if !parent.is_dir()
        || root
            .canonicalize()
            .is_ok_and(|root| parent.starts_with(root))
    {
        return Err(Error::new(
            "EXPORT_PATH",
            "Select a report destination outside Testrun's private runtime directory",
        ));
    }
    let path = parent.join(destination.file_name().unwrap());
    if bytes.len() > max {
        return Err(Error::new(
            "EXPORT_BOUNDS",
            "Export exceeds its format's byte limit; select smaller evidence or a smaller report page",
        ));
    }
    let hash = super::digest(bytes);
    let mut already = false;
    match artifacts::write(&path, bytes) {
        Ok(()) => {}
        Err(error) if error.code == "EVIDENCE_IO" => {
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                return Err(Error::new(
                    "EXPORT_CONFLICT",
                    "Destination is unavailable or not a regular report file",
                ));
            }
            let file = options.open(&path).map_err(|_| {
                Error::new(
                    "EXPORT_CONFLICT",
                    "Cannot inspect existing export destination",
                )
            })?;
            if !file
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= max as u64)
            {
                return Err(Error::new(
                    "EXPORT_CONFLICT",
                    "Existing destination is not this report",
                ));
            }
            let mut existing = Vec::new();
            file.take(max as u64 + 1)
                .read_to_end(&mut existing)
                .map_err(|_| {
                    Error::new(
                        "EXPORT_CONFLICT",
                        "Cannot inspect existing export destination",
                    )
                })?;
            if existing.as_slice() != bytes {
                return Err(Error::new(
                    "EXPORT_CONFLICT",
                    "Destination contains different bytes; select a new path",
                ));
            }
            already = true;
        }
        Err(error) => return Err(error),
    }
    Ok(
        json!({"exported":true,"alreadyPresent":already,"sha256":hash,"bytes":bytes.len(),"projection":format,"privacy":if format=="native_artifact"{"unredacted_native_artifact"}else{"redacted_projection"},"retention":"caller_owned; investigation removal does not delete this report","remoteTransmission":false}),
    )
}
