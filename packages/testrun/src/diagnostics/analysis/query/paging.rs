//! Index-order keysets and shared budgets over retained immutable analyses.
use super::{Query, alignment, symbol_status};
use crate::{
    diagnostics::analysis::model::{Index, Receipt},
    error::{Error, Result},
};
use canonic_mcp_kit::page::{Builder, Limits, Position, Request, Scope};
use serde_json::{Value, json};

pub(in crate::diagnostics::analysis) fn position(
    receipt: &Receipt,
    q: &Query,
) -> Result<(Scope, Request, Option<Vec<u8>>)> {
    let fallback = json!({"investigationId":q.investigation_id,"analysisId":q.analysis_id,"view":q.view,"kind":q.kind,"clockDomain":q.clock_domain,"from":q.from,"to":q.to,"correlationId":q.correlation_id,"nativeUtid":q.native_utid,"nativeUpid":q.native_upid,"collection":q.collection});
    let query = if q.wire_query.is_null() {
        fallback
    } else {
        q.wire_query.clone()
    };
    let scope =
        crate::pagination::scope("diagnostics_query", &(receipt.index_sha256.clone(), query))?;
    let args =
        canonic_mcp_kit::page::arguments(q.cursor.as_deref(), q.limit.map(|limit| limit as u64));
    let (request, after) = crate::pagination::request(&args, &scope)?;
    Ok((scope, request, after))
}
pub(in crate::diagnostics::analysis) fn ordinal(after: Option<&[u8]>) -> Result<usize> {
    match after {
        None => Ok(0),
        Some(key) if key.len() == 8 => usize::try_from(u64::from_be_bytes(
            key.try_into()
                .map_err(|_| canonic_mcp_kit::page::Error::InvalidCursor)?,
        ))
        .ok()
        .and_then(|ordinal| ordinal.checked_add(1))
        .ok_or_else(|| canonic_mcp_kit::page::Error::InvalidCursor.into()),
        _ => Err(canonic_mcp_kit::page::Error::InvalidCursor.into()),
    }
}
pub(in crate::diagnostics::analysis) fn finish_page(
    mut result: Value,
    scope: &Scope,
    request: &Request,
    start: usize,
) -> Result<Value> {
    let more = result["nextOffset"].is_number();
    let field = ["rows", "traces", "measurementGroups"]
        .into_iter()
        .find(|field| result[*field].is_array());
    let mut next = None;
    if let Some(field) = field {
        let Value::Array(values) = result[field].take() else {
            unreachable!("array")
        };
        let mut builder = Builder::new(
            scope,
            request.limit,
            Limits::STANDARD,
            crate::protocol::MAX_REPLY,
            "diagnostics_read",
        )?;
        let mut rows = values.into_iter().enumerate();
        let mut ended = false;
        while !builder.is_full() {
            let Some((index, row)) = rows.next() else {
                ended = true;
                break;
            };
            if !builder.push(
                row,
                &Position::Keyset(((start + index) as u64).to_be_bytes().to_vec()),
            )? {
                break;
            }
        }
        let page = builder.finish(more || (!ended && rows.size_hint().1 != Some(0)));
        result[field] = json!(page.items);
        next = page.next_cursor;
    }
    result
        .as_object_mut()
        .expect("analysis page")
        .remove("nextOffset");
    result["nextCursor"] = json!(next);
    Ok(result)
}
fn key(ordinal: usize, row: &super::super::model::Row, timeline: bool) -> Vec<u8> {
    let mut key = Vec::new();
    if timeline {
        let bits = row.timestamp.expect("timeline row").to_bits();
        key.extend(
            if bits & (1 << 63) == 0 {
                bits ^ (1 << 63)
            } else {
                !bits
            }
            .to_be_bytes(),
        );
    }
    key.extend((ordinal as u64).to_be_bytes());
    key
}
pub(super) fn rows(
    receipt: &Receipt,
    index: &Index,
    q: &Query,
    scope: &Scope,
    request: &Request,
    after: Option<&[u8]>,
) -> Result<Value> {
    let timeline = q.view.as_deref() == Some("timeline");
    if after.is_some_and(|key| key.len() != if timeline { 16 } else { 8 }) {
        return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
    }
    validate_time_range(q)?;
    let selected_upid = q.native_upid.map(|id| id.to_string());
    let selected_utid = q.native_utid.map(|id| id.to_string());
    let threads = super::process_threads(index, selected_upid.as_deref());
    let selected = |row: &&super::super::model::Row| {
        let fields = &row.data["observation"]["fields"];
        (!timeline || row.timestamp.is_some())
            && q.kind.as_ref().is_none_or(|kind| kind == &row.kind)
            && q.clock_domain
                .as_ref()
                .is_none_or(|clock| row.clock_domain.as_ref() == Some(clock))
            && q.from
                .is_none_or(|from| row.timestamp.is_some_and(|time| time >= from))
            && q.to
                .is_none_or(|to| row.timestamp.is_some_and(|time| time <= to))
            && q.correlation_id
                .as_ref()
                .is_none_or(|id| row.correlations.contains(id))
            && selected_utid
                .as_ref()
                .is_none_or(|id| fields["utid"].as_str() == Some(id.as_str()))
            && selected_upid.as_ref().is_none_or(|id| {
                fields["upid"].as_str() == Some(id.as_str())
                    || fields["utid"]
                        .as_str()
                        .is_some_and(|id| threads.contains(id))
            })
    };
    let start = if timeline { 0 } else { ordinal(after)? };
    let rows = index
        .rows
        .iter()
        .enumerate()
        .skip(start)
        .filter(|(_, row)| selected(row));
    let mut ordered;
    let mut rows: Box<dyn Iterator<Item = (usize, &super::super::model::Row)> + '_> = if timeline {
        ordered = rows.collect::<Vec<_>>();
        let domains: std::collections::BTreeSet<_> = ordered
            .iter()
            .filter_map(|(_, row)| row.clock_domain.as_deref())
            .collect();
        if domains.len() > 1 {
            return Err(Error::invalid(
                "Select one clockDomain before ordering a timeline",
            ));
        }
        ordered.sort_by_key(|(ordinal, row)| key(*ordinal, row, true));
        Box::new(ordered.into_iter().filter(move |(ordinal, row)| {
            after.is_none_or(|after| key(*ordinal, row, true).as_slice() > after)
        }))
    } else {
        Box::new(rows)
    };
    let mut builder = Builder::new(
        scope,
        request.limit,
        Limits::STANDARD,
        crate::protocol::MAX_REPLY,
        "diagnostics_read",
    )?;
    let mut ended = false;
    while !builder.is_full() {
        let Some((ordinal, row)) = rows.next() else {
            ended = true;
            break;
        };
        let value = json!({"ordinal":ordinal,"record":row,"evidence":{"collectorId":receipt.collector_id,"rawSha256":receipt.raw_sha256,"source":row.source},"alignedTime":alignment(row,&receipt.metadata),"symbols":symbol_status(row,&receipt.metadata)});
        if !builder.push(value, &Position::Keyset(key(ordinal, row, timeline)))? {
            break;
        }
    }
    let page = builder.finish(!ended);
    let mut result = json!({"investigationId":q.investigation_id,"analysisId":q.analysis_id,"parser":index.parser,"indexSha256":receipt.index_sha256,"binding":receipt.binding,"bindingProvenance":"caller_declared","rows":page.items,"nextCursor":page.next_cursor,"clocks":index.clocks,"capturePhase":receipt.capture_phase,"captureReason":receipt.capture_reason,"sourceLoss":receipt.metadata.source_loss,"unsupported":index.unsupported,"private":true,"conclusion":"observations_only; zero matches do not establish absence or health"});
    result["observationComplete"] = json!(
        receipt.metadata.observation_complete
            && receipt.metadata.source_loss.is_empty()
            && receipt.capture_phase == "complete"
            && index.unsupported.is_empty()
    );
    result["aggregation"] = json!(if timeline {
        "none; timestamp order in one producer clock"
    } else {
        "none; source-order rows"
    });
    result["sampleDefinition"] =
        json!("recorded stack rows; inspect sampleKind; report/backtrace rows are not CPU samples");
    Ok(result)
}

fn validate_time_range(q: &Query) -> Result<()> {
    if q.from.is_some_and(|n| !n.is_finite())
        || q.to.is_some_and(|n| !n.is_finite())
        || q.from.zip(q.to).is_some_and(|(a, b)| a > b)
        || ((q.from.is_some() || q.to.is_some()) && q.clock_domain.is_none())
    {
        return Err(Error::invalid(
            "Use finite ordered times and a clockDomain for time ranges",
        ));
    }
    Ok(())
}
