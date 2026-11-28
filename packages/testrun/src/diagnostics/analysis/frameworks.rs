//! Bounded runtime/native composition. Child formats keep their own models and clocks.
use super::{decode, model::*};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Bundle {
    schema_version: u32,
    framework: String,
    framework_version: String,
    target: Target,
    build: Build,
    components: Vec<Component>,
    #[serde(default)]
    links: Vec<Link>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Witness {
    component_id: String,
    ordinal: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Link {
    id: String,
    kind: String,
    from: Witness,
    to: Witness,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Target {
    id: String,
    app_id: String,
    class: String,
    os: String,
    transport: String,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Build {
    id: String,
    architecture: String,
    mode: String,
    aot: bool,
    trimmed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Component {
    id: String,
    role: String,
    process_generation: String,
    runtime_generation: Option<String>,
    build_id: String,
    architecture: String,
    format: String,
    payload: Value,
}
fn invalid() -> Error {
    Error::invalid(
        "Use a bounded framework-evidence-v1 bundle with exact build/architecture, component generations and supported runtime/native formats",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
fn allowed(framework: &str, role: &str, format: &str) -> bool {
    if role == "native" {
        return matches!(
            format,
            "android-diagnostics-v1"
                | "apple-diagnostics-v1"
                | "native-trace-v1"
                | "android-tombstone"
                | "android-anr"
                | "apple-ips"
                | "apple-crash-text"
        );
    }
    if role != "runtime" {
        return false;
    }
    match framework {
        "kotlin_multiplatform" | "compose_multiplatform" => {
            matches!(format, "runtime-samples-v1" | "chrome-trace" | "speedscope")
        }
        "dotnet_maui" => matches!(format, "speedscope" | "runtime-samples-v1"),
        "unity" | "unreal" | "custom_engine" => {
            matches!(format, "runtime-samples-v1" | "chrome-trace" | "speedscope")
        }
        "capacitor" | "cordova" | "ionic" | "electron" | "cef" | "tauri" | "webview2"
        | "wkwebview" | "hybrid" => matches!(
            format,
            "embedded-cdp-v1" | "browser-operation-v1" | "chrome-trace" | "runtime-samples-v1"
        ),
        _ => false,
    }
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let bundle: Bundle =
        serde_json::from_value(decode::bounded_json(bytes, MAX_RAW)?).map_err(|_| invalid())?;
    validate_bundle(&bundle)?;
    let mut ids = BTreeSet::new();
    let mut roles = BTreeSet::new();
    let mut witnesses = std::collections::BTreeMap::new();
    for (n, c) in bundle.components.into_iter().enumerate() {
        validate_component(
            &bundle.framework,
            &bundle.target,
            &bundle.build,
            &c,
            &mut ids,
        )?;
        // Only inline payloads: no files, URLs, external tools or recursive bundles.
        let raw = if matches!(
            c.format.as_str(),
            "embedded-cdp-v1" | "android-tombstone" | "android-anr" | "apple-crash-text"
        ) {
            c.payload.as_str().ok_or_else(invalid)?.as_bytes().to_vec()
        } else {
            serde_json::to_vec(&c.payload).map_err(|_| invalid())?
        };
        let child = decode::decode(&c.format, &raw)?;
        validate_component_rows(&c, &bundle.build, &bundle.target, &child)?;
        let prefix = format!("c{n}:");
        if index.clocks.len() + child.clocks.len() > 32 {
            return Err(invalid());
        }
        for mut clock in child.clocks {
            clock.id = format!("{prefix}{}", clock.id);
            if let Some(reference) = clock.reference_domain.as_mut() {
                *reference = format!("{prefix}{reference}");
            }
            index.clocks.push(clock);
        }
        roles.insert(c.role.clone());
        let provenance = json!({"componentId":c.id,"role":c.role,"framework":bundle.framework,"frameworkVersion":bundle.framework_version,"target":bundle.target,"build":bundle.build,"processGeneration":c.process_generation,"runtimeGeneration":c.runtime_generation,"format":c.format,"payloadSha256":crate::diagnostics::digest(&raw),"identityProvenance":"producer_declared","causeEstablished":false});
        let mut summary = decode::row(
            "process",
            "Framework evidence component",
            format!("/components/{n}"),
            provenance.clone(),
        );
        summary.correlations.push(format!("component:{}", c.id));
        decode::push(index, summary)?;
        witnesses.insert(c.id.clone(), (index.rows.len(), child.rows.len()));
        for mut row in child.rows {
            row.source = format!("/components/{n}/payload{}", row.source);
            if let Some(clock) = row.clock_domain.as_mut() {
                *clock = format!("{prefix}{clock}");
            }
            // Isolate identical runtime-local IDs across components/generations.
            row.correlations = row
                .correlations
                .into_iter()
                .map(|id| format!("{prefix}{id}"))
                .collect();
            if row.correlations.len() < 32 {
                row.correlations.push(format!("component:{}", c.id));
            }
            if let Some(object) = row.data.as_object_mut() {
                if object
                    .insert("frameworkEvidence".into(), provenance.clone())
                    .is_some()
                {
                    return Err(invalid());
                }
            } else {
                row.data = json!({"frameworkEvidence":provenance,"observation":row.data});
            }
            decode::push(index, row)?;
        }
        for gap in child.unsupported {
            index.unsupported.push(format!("{prefix}{gap}"));
        }
    }
    decode_links(bundle.links, &witnesses, index)?;
    if !roles.contains("native") || !roles.contains("runtime") {
        index
            .unsupported
            .push("runtime_native_combination_incomplete".into());
    }
    index.unsupported.push("framework_identity_and_transport_are_declared; no live capability/automation or cross-clock causal proof".into());
    Ok(())
}

fn validate_bundle(bundle: &Bundle) -> Result<()> {
    if bundle.schema_version != 1
        || bundle.components.is_empty()
        || bundle.components.len() > 8
        || bundle.links.len() > 64
    {
        return Err(invalid());
    }
    for s in [
        &bundle.framework,
        &bundle.framework_version,
        &bundle.target.id,
        &bundle.target.app_id,
        &bundle.build.id,
        &bundle.build.architecture,
    ] {
        label(s)?;
    }
    if !matches!(
        bundle.framework.as_str(),
        "kotlin_multiplatform"
            | "compose_multiplatform"
            | "dotnet_maui"
            | "unity"
            | "unreal"
            | "custom_engine"
            | "capacitor"
            | "cordova"
            | "ionic"
            | "electron"
            | "cef"
            | "tauri"
            | "webview2"
            | "wkwebview"
            | "hybrid"
    ) {
        return Err(invalid());
    }
    if !matches!(
        bundle.target.class.as_str(),
        "phone"
            | "tablet"
            | "desktop"
            | "tv"
            | "watch"
            | "automotive"
            | "spatial"
            | "extension"
            | "service"
            | "unknown"
    ) || !matches!(
        bundle.target.os.as_str(),
        "android"
            | "ios"
            | "macos"
            | "windows"
            | "linux"
            | "tvos"
            | "watchos"
            | "visionos"
            | "unknown"
    ) || !matches!(
        bundle.target.transport.as_str(),
        "adb" | "apple" | "cdp" | "import"
    ) || !matches!(
        bundle.build.mode.as_str(),
        "debug" | "profile" | "release" | "unknown"
    ) {
        return Err(invalid());
    }
    if bundle.target.transport == "adb" && bundle.target.os != "android"
        || bundle.target.transport == "apple"
            && !matches!(
                bundle.target.os.as_str(),
                "ios" | "macos" | "tvos" | "watchos" | "visionos"
            )
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_component(
    framework: &str,
    target: &Target,
    build: &Build,
    c: &Component,
    ids: &mut BTreeSet<String>,
) -> Result<()> {
    for s in [&c.id, &c.process_generation] {
        label(s)?;
    }
    if !ids.insert(c.id.clone())
        || !allowed(framework, &c.role, &c.format)
        || c.build_id != build.id
        || c.architecture != build.architecture
        || c.role == "runtime" && c.runtime_generation.is_none()
    {
        return Err(invalid());
    }
    if let Some(g) = &c.runtime_generation {
        label(g)?;
    }
    if c.role == "native"
        && (c.format.starts_with("android-") && target.os != "android"
            || c.format.starts_with("apple-")
                && !matches!(
                    target.os.as_str(),
                    "ios" | "macos" | "tvos" | "watchos" | "visionos"
                ))
    {
        return Err(invalid());
    }
    if c.format == "runtime-samples-v1" {
        let runtime = c.payload["runtime"].as_str().ok_or_else(invalid)?;
        let expected = match framework {
            "kotlin_multiplatform" | "compose_multiplatform" => "kotlin",
            "dotnet_maui" => "dotnet",
            "unity" => "unity",
            "unreal" => "unreal",
            "custom_engine" => "custom",
            "wkwebview" => "webkit",
            _ => "javascript",
        };
        if runtime != expected
            || c.payload["buildId"] != c.build_id
            || c.payload["architecture"] != c.architecture
        {
            return Err(invalid());
        }
    }
    if framework == "wkwebview" && c.format == "embedded-cdp-v1" {
        return Err(Error::new(
            "UNSUPPORTED_DIAGNOSTIC_FORMAT",
            "WKWebView uses WebKit inspection, not CDP; retain supported native/export evidence",
        ));
    }
    Ok(())
}

fn validate_component_rows(
    c: &Component,
    build: &Build,
    target: &Target,
    child: &Index,
) -> Result<()> {
    for row in &child.rows {
        for (field, expected) in [
            ("processGeneration", Some(c.process_generation.as_str())),
            ("runtimeGeneration", c.runtime_generation.as_deref()),
        ] {
            if row
                .data
                .get(field)
                .is_some_and(|v| !v.is_null() && v.as_str() != expected)
            {
                return Err(invalid());
            }
        }
        for (field, expected) in [
            ("buildId", build.id.as_str()),
            ("architecture", build.architecture.as_str()),
            ("appId", target.app_id.as_str()),
            ("targetId", target.id.as_str()),
        ] {
            if row
                .data
                .get(field)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(expected))
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Samples {
    schema_version: u32,
    runtime: String,
    build_id: String,
    symbols_build_id: Option<String>,
    architecture: String,
    records: Vec<Sample>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Sample {
    name: String,
    category: String,
    identity: String,
    state: Option<String>,
    frames: Vec<String>,
    value: Option<f64>,
    unit: Option<String>,
    definition: Option<String>,
    action_id: Option<String>,
}
pub(super) fn samples(bytes: &[u8], index: &mut Index) -> Result<()> {
    let packet: Samples =
        serde_json::from_value(decode::bounded_json(bytes, MAX_RAW)?).map_err(|_| invalid())?;
    if packet.schema_version != 1
        || !matches!(
            packet.runtime.as_str(),
            "kotlin" | "dotnet" | "unity" | "unreal" | "custom" | "javascript" | "webkit"
        )
        || packet.records.len() > 4096
    {
        return Err(invalid());
    }
    label(&packet.build_id)?;
    label(&packet.architecture)?;
    if packet
        .symbols_build_id
        .as_ref()
        .is_some_and(|id| id != &packet.build_id)
    {
        return Err(Error::new(
            "SYMBOL_MISMATCH",
            "Runtime sample symbols must match the exact build",
        ));
    }
    for (n, s) in packet.records.into_iter().enumerate() {
        for text in [&s.name, &s.identity] {
            label(text)?;
        }
        if !matches!(
            s.category.as_str(),
            "coroutine"
                | "recomposition"
                | "managed_exception"
                | "gc"
                | "scripting"
                | "native"
                | "asset"
                | "cpu"
                | "gpu"
                | "bridge"
                | "navigation"
        ) || s.frames.len() > 128
        {
            return Err(invalid());
        }
        if s.category == "coroutine"
            && !matches!(
                s.state.as_deref(),
                Some("created" | "running" | "suspended" | "completed" | "unknown")
            )
        {
            return Err(invalid());
        }
        for frame in &s.frames {
            crate::diagnostics::model::label(frame, 1024)?;
        }
        if let Some(state) = &s.state {
            label(state)?;
        }
        if s.value.is_some() != s.unit.is_some() || s.value.is_some() != s.definition.is_some() {
            return Err(invalid());
        }
        let kind = if s.category == "managed_exception" {
            "failure"
        } else if !s.frames.is_empty() {
            "stack"
        } else if s.value.is_some() {
            "metric"
        } else {
            "event"
        };
        let mut row = decode::row(
            kind,
            &s.name,
            format!("/records/{n}"),
            json!({"runtime":packet.runtime,"buildId":packet.build_id,"architecture":packet.architecture,"identity":s.identity,"category":s.category,"state":s.state,"frames":s.frames,"symbolication":if packet.symbols_build_id.is_some(){"producer_build_matched"}else{"unresolved"},"timing":"snapshot; no inferred duration or common clock","gpuMeaning":"producer-defined measurement; not CPU duration or displayed frames"}),
        );
        row.value = s.value;
        row.unit = s.unit;
        row.definition = s.definition;
        row.correlations
            .push(format!("runtime-object:{}", s.identity));
        if let Some(action) = s.action_id {
            label(&action)?;
            row.correlations.push(action);
        }
        decode::push(index, row)?;
    }
    index.unsupported.push("runtime_samples_are_explicit_app_or_exporter_observations; not a complete heap/coroutine/frame census or authenticated symbols".into());
    Ok(())
}

pub(super) fn embedded(bytes: &[u8], index: &mut Index) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    for (line, raw) in text
        .lines()
        .enumerate()
        .filter(|(_, s)| !s.trim().is_empty())
    {
        let value = decode::bounded_json(raw.as_bytes(), 1024 * 1024)?;
        if value["schemaVersion"] != 1 || value["adapter"] != "embedded-cdp/1" {
            return Err(invalid());
        }
        for key in ["targetId", "appId", "runtimeGeneration"] {
            label(value[key].as_str().ok_or_else(invalid)?)?;
        }
        let actions = value["actionIds"]
            .as_array()
            .filter(|a| a.len() <= 32)
            .ok_or_else(invalid)?;
        let mut correlations = vec![];
        for action in actions {
            let s = action.as_str().ok_or_else(invalid)?;
            label(s)?;
            correlations.push(s.to_string());
        }
        let kind = value["kind"].as_str().ok_or_else(invalid)?;
        if !matches!(
            kind,
            "discovery" | "selection" | "attachment" | "events" | "end"
        ) {
            return Err(invalid());
        }
        let records = if kind == "events" {
            value["data"]["events"]
                .as_array()
                .filter(|a| a.len() <= 256)
                .ok_or_else(invalid)?
                .clone()
        } else {
            vec![value["data"].clone()]
        };
        for (n, record) in records.into_iter().enumerate() {
            let method = record["method"].as_str().unwrap_or(kind);
            let kind = if matches!(
                method,
                "Runtime.exceptionThrown" | "Inspector.targetCrashed"
            ) {
                "failure"
            } else if method == "Page.frameNavigated" {
                "marker"
            } else {
                "event"
            };
            let mut row = decode::row(
                kind,
                method,
                format!("line:{}/records/{n}", line + 1),
                json!({"targetId":value["targetId"],"appId":value["appId"],"pageId":value["pageId"],"runtimeGeneration":value["runtimeGeneration"],"event":record,"nativeAssociation":"unverified","generationChanged":value["data"]["generationChanged"]}),
            );
            row.correlations = correlations.clone();
            decode::push(index, row)?;
        }
    }
    index.unsupported.push(
        "partial_embedded_page_window; no native/bridge completeness or cross-clock causal proof"
            .into(),
    );
    Ok(())
}

fn decode_links(
    links: Vec<Link>,
    witnesses: &std::collections::BTreeMap<String, (usize, usize)>,
    index: &mut Index,
) -> Result<()> {
    let mut link_ids = BTreeSet::new();
    for (n, link) in links.into_iter().enumerate() {
        label(&link.id)?;
        if !link_ids.insert(link.id.clone())
            || !matches!(
                link.kind.as_str(),
                "action" | "bridge" | "navigation" | "restart" | "phase"
            )
        {
            return Err(invalid());
        }
        let ordinal = |w: &Witness| -> Result<usize> {
            let (base, count) = witnesses.get(&w.component_id).ok_or_else(invalid)?;
            if w.ordinal >= *count {
                return Err(invalid());
            }
            Ok(base + w.ordinal)
        };
        let from = ordinal(&link.from)?;
        let to = ordinal(&link.to)?;
        let mut row = decode::row(
            "marker",
            "Declared framework evidence link",
            format!("/links/{n}"),
            json!({"id":link.id,"kind":link.kind,"from":link.from,"to":link.to,"witnessOrdinals":[from,to],"relationship":"producer_declared; temporal/causal alignment unverified"}),
        );
        row.correlations = vec![link.id];
        decode::push(index, row)?;
    }
    Ok(())
}
