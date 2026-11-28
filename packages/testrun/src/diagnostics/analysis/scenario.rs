//! Source-specific lifecycle/media/device observations and setting restoration evidence.
#[cfg(test)]
mod tests;
use super::model::{Clock, Index, Metadata, Row};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
fn invalid() -> Error {
    Error::new(
        "INVALID_SCENARIO_CONTEXT",
        "Invalid, ambiguous or unsupported scenario context evidence",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
fn number(n: f64) -> Result<()> {
    if !n.is_finite() || !(0.0..=9007199254740991.0).contains(&n) {
        return Err(invalid());
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Capture {
    schema_version: u32,
    target_id: String,
    app_id: Option<String>,
    process_generation: Option<String>,
    hardware: String,
    coverage: String,
    clocks: Vec<Clock>,
    records: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Record {
    id: String,
    clock: String,
    at: f64,
    scope: String,
    action_id: Option<String>,
    observation: Observation,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Observation {
    #[serde(rename_all = "camelCase")]
    Lifecycle {
        event: String,
        resource_id: Option<String>,
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    Media {
        session_id: String,
        item_id: String,
        state: String,
        position_ms: Option<f64>,
        position_source: String,
        buffered_ms: Option<f64>,
        buffer_state: String,
        route: Option<String>,
        audio_focus: Option<String>,
        interruption: Option<String>,
        decoder_error: Option<String>,
        audio_sample: Option<AudioSample>,
    },
    #[serde(rename_all = "camelCase")]
    Peripheral {
        kind: String,
        event: String,
        resource_id: String,
        effects: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Energy {
        kind: String,
        value: f64,
        unit: String,
        attribution: String,
        attribution_evidence: Option<String>,
        definition: String,
    },
    #[serde(rename_all = "camelCase")]
    Setting {
        name: String,
        before: Value,
        applied: Value,
        current: Value,
        phase: String,
        owner: String,
        receipt_ref: Option<String>,
        restore_error: Option<String>,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AudioSample {
    rms_dbfs: f64,
    peak_dbfs: f64,
    frames: u64,
    duration_ms: f64,
    health: String,
    route_changed: bool,
}
#[expect(
    clippy::too_many_lines,
    reason = "bounded typed dispatch keeps measurement and side-effect semantics with source witnesses"
)]
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let capture: Capture =
        serde_json::from_value(super::decode::bounded_json(bytes, super::model::MAX_RAW)?)
            .map_err(|_| invalid())?;
    if capture.schema_version != 1
        || capture.records.len() > 4096
        || capture.clocks.is_empty()
        || !matches!(
            capture.hardware.as_str(),
            "physical" | "simulator" | "emulator" | "unknown"
        )
    {
        return Err(invalid());
    }
    label(&capture.target_id)?;
    label(&capture.coverage)?;
    for s in capture
        .app_id
        .iter()
        .chain(capture.process_generation.iter())
    {
        label(s)?;
    }
    Metadata {
        clocks: capture.clocks.clone(),
        ..Metadata::default()
    }
    .validate()?;
    if capture
        .clocks
        .iter()
        .any(|c| !c.anchors.is_empty() || c.reference_domain.is_some() || c.max_drift_ppm.is_some())
    {
        return Err(invalid());
    }
    index.clocks = capture.clocks;
    let mut ids = BTreeSet::new();
    let mut times = BTreeMap::new();
    type TrackIdentity = (String, String, String, String, String);
    let mut positions: BTreeMap<TrackIdentity, (f64, f64)> = BTreeMap::new();
    for (i, record) in capture.records.into_iter().enumerate() {
        label(&record.id)?;
        number(record.at)?;
        if !ids.insert(record.id.clone())
            || !index.clocks.iter().any(|c| c.id == record.clock)
            || !matches!(record.scope.as_str(), "app" | "device" | "external")
        {
            return Err(invalid());
        }
        if record.scope == "app"
            && (capture.app_id.is_none() || capture.process_generation.is_none())
        {
            return Err(invalid());
        }
        if times
            .insert(record.clock.clone(), record.at)
            .is_some_and(|previous| previous > record.at)
        {
            return Err(invalid());
        }
        if let Some(action) = &record.action_id {
            label(action)?;
        }
        let mut data = json!({"targetId":capture.target_id,"appId":if record.scope=="app"{capture.app_id.clone()}else{None},"declaredAppId":capture.app_id,"processGeneration":if record.scope=="app"{capture.process_generation.clone()}else{None},"hardware":capture.hardware,"scope":record.scope,"coverage":capture.coverage,"causeEstablished":false,"identityProvenance":"producer-declared; imports do not authenticate app/device/process or setting ownership"});
        let category = match &record.observation {
            Observation::Lifecycle {
                event,
                resource_id,
                reason,
            } => {
                if !matches!(
                    event.as_str(),
                    "process_started"
                        | "resumed"
                        | "paused"
                        | "stopped"
                        | "suspended"
                        | "process_exited"
                        | "job_scheduled"
                        | "job_started"
                        | "job_finished"
                        | "task_expired"
                        | "foreground_service_started"
                        | "foreground_service_stopped"
                        | "notification_posted"
                        | "notification_removed"
                        | "wakelock_acquired"
                        | "wakelock_released"
                        | "device_locked"
                        | "device_unlocked"
                ) {
                    return Err(invalid());
                }
                label(reason)?;
                if let Some(id) = resource_id {
                    label(id)?;
                }
                "lifecycle"
            }
            Observation::Media {
                session_id,
                item_id,
                state,
                position_ms,
                position_source,
                buffered_ms,
                buffer_state,
                route,
                audio_focus,
                interruption,
                decoder_error,
                audio_sample,
            } => {
                label(session_id)?;
                label(item_id)?;
                if !matches!(
                    state.as_str(),
                    "playing"
                        | "paused"
                        | "buffering"
                        | "connecting"
                        | "stopped"
                        | "error"
                        | "unknown"
                ) || !matches!(
                    position_source.as_str(),
                    "player_reported" | "service_extrapolated" | "decoded" | "unknown"
                ) || !matches!(
                    buffer_state.as_str(),
                    "empty" | "filling" | "ready" | "stalled" | "unknown"
                ) {
                    return Err(invalid());
                }
                for value in position_ms.iter().chain(buffered_ms.iter()) {
                    number(*value)?;
                }
                for s in route
                    .iter()
                    .chain(audio_focus.iter())
                    .chain(interruption.iter())
                    .chain(decoder_error.iter())
                {
                    label(s)?;
                }
                if let Some(sample) = audio_sample {
                    if !sample.rms_dbfs.is_finite()
                        || !sample.peak_dbfs.is_finite()
                        || !(-2000.0..=0.0).contains(&sample.rms_dbfs)
                        || !(-2000.0..=0.0).contains(&sample.peak_dbfs)
                        || sample.rms_dbfs > sample.peak_dbfs
                        || sample.frames > 9007199254740991
                        || !matches!(sample.health.as_str(), "complete" | "partial" | "unknown")
                    {
                        return Err(invalid());
                    }
                    number(sample.duration_ms)?;
                    data["audioMeaning"] = json!(
                        "producer sample levels and transport health; neither player position nor nonzero samples prove speaker/headphone audibility or app attribution; route-change effect remains explicit"
                    );
                }
                if let Some(position) = position_ms {
                    let key = (
                        record.scope.clone(),
                        record.clock.clone(),
                        session_id.clone(),
                        item_id.clone(),
                        position_source.clone(),
                    );
                    if let Some((at, previous)) = positions.insert(key, (record.at, *position)) {
                        if at < record.at && *position >= previous && position_source != "unknown" {
                            data["positionObservation"] = json!({"deltaMs":position-previous,"meaning":if position_source=="service_extrapolated"{"service-projected progress; not decoded or audible output"}else{"reported same-session/item/source difference; not audibility, continuous playback or liveness proof"}});
                        } else {
                            data["positionObservation"] = json!({"unavailable":"same-time, decreasing or unknown position source; seek/reset/loss not inferred"});
                        }
                    }
                }
                "media"
            }
            Observation::Peripheral {
                kind,
                event,
                resource_id,
                effects,
            } => {
                label(event)?;
                label(resource_id)?;
                if !matches!(
                    kind.as_str(),
                    "video" | "camera" | "bluetooth" | "location" | "sensor"
                ) || effects.len() > 8
                    || effects.iter().collect::<BTreeSet<_>>().len() != effects.len()
                    || effects.iter().any(|e| {
                        !matches!(
                            e.as_str(),
                            "camera_active"
                                | "microphone_active"
                                | "radio_connection"
                                | "location_request"
                                | "sensor_sampling"
                                | "external_device"
                                | "unknown"
                        )
                    })
                {
                    return Err(invalid());
                }
                data["probeMeaning"] = json!(
                    "recorded platform/app marker only; no active camera/radio/location/sensor probe, external-device input or hardware acceptance performed"
                );
                "peripherals"
            }
            Observation::Energy {
                kind,
                value,
                unit,
                attribution,
                attribution_evidence,
                definition,
            } => {
                label(definition)?;
                if !value.is_finite()
                    || value.abs() > 9007199254740991.0
                    || !matches!(
                        (kind.as_str(), unit.as_str()),
                        ("battery_level", "percent")
                            | ("temperature", "celsius")
                            | ("thermal_status", "code")
                            | ("power", "watt" | "milliwatt")
                            | ("energy", "joule" | "millijoule")
                            | ("charge", "milliamp_hours")
                    )
                    || kind != "temperature" && *value < 0.0
                    || kind == "battery_level" && *value > 100.0
                    || kind == "thermal_status" && (*value > 6.0 || value.fract() != 0.0)
                    || !matches!(attribution.as_str(), "device" | "process" | "unknown")
                {
                    return Err(invalid());
                }
                if let Some(evidence) = attribution_evidence {
                    label(evidence)?;
                }
                if attribution == "device" && record.scope != "device"
                    || attribution == "process"
                        && (record.scope != "app"
                            || !matches!(kind.as_str(), "power" | "energy")
                            || attribution_evidence.is_none())
                {
                    return Err(invalid());
                }
                data["energyMeaning"] = json!(
                    "source-defined measurement and attribution evidence; battery percentage/charge/temperature/thermal status are not precise app energy; no power integration or causal attribution inferred"
                );
                "energy"
            }
            Observation::Setting {
                name,
                before,
                applied,
                current,
                phase,
                owner,
                receipt_ref,
                restore_error,
            } => {
                if !matches!(
                    name.as_str(),
                    "locale"
                        | "timezone"
                        | "appearance"
                        | "text_size"
                        | "rotation"
                        | "permission"
                        | "connectivity"
                ) || !matches!(
                    phase.as_str(),
                    "planned" | "changed" | "restored" | "restore_failed" | "outcome_unknown"
                ) || !matches!(owner.as_str(), "owned_scenario" | "external")
                {
                    return Err(invalid());
                }
                for v in [before, applied, current] {
                    scalar(v)?;
                }
                for s in receipt_ref.iter().chain(restore_error.iter()) {
                    label(s)?;
                }
                if owner == "owned_scenario" && phase != "planned" && receipt_ref.is_none()
                    || phase == "restore_failed" && restore_error.is_none()
                {
                    return Err(invalid());
                }
                data["restoration"] = json!({"status":restore(before,applied,current,phase,owner),"performed":false,"authority":"producer-declared ledger; receipt label is not verified setter authority; use the existing setting owner and fresh readback"});
                "settings"
            }
        };
        data["scenario"] =
            json!({"category":category,"id":record.id,"observation":record.observation});
        let mut row = super::decode::row(
            if category == "energy" {
                "metric"
            } else {
                "event"
            },
            category,
            format!("/records/{i}"),
            data,
        );
        row.timestamp = Some(record.at);
        row.clock_domain = Some(record.clock);
        if let Observation::Energy {
            value,
            unit,
            definition,
            ..
        } = &record.observation
        {
            row.value = Some(*value);
            row.unit = Some(unit.clone());
            row.definition = Some(definition.clone());
        }
        if let Some(id) = record.action_id {
            row.correlations.push(id);
        }
        super::decode::push(index, row)?;
    }
    index.unsupported.push("Partial producer context; clocks remain separate, no causal/liveness/audibility or app-energy proof inferred; restoration is a report, never automatic settings changes or active probes".into());
    Ok(())
}
fn scalar(v: &Value) -> Result<()> {
    match v {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::String(s) if s.len() <= 1024 => Ok(()),
        Value::Number(n)
            if n.as_f64()
                .is_some_and(|n| n.is_finite() && n.abs() <= 9007199254740991.0) =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}
fn restore(
    before: &Value,
    applied: &Value,
    current: &Value,
    phase: &str,
    owner: &str,
) -> &'static str {
    if owner != "owned_scenario" {
        "external_setting_preserved"
    } else if phase == "outcome_unknown" {
        "inspect_owner_outcome_before_restore"
    } else if phase == "restore_failed" {
        "reported_restore_failure"
    } else if before.is_null() || applied.is_null() || current.is_null() {
        "setting_readback_unavailable"
    } else if phase == "restored" {
        if current == before {
            "reported_restore_matches_readback"
        } else {
            "reported_restore_readback_mismatch"
        }
    } else if phase == "planned" || before == applied {
        "no_owned_change_to_restore"
    } else if current == before {
        "current_already_matches_before"
    } else if current != applied {
        "concurrent_change_preserved"
    } else {
        "restore_candidate_requires_owner_authority_and_fresh_readback"
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Query {
    operation: String,
    clock_domain: Option<String>,
    action_id: Option<String>,
}
fn category(row: &Row) -> Option<&str> {
    row.data["scenario"]["category"].as_str().or_else(|| {
        if row.data["stateSource"] != true {
            None
        } else {
            match row.data["group"].as_str() {
                Some("battery" | "thermal") => Some("energy"),
                Some(
                    "power" | "idle" | "standby" | "background" | "jobs" | "services" | "alarms"
                    | "notifications",
                ) => Some("lifecycle"),
                Some("audio" | "media_sessions") => Some("media"),
                Some(
                    "locale"
                    | "timezone"
                    | "font_scale"
                    | "user_rotation"
                    | "accelerometer_rotation",
                ) => Some("settings"),
                _ => None,
            }
        }
    })
}
pub(super) fn query(index: &Index, q: &Query, offset: usize, limit: usize) -> Result<Value> {
    if !matches!(
        q.operation.as_str(),
        "summary"
            | "timeline"
            | "lifecycle"
            | "media"
            | "peripherals"
            | "energy"
            | "settings"
            | "restore"
    ) {
        return Err(Error::invalid("Select a scenario operation"));
    }
    for s in q.clock_domain.iter().chain(q.action_id.iter()) {
        label(s)?;
    }
    let mut rows: Vec<_> = index
        .rows
        .iter()
        .filter(|r| {
            category(r).is_some_and(|c| {
                matches!(q.operation.as_str(), "summary" | "timeline")
                    || c == q.operation
                    || q.operation == "restore"
                        && c == "settings"
                        && r.data["restoration"].is_object()
            }) && q
                .clock_domain
                .as_ref()
                .is_none_or(|c| r.clock_domain.as_ref() == Some(c))
                && q.action_id
                    .as_ref()
                    .is_none_or(|a| r.correlations.contains(a))
        })
        .collect();
    if q.operation == "timeline" {
        if rows
            .iter()
            .filter_map(|r| r.clock_domain.as_ref())
            .collect::<BTreeSet<_>>()
            .len()
            > 1
        {
            return Err(Error::invalid(
                "Choose one scenario clockDomain; no cross-clock ordering inferred",
            ));
        }
        rows.sort_by(|a, b| {
            a.timestamp
                .unwrap_or(f64::INFINITY)
                .total_cmp(&b.timestamp.unwrap_or(f64::INFINITY))
        });
    }
    let matched = rows.len();
    let next = offset.saturating_add(limit).min(matched);
    let mut counts = BTreeMap::new();
    for r in &rows {
        *counts.entry(category(r).unwrap()).or_insert(0usize) += 1;
    }
    Ok(
        json!({"rows":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"matched":matched,"nextOffset":if next<matched{Some(next)}else{None},"categories":counts,"unsupported":index.unsupported,"interpretation":"partial source context; device measurements are not app measurements, player progress is not audibility, setting labels are not setter authority; no active probe or restore performed"}),
    )
}
