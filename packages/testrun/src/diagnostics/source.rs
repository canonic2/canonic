//! Read-only adapters: pinned import descriptors and borrowed retained-operation logs.
use super::model::{Source, label};
use crate::{
    builds,
    error::{Error, Result},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata, OpenOptions},
    io::Read,
    path::Path,
};

pub(crate) enum Reader {
    Observability(Box<super::observability::Capture>),
    Host(Box<super::host::Capture>),
    BoxLogs(super::host::BoxOptions),
    Embedded(Box<super::embedded::Capture>),
    Import {
        file: File,
        metadata: Metadata,
    },
    Build {
        operation: String,
        cursor: Option<String>,
    },
    Android(Box<super::android::Capture>),
    Apple(Box<super::apple::Capture>),
    Buffered {
        bytes: Vec<u8>,
        reason: &'static str,
    },
    ReactNative(Box<super::react_native::Capture>),
    Dart(Box<super::dart::Capture>),
    NativeTrace(Box<super::native_trace::Capture>),
}
pub(crate) struct Chunk {
    pub bytes: Vec<u8>,
    pub offset: u64,
    pub complete: bool,
    pub summary: Option<serde_json::Value>,
    pub reason: Option<String>,
}
impl Reader {
    pub fn native_trace(&self) -> bool {
        matches!(self, Self::NativeTrace(_))
    }

    pub fn perfetto_start(&mut self) -> Option<tokio::sync::oneshot::Receiver<std::time::Instant>> {
        match self {
            Self::NativeTrace(capture) => capture.perfetto_start(),
            _ => None,
        }
    }

    pub fn native(&self) -> bool {
        matches!(
            self,
            Self::Android(_)
                | Self::Host(_)
                | Self::Observability(_)
                | Self::BoxLogs(_)
                | Self::Embedded(_)
                | Self::Apple(_)
                | Self::ReactNative(_)
                | Self::Dart(_)
                | Self::NativeTrace(_)
        )
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The exhaustive source admission table keeps each source constructor next to its retained identity contract"
    )]
    pub fn open(
        source: &Source,
        builds: &builds::Owners,
        root: &Path,
        workspace: &str,
        browsers: Option<(&crate::browser::runtime::Owners, &str)>,
        binding: &super::model::Binding,
    ) -> Result<(Self, String)> {
        match source {
            Source::ObservabilityHttp { options } => Ok((
                Self::Observability(Box::new(super::observability::Capture::new(
                    options.clone(),
                )?)),
                format!(
                    "observability:{}",
                    super::digest(serde_json::to_string(options).unwrap().as_bytes())
                ),
            )),
            Source::HostProcess { options } => Ok((
                Self::Host(Box::new(super::host::Capture::new(options.clone())?)),
                format!(
                    "host:{}",
                    super::digest(serde_json::to_string(options).unwrap().as_bytes())
                ),
            )),
            Source::BoxLogs { options } => {
                options.validate()?;
                Ok((
                    Self::BoxLogs(options.clone()),
                    format!(
                        "box-logs:{}",
                        super::digest(serde_json::to_string(options).unwrap().as_bytes())
                    ),
                ))
            }
            Source::EmbeddedCdp { options } => Ok((
                Self::Embedded(Box::new(super::embedded::Capture::new(options.clone())?)),
                format!(
                    "embedded:{}",
                    super::digest(serde_json::to_string(options).unwrap().as_bytes())
                ),
            )),
            Source::Marker {
                name,
                phase,
                action_id,
            } => {
                let bytes = super::reproduction::marker(name, phase, action_id, binding)?;
                let identity = format!("marker:{}", super::digest(&bytes));
                Ok((
                    Self::Buffered {
                        bytes,
                        reason: "host_action_marker",
                    },
                    identity,
                ))
            }
            Source::NativeTrace { options } => Ok((
                Self::NativeTrace(Box::new(super::native_trace::Capture::new(
                    options.clone(),
                )?)),
                format!(
                    "native-trace:{}",
                    super::digest(serde_json::to_string(options).unwrap().as_bytes())
                ),
            )),
            Source::Dart { options } => {
                let capture = super::dart::Capture::new(options.clone())?;
                Ok((
                    Self::Dart(Box::new(capture)),
                    format!(
                        "dart:{}",
                        super::digest(serde_json::to_string(options).unwrap().as_bytes())
                    ),
                ))
            }
            Source::ReactNative {
                endpoint,
                target_id,
                app_id,
                page_id,
                logical_device_id,
                mode,
                network,
                scripts,
                probe,
                allow_debugger_takeover,
                allow_profiler_control,
                sample_ms,
            } => {
                let capture = super::react_native::Capture::new(super::react_native::Options {
                    endpoint: endpoint.clone(),
                    target: target_id.clone(),
                    app: app_id.clone(),
                    page: page_id.clone(),
                    logical: logical_device_id.clone(),
                    mode: mode.clone(),
                    network: *network,
                    scripts: *scripts,
                    probe: *probe,
                    takeover: *allow_debugger_takeover,
                    profiler: *allow_profiler_control,
                    sample_ms: *sample_ms,
                })?;
                Ok((
                    Self::ReactNative(Box::new(capture)),
                    format!(
                        "rn:{}",
                        super::digest(
                            format!(
                                "{endpoint}:{target_id}:{app_id}:{page_id:?}:{logical_device_id:?}"
                            )
                            .as_bytes()
                        )
                    ),
                ))
            }
            Source::BrowserOperation {
                target_id,
                operation_id,
                request,
            } => {
                let (owners, pool) = browsers.ok_or_else(|| Error::new("NOT_CONNECTED", "Connect the selected browser and retain its operation before collecting browser evidence"))?;
                let mut value =
                    owners.diagnostic_operation(pool, target_id, operation_id, request)?;
                value["actionIds"] = json!(binding.action_ids);
                let bytes = serde_json::to_vec(&value)
                    .map_err(|_| Error::invalid("Cannot encode browser operation"))?;
                if bytes.len() > 8 * 1024 * 1024 {
                    return Err(Error::new(
                        "BROWSER_EVIDENCE_BOUNDS",
                        "Select a smaller native query page; browser envelope exceeds 8 MiB",
                    ));
                }
                Ok((
                    Self::Buffered {
                        bytes,
                        reason: "browser_operation_snapshot",
                    },
                    format!(
                        "browser-operation:{}",
                        super::digest(format!("{target_id}:{operation_id}").as_bytes())
                    ),
                ))
            }
            Source::Ios {
                target_id,
                app_id,
                transport,
                collect_logs,
                system_context,
                log_seconds,
                device_set,
                report_directory,
            } => {
                let capture = super::apple::Capture::new(
                    target_id,
                    app_id,
                    transport,
                    *collect_logs,
                    *system_context,
                    *log_seconds,
                    device_set.clone(),
                    report_directory.clone(),
                )?;
                Ok((
                    Self::Apple(Box::new(capture)),
                    format!(
                        "apple:{}",
                        super::digest(
                            format!("{target_id}:{app_id}:{transport}:{device_set:?}").as_bytes()
                        )
                    ),
                ))
            }
            Source::Import { path, format } => {
                label(format, 128)?;
                if !path.is_absolute() {
                    return Err(Error::invalid("Import path must be absolute"));
                }
                if !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
                    return Err(Error::new(
                        "DIAGNOSTIC_SOURCE",
                        "Import must be a regular file, not a symlink or special file",
                    ));
                }
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                let file = options
                    .open(path)
                    .map_err(|_| Error::new("DIAGNOSTIC_SOURCE", "Cannot open selected import"))?;
                let metadata = file.metadata().map_err(|_| {
                    Error::new("DIAGNOSTIC_SOURCE", "Cannot inspect selected import")
                })?;
                if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
                    return Err(Error::new(
                        "DIAGNOSTIC_SOURCE",
                        "Import must be a regular file of at most 8 MiB",
                    ));
                }
                // Canonical path is private input; the public identity is opaque.
                let identity = format!(
                    "{}:{:?}:{}",
                    path.display(),
                    metadata.modified().ok(),
                    metadata.len()
                );
                #[cfg(unix)]
                let identity = {
                    use std::os::unix::fs::MetadataExt;
                    format!("{identity}:{}:{}", metadata.dev(), metadata.ino())
                };
                Ok((
                    Self::Import { file, metadata },
                    format!("import:{:x}", Sha256::digest(identity.as_bytes())),
                ))
            }
            Source::BuildOutput { operation_id } => {
                builds.status(root, workspace, operation_id, false)?;
                Ok((
                    Self::Build {
                        operation: operation_id.clone(),
                        cursor: None,
                    },
                    operation_id.clone(),
                ))
            }
            Source::Android {
                target_id,
                app_id,
                user_id,
                mode,
                include_reports,
                system_context,
                since,
            } => {
                if !matches!(mode.as_str(), "snapshot" | "follow") {
                    return Err(Error::invalid("Android mode must be snapshot or follow"));
                }
                let capture = super::android::Capture::new(super::android::Options {
                    target: target_id.clone(),
                    app: app_id.clone(),
                    user: *user_id,
                    follow: mode == "follow",
                    reports: *include_reports,
                    system: *system_context,
                    since: since.clone(),
                    state: None,
                })?;
                Ok((
                    Self::Android(Box::new(capture)),
                    format!(
                        "android:{}",
                        super::digest(format!("{target_id}:{app_id}:{user_id}").as_bytes())
                    ),
                ))
            }
            Source::AndroidState {
                target_id,
                app_id,
                user_id,
                mode,
                groups,
                interval_ms,
                max_processes,
                label,
            } => {
                if !matches!(mode.as_str(), "snapshot" | "follow") {
                    return Err(Error::invalid("Android mode must be snapshot or follow"));
                }
                let state = super::android::state::Options {
                    groups: groups.clone(),
                    interval_ms: *interval_ms,
                    max_processes: *max_processes,
                    label: label.clone(),
                };
                state.validate()?;
                let capture = super::android::Capture::new(super::android::Options {
                    target: target_id.clone(),
                    app: app_id.clone(),
                    user: *user_id,
                    follow: mode == "follow",
                    reports: false,
                    system: false,
                    since: None,
                    state: Some(state),
                })?;
                Ok((
                    Self::Android(Box::new(capture)),
                    format!(
                        "android-state:{}",
                        super::digest(format!("{target_id}:{app_id}:{user_id}").as_bytes())
                    ),
                ))
            }
        }
    }
    pub fn context(&mut self, binding: &super::model::Binding) {
        if let Self::Embedded(capture) = self {
            capture.actions = binding.action_ids.clone();
        }
        if let Self::NativeTrace(capture) = self {
            capture.actions = binding.action_ids.clone();
        }
        if let Self::Dart(capture) = self {
            capture.actions = binding.action_ids.clone();
        }
        if let Self::ReactNative(capture) = self {
            capture.actions = binding.action_ids.clone();
        }
        if let Self::Android(capture) = self {
            capture.action_ids = binding.action_ids.clone();
        }
        if let Self::Apple(capture) = self {
            capture.actions = binding.action_ids.clone();
        }
    }
    pub async fn read(
        &mut self,
        builds: &builds::Owners,
        root: &Path,
        workspace: &str,
        offset: u64,
        budget: std::time::Duration,
        stopped: tokio::sync::watch::Receiver<Option<String>>,
    ) -> Result<Chunk> {
        match self {
            Self::Observability(capture) => {
                if stopped.borrow().is_some() {
                    return Err(Error::new("CANCELLED", "Observability read cancelled"));
                }
                let bytes = capture.read(budget).await?;
                Ok(Chunk::observed(
                    offset,
                    bytes,
                    true,
                    json!({"adapter":"observability-http/1","coverage":"selected_page","credentialsRefreshed":false,"mutationPerformed":false}),
                    "observability_selected_page",
                ))
            }
            Self::Host(capture) => {
                if stopped.borrow().is_some() {
                    return Err(Error::new("CANCELLED", "Host process read cancelled"));
                }
                let bytes = capture.read(budget).await?;
                Ok(Chunk::observed(
                    offset,
                    bytes,
                    true,
                    json!({"adapter":"host-process/1","ownership":"borrowed","coverage":"snapshot","signalsSent":false}),
                    "host_process_snapshot",
                ))
            }
            Self::BoxLogs(options) => {
                if stopped.borrow().is_some() {
                    return Err(Error::new("CANCELLED", "Box log read cancelled"));
                }
                let bytes = options.read(budget).await?;
                Ok(Chunk::observed(
                    offset,
                    bytes,
                    true,
                    json!({"adapter":"box-log/1","lifecycleOwner":"Box","coverage":"retained_tail","lifecycleChanged":false}),
                    "box_retained_log_tail",
                ))
            }
            Self::Embedded(capture) => {
                capture.control(stopped, budget);
                Ok(Chunk::captured(
                    offset,
                    capture.read().await?,
                    "embedded_observed_window",
                ))
            }
            Self::NativeTrace(capture) => {
                capture.control(stopped, budget);
                read_native_trace(capture, offset).await
            }
            Self::Dart(capture) => {
                capture.control(stopped, budget);
                Ok(Chunk::captured(
                    offset,
                    capture.read().await?,
                    "dart_observed_window",
                ))
            }
            Self::ReactNative(capture) => {
                capture.control(stopped, budget);
                Ok(Chunk::captured(
                    offset,
                    capture.read().await?,
                    "react_native_sampled_window",
                ))
            }
            Self::Buffered { bytes, reason } => read_buffered(bytes, reason, offset),
            Self::Apple(capture) => {
                capture.control(stopped, budget);
                Ok(Chunk::captured(
                    offset,
                    capture.read().await?,
                    "apple_sampled_window",
                ))
            }
            Self::Import { file, metadata } => read_import(file, metadata, offset),
            Self::Build { operation, cursor } => {
                read_build(builds, root, workspace, operation, cursor, offset)
            }
            Self::Android(capture) => {
                capture.control(stopped, budget);
                let batch = capture.read().await?;
                Ok(Chunk::observed(
                    offset,
                    batch.bytes,
                    batch.complete,
                    batch.summary,
                    "android_sampled_window",
                ))
            }
        }
    }
    pub async fn settle(&mut self) -> Option<serde_json::Value> {
        if let Self::Observability(capture) = self {
            return Some(capture.settle().await);
        }
        if let Self::Embedded(capture) = self {
            return Some(capture.settle().await);
        }
        if let Self::NativeTrace(capture) = self {
            return Some(capture.settle().await);
        }
        if let Self::Dart(capture) = self {
            return Some(capture.settle().await);
        }
        if let Self::ReactNative(capture) = self {
            Some(capture.settle().await)
        } else {
            None
        }
    }
}

impl Chunk {
    fn captured(offset: u64, capture: (Vec<u8>, bool, serde_json::Value), reason: &str) -> Self {
        let (bytes, complete, summary) = capture;
        Self::observed(offset, bytes, complete, summary, reason)
    }
    fn observed(
        offset: u64,
        bytes: Vec<u8>,
        complete: bool,
        summary: serde_json::Value,
        reason: &str,
    ) -> Self {
        Self {
            offset: offset + bytes.len() as u64,
            bytes,
            complete,
            summary: Some(summary),
            reason: Some(reason.into()),
        }
    }
}

fn read_import(file: &mut File, metadata: &Metadata, offset: u64) -> Result<Chunk> {
    let mut bytes = vec![0; 16 * 1024];
    let count = file
        .read(&mut bytes)
        .map_err(|_| Error::new("DIAGNOSTIC_SOURCE", "Import read failed"))?;
    bytes.truncate(count);
    let next = offset + count as u64;
    let complete = next == metadata.len();
    if complete {
        let after = file
            .metadata()
            .map_err(|_| Error::new("DIAGNOSTIC_SOURCE", "Import identity unavailable"))?;
        if after.len() != metadata.len() || after.modified().ok() != metadata.modified().ok() {
            return Err(Error::new(
                "DIAGNOSTIC_SOURCE_CHANGED",
                "Import changed during collection",
            ));
        }
    } else if count == 0 {
        return Err(Error::new(
            "DIAGNOSTIC_SOURCE_CHANGED",
            "Import ended before its observed size",
        ));
    }
    Ok(Chunk {
        bytes,
        offset: next,
        complete,
        summary: None,
        reason: None,
    })
}
fn read_build(
    builds: &builds::Owners,
    root: &Path,
    workspace: &str,
    operation: &str,
    cursor: &mut Option<String>,
    offset: u64,
) -> Result<Chunk> {
    let mut args = canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(16384));
    args["operationId"] = json!(operation);
    let value = builds.log(root, workspace, &args)?;
    *cursor = value["nextCursor"].as_str().map(str::to_owned);
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value["dataBase64"].as_str().unwrap_or_default())
        .map_err(|_| Error::new("DIAGNOSTIC_SOURCE", "Operation log bytes are invalid"))?;
    Ok(Chunk {
        complete: value["settled"] == true && cursor.is_none(),
        offset: offset + bytes.len() as u64,
        bytes,
        summary: None,
        reason: None,
    })
}

async fn read_native_trace(
    capture: &mut super::native_trace::Capture,
    offset: u64,
) -> Result<Chunk> {
    let result = capture.read(offset).await?;
    let reason = if result.2["xctraceRecording"]["deviceDisconnected"] == true {
        "device_disconnected"
    } else {
        "native_trace_window"
    };
    Ok(Chunk::captured(offset, result, reason))
}

fn read_buffered(bytes: &[u8], reason: &str, offset: u64) -> Result<Chunk> {
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    if start > bytes.len() {
        return Err(Error::invalid("Invalid buffered evidence cursor"));
    }
    let end = start.saturating_add(16384).min(bytes.len());
    Ok(Chunk {
        bytes: bytes[start..end].to_vec(),
        offset: end as u64,
        complete: end == bytes.len(),
        summary: None,
        reason: Some(reason.into()),
    })
}
