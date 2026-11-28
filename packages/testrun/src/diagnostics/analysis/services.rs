//! Service/process scope over concrete artifact readers; no supervisor or remote exec.
use super::{decode, model::*};
use crate::error::{Error, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod minidump;
mod pprof;
mod profiles;
#[cfg(test)]
mod tests;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Selection {
    target_id: String,
    service_id: String,
    host_id: String,
    os: String,
    transport: String,
    pid: u32,
    process_generation: String,
    runtime: String,
    runtime_version: String,
    build_id: String,
    architecture: String,
    scope: Scope,
    owner: Owner,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Scope {
    Host,
    Container {
        #[serde(rename = "containerId")]
        container_id: String,
        #[serde(rename = "namespaceId")]
        namespace_id: String,
        #[serde(rename = "imageId")]
        image_id: String,
    },
    Kubernetes {
        cluster: String,
        namespace: String,
        #[serde(rename = "podUid")]
        pod_uid: String,
        #[serde(rename = "containerId")]
        container_id: String,
        #[serde(rename = "restartCount")]
        restart_count: u32,
        #[serde(rename = "imageId")]
        image_id: String,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Owner {
    Borrowed,
    External {
        #[serde(rename = "invocationRef")]
        invocation_ref: String,
    },
    Box {
        #[serde(rename = "projectId")]
        project_id: String,
        workspace: String,
        #[serde(rename = "operationId")]
        operation_id: String,
        #[serde(rename = "generationId")]
        generation_id: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Envelope {
    schema_version: u32,
    selection: Selection,
    format: String,
    encoding: String,
    payload: Value,
}
fn invalid() -> Error {
    Error::invalid(
        "Use exact service/process/host/runtime/build and workload-generation scope with a supported nonrecursive artifact format",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
fn formats(runtime: &str, format: &str) -> bool {
    if matches!(
        format,
        "host-process-v1"
            | "box-log-v1"
            | "native-trace-v1"
            | "apple-ips"
            | "apple-crash-text"
            | "windows-minidump"
            | "chrome-trace"
            | "runtime-checks-v1"
            | "sanitizer-report-text"
    ) {
        return true;
    }
    match runtime {
        "node" | "bun" | "deno" => format == "js-cpu-profile",
        "jvm" => format == "jfr-json",
        "dotnet" | "python" => format == "speedscope",
        "go" => format == "pprof",
        "ruby" => format == "stackprof-json",
        "php" => format == "xdebug-cachegrind",
        "native" => matches!(format, "speedscope" | "pprof"),
        _ => false,
    }
}
fn validate(s: &Selection) -> Result<()> {
    for t in [
        &s.target_id,
        &s.service_id,
        &s.host_id,
        &s.process_generation,
        &s.runtime_version,
        &s.build_id,
        &s.architecture,
    ] {
        label(t)?;
    }
    if s.pid == 0
        || !matches!(
            s.runtime.as_str(),
            "node"
                | "bun"
                | "deno"
                | "jvm"
                | "dotnet"
                | "go"
                | "python"
                | "ruby"
                | "php"
                | "native"
        )
        || !matches!(s.os.as_str(), "macos" | "windows" | "linux")
    {
        return Err(invalid());
    }
    match &s.scope {
        Scope::Host => {
            if !matches!(
                s.transport.as_str(),
                "local_export" | "remote_export" | "box_export"
            ) {
                return Err(invalid());
            }
        }
        Scope::Container {
            container_id,
            namespace_id,
            image_id,
        } => {
            for t in [container_id, namespace_id, image_id] {
                label(t)?;
            }
            if s.transport != "container_export" {
                return Err(invalid());
            }
        }
        Scope::Kubernetes {
            cluster,
            namespace,
            pod_uid,
            container_id,
            image_id,
            ..
        } => {
            for t in [cluster, namespace, pod_uid, container_id, image_id] {
                label(t)?;
            }
            if s.transport != "kubernetes_export" {
                return Err(invalid());
            }
        }
    }
    match &s.owner {
        Owner::Borrowed => {}
        Owner::External { invocation_ref } => label(invocation_ref)?,
        Owner::Box {
            project_id,
            workspace,
            operation_id,
            generation_id,
        } => {
            for t in [project_id, workspace, operation_id, generation_id] {
                label(t)?;
            }
            if s.transport != "box_export" {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let e: Envelope =
        serde_json::from_value(decode::bounded_json(bytes, MAX_RAW)?).map_err(|_| invalid())?;
    validate(&e.selection)?;
    if e.schema_version != 1 || !formats(&e.selection.runtime, &e.format) {
        return Err(invalid());
    }
    if e.format == "host-process-v1" && !matches!(e.selection.scope, Scope::Host) {
        return Err(Error::new(
            "SCOPE_MISMATCH",
            "Host process counters cannot be relabeled as container/pod metrics",
        ));
    }
    if e.format == "windows-minidump" && e.selection.os != "windows"
        || e.format.starts_with("apple-") && e.selection.os != "macos"
    {
        return Err(invalid());
    }
    let raw = match e.encoding.as_str() {
        "json" => serde_json::to_vec(&e.payload).map_err(|_| invalid())?,
        "utf8" => e.payload.as_str().ok_or_else(invalid)?.as_bytes().to_vec(),
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(e.payload.as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?,
        _ => return Err(invalid()),
    };
    let child = decode::decode(&e.format, &raw)?;
    let provenance = json!({"selection":e.selection,"format":e.format,"payloadSha256":crate::diagnostics::digest(&raw),"identityProvenance":"producer_declared; retained bytes do not authenticate remote identity or launch ownership","executionPerformed":false});
    index.clocks = child.clocks;
    index.unsupported = child.unsupported;
    decode::push(
        index,
        decode::row(
            "process",
            "Selected service evidence",
            "/selection".into(),
            provenance.clone(),
        ),
    )?;
    for mut row in child.rows {
        validate_artifact_row(&row, &e.selection)?;
        if let Some(data) = row.data.as_object_mut() {
            if data
                .insert("serviceEvidence".into(), provenance.clone())
                .is_some()
            {
                return Err(invalid());
            }
        } else {
            row.data = json!({"observation":row.data,"serviceEvidence":provenance});
        }
        row.source = format!("/payload/{}", row.source);
        decode::push(index, row)?;
    }
    index.unsupported.push("selected service export; no complete runtime/host/workload history, remote authentication or lifecycle authority inferred".into());
    Ok(())
}
pub(super) fn artifact(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    match format {
        "host-process-v1" => host(bytes, index),
        "box-log-v1" => box_log(bytes, index),
        "js-cpu-profile" => profiles::javascript(bytes, index),
        "jfr-json" => profiles::jfr(bytes, index),
        "stackprof-json" => profiles::ruby(bytes, index),
        "xdebug-cachegrind" => profiles::php(bytes, index),
        "pprof" => pprof::decode(bytes, index),
        "windows-minidump" => minidump::decode(bytes, index),
        _ => Err(invalid()),
    }
}
fn host(bytes: &[u8], index: &mut Index) -> Result<()> {
    let v = decode::bounded_json(bytes, 65536)?;
    if v["schemaVersion"] != 1
        || v["ownership"] != "borrowed"
        || v["coverage"] != "snapshot"
        || v["pid"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > i32::MAX as u64)
        || !matches!(v["hostOs"].as_str(), Some("macos" | "linux" | "windows"))
    {
        return Err(invalid());
    }
    for key in ["targetId", "serviceId", "hostId", "processGeneration"] {
        label(v[key].as_str().ok_or_else(invalid)?)?;
    }
    let metrics = v["metrics"]
        .as_array()
        .filter(|a| a.len() <= 32)
        .ok_or_else(invalid)?;
    let mut summary = v.clone();
    summary.as_object_mut().unwrap().remove("metrics");
    decode::push(
        index,
        decode::row(
            "process",
            "Borrowed host process snapshot",
            "/pid".into(),
            summary.clone(),
        ),
    )?;
    for (n, m) in metrics.iter().enumerate() {
        let mut row = decode::row(
            "metric",
            m["name"].as_str().ok_or_else(invalid)?,
            format!("/metrics/{n}"),
            summary.clone(),
        );
        row.value = Some(
            m["value"]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(invalid)?,
        );
        row.unit = Some(m["unit"].as_str().ok_or_else(invalid)?.into());
        row.definition = Some(m["definition"].as_str().ok_or_else(invalid)?.into());
        decode::push(index, row)?;
    }
    index.unsupported.push("host resource snapshot; no process tree census, rates, native heap ownership, profiler or app-health inference".into());
    Ok(())
}
fn box_log(bytes: &[u8], index: &mut Index) -> Result<()> {
    let v = decode::bounded_json(bytes, 512 * 1024)?;
    if !matches!(v["stream"].as_str(), Some("stdout" | "stderr")) || !v["tail_only"].is_boolean() {
        return Err(invalid());
    }
    if v["operation"].as_str().is_none()
        && (v["generation_id"].as_str().is_none() || v["service"].as_str().is_none())
    {
        return Err(invalid());
    }
    for key in ["workspace", "operation", "generation_id", "service"] {
        if let Some(s) = v.get(key) {
            label(s.as_str().ok_or_else(invalid)?)?;
        }
    }
    let text = v["text"]
        .as_str()
        .filter(|s| s.len() <= 65536)
        .ok_or_else(invalid)?;
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + 8192).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut data = v.clone();
        data["text"] = json!(&text[start..end]);
        data["byteOffsetWithinTail"] = json!(start);
        data["lifecycleOwner"] = json!("Box; no start/stop/recover authority transferred");
        decode::push(
            index,
            decode::row(
                "event",
                "Box retained log tail",
                format!("/text/bytes/{start}"),
                data,
            ),
        )?;
        start = end;
    }
    index.unsupported.push("Box retained tail; rotation/earlier output may be absent; offline observation does not establish process liveness".into());
    Ok(())
}

fn validate_artifact_row(row: &Row, selection: &Selection) -> Result<()> {
    if row
        .data
        .get("pid")
        .is_some_and(|v| !v.is_null() && v.as_u64() != Some(selection.pid as u64))
    {
        return Err(Error::new(
            "SCOPE_MISMATCH",
            "Artifact PID contradicts selected process",
        ));
    }
    for (key, expected) in [
        ("targetId", selection.target_id.as_str()),
        ("serviceId", selection.service_id.as_str()),
        ("hostId", selection.host_id.as_str()),
        ("hostOs", selection.os.as_str()),
        ("processGeneration", selection.process_generation.as_str()),
        ("buildId", selection.build_id.as_str()),
        ("architecture", selection.architecture.as_str()),
    ] {
        if row
            .data
            .get(key)
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(expected))
        {
            return Err(Error::new(
                "SCOPE_MISMATCH",
                "Artifact identity contradicts selected service scope",
            ));
        }
    }
    if let Owner::Box {
        workspace,
        operation_id,
        generation_id,
        ..
    } = &selection.owner
    {
        for (key, expected) in [
            ("workspace", workspace),
            ("operation", operation_id),
            ("generation_id", generation_id),
        ] {
            if row
                .data
                .get(key)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(expected))
            {
                return Err(Error::new(
                    "SCOPE_MISMATCH",
                    "Box log identity contradicts selected operation/generation",
                ));
            }
        }
    }

    Ok(())
}
