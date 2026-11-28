//! Project Activity Monitor's system-wide XML to two trace-local process series.
use super::*;
use serde::{Deserialize, Serialize};

const MAX_SAMPLES: usize = 4096;
const RENDER_SERVER: &str = "backboardd";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Sample {
    source_row: usize,
    start_ns: u64,
    pid: u32,
    process_name: String,
    process_ref: Option<String>,
    cpu_percent: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Export {
    samples: Vec<Sample>,
    selected_rows_without_cpu: usize,
    rows_without_process_identity: usize,
}

fn field<'a>(row: &'a Node, name: &str, map: &References<'a>) -> Result<Option<&'a Node>> {
    let Some(mut node) = child(row, name) else {
        return Ok(None);
    };
    for _ in 0..32 {
        let Some(target) = reference_target(node, map)? else {
            return Ok(Some(node));
        };
        node = target;
    }
    Err(invalid())
}

pub(crate) fn activity_monitor_export(xml: &[u8], app_pid: u32) -> Result<String> {
    let root = parse_bounded(std::str::from_utf8(xml).map_err(|_| invalid())?, 150000)?;
    let mut references = BTreeMap::new();
    ids(&root, &mut references)?;
    let mut rows = vec![];
    collect(&root, "row", &mut rows);
    let mut output = Export {
        samples: vec![],
        selected_rows_without_cpu: 0,
        rows_without_process_identity: 0,
    };
    for (source_row, row) in rows.into_iter().enumerate() {
        let identity = match field(row, "process", &references)? {
            Some(process) => field(process, "pid", &references)?
                .and_then(|n| n.text.trim().parse::<u32>().ok())
                .map(|pid| (process, pid)),
            None => None,
        };
        let Some((process, pid)) = identity else {
            output.rows_without_process_identity += 1;
            continue;
        };
        let label = process.attrs.get("fmt").map(String::as_str).unwrap_or("");
        let suffix = format!(" ({pid})");
        let name = label.strip_suffix(&suffix).unwrap_or(label);
        if pid != app_pid && name != RENDER_SERVER {
            continue;
        }
        let start_ns =
            field(row, "start-time", &references)?.and_then(|n| n.text.trim().parse::<u64>().ok());
        let cpu_percent = field(row, "system-cpu-percent", &references)?
            .and_then(|n| n.text.trim().parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.0);
        let (Some(start_ns), Some(cpu_percent)) = (start_ns, cpu_percent) else {
            output.selected_rows_without_cpu += 1;
            continue;
        };
        if output.samples.len() >= MAX_SAMPLES || name.len() > 256 {
            return Err(Error::new(
                "NATIVE_TRACE_BOUNDS",
                "Activity Monitor samples exceed the structured evidence bounds",
            ));
        }
        output.samples.push(Sample {
            source_row,
            start_ns,
            pid,
            process_name: name.into(),
            process_ref: process.attrs.get("id").cloned(),
            cpu_percent,
        });
    }
    let text = serde_json::to_string(&output).map_err(|_| invalid())?;
    if text.len() > 1024 * 1024 {
        return Err(Error::new(
            "NATIVE_TRACE_BOUNDS",
            "Activity Monitor export exceeds the structured evidence bounds",
        ));
    }
    Ok(text)
}

pub(in super::super) fn decode_activity_monitor(
    index: &mut Index,
    envelope: &Value,
    content: &str,
    section: usize,
) -> Result<()> {
    let pid = envelope["report"]["selectedAppPid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid > 0)
        .ok_or_else(invalid)?;
    let export: Export = serde_json::from_str(content).map_err(|_| invalid())?;
    if export.samples.len() > MAX_SAMPLES {
        return Err(invalid());
    }
    let mut app = BTreeSet::new();
    let mut render_server = BTreeSet::new();
    for (i, sample) in export.samples.iter().enumerate() {
        let (identities, role, name) = if sample.process_name == RENDER_SERVER {
            (&mut render_server, "render_server", "iOS render server CPU")
        } else if sample.pid == pid {
            (&mut app, "app", "iOS app CPU")
        } else {
            return Err(invalid());
        };
        let valid_ref = sample.process_ref.as_ref().is_none_or(|id| id.len() <= 64);
        if sample.pid == 0 || sample.process_name.len() > 256 || !valid_ref {
            return Err(invalid());
        }
        identities.insert((sample.pid, &sample.process_name, &sample.process_ref));
        emit(
            index,
            envelope,
            "metric",
            name,
            format!("/sections/{section}/text/sample/{i}"),
            Some(sample.start_ns as f64),
            Some((
                sample.cpu_percent,
                "percent",
                "Activity Monitor per-process CPU percent as exported",
            )),
            json!({"table":ACTIVITY_MONITOR_TABLE,"processRole":role,"pid":sample.pid,"processName":sample.process_name,"traceProcessRef":sample.process_ref,"exportedRow":sample.source_row,"sharedSystemProcess":role=="render_server"}),
        )?;
    }
    for (name, identities) in [("selected app PID", app), (RENDER_SERVER, render_server)] {
        if identities.is_empty() {
            index
                .unsupported
                .push(format!("Activity Monitor: no CPU samples for {name}"));
        } else if identities.len() > 1 {
            index.unsupported.push(format!("Activity Monitor: multiple trace process identities for {name}; inspect them separately"));
        }
    }
    if export.selected_rows_without_cpu > 0 || export.rows_without_process_identity > 0 {
        index.unsupported.push(format!(
            "Activity Monitor: {} selected rows lacked a usable time or CPU value; {} rows lacked a process identity",
            export.selected_rows_without_cpu, export.rows_without_process_identity,
        ));
    }
    Ok(())
}
