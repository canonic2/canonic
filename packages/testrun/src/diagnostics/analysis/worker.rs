//! One-shot parser process: stdin/stdout only, no runtime, device or network clients.
use super::{decode, model::*};
use crate::error::{Error, Result};
use base64::Engine;
use serde_json::json;
use std::{
    io::{Read, Write},
    process::{ExitCode, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

/// Internal executable entry point, before creating the async/device runtime.
pub fn entry() -> Option<ExitCode> {
    if std::env::args().nth(1).as_deref() != Some("diagnostic-parser-worker") {
        return None;
    }
    Some(if execute_stdio().is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
fn execute_stdio() -> Result<()> {
    memory_limit()?;
    let mut input = Vec::new();
    std::io::stdin()
        .take((12 * 1024 * 1024 + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|_| Error::new("ANALYSIS_IO", "Cannot read parser input"))?;
    let result = (|| {
        let request = decode::bounded_json(&input, 12 * 1024 * 1024)?;
        if request.as_object().is_none_or(|o| o.len() != 2) {
            return Err(Error::invalid("Invalid parser envelope"));
        }
        let format = request["format"]
            .as_str()
            .ok_or_else(|| Error::invalid("Missing parser format"))?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(request["rawBase64"].as_str().unwrap_or_default())
            .map_err(|_| Error::invalid("Invalid parser bytes"))?;
        decode::decode(format, &raw)
    })();
    let value = match result {
        Ok(index) => json!({"index":index}),
        Err(e) => json!({"error":e.code}),
    };
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| Error::new("ANALYSIS_IO", "Cannot encode parser output"))?;
    if bytes.len() > MAX_INDEX + 1024 {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Parser output exceeds its budget",
        ));
    }
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|_| Error::new("ANALYSIS_IO", "Cannot write parser output"))
}
fn memory_limit() -> Result<()> {
    #[cfg(unix)]
    {
        let cpu = libc::rlimit {
            rlim_cur: 5,
            rlim_max: 5,
        };
        // SAFETY: limits affect only this disposable, single-threaded worker.
        if unsafe { libc::setrlimit(libc::RLIMIT_CPU, &cpu) } != 0 {
            return Err(Error::new("ANALYSIS_LIMITS", "Cannot bound parser CPU"));
        }
        #[cfg(target_os = "linux")]
        {
            let memory = libc::rlimit {
                rlim_cur: 512 * 1024 * 1024,
                rlim_max: 512 * 1024 * 1024,
            };
            // SAFETY: this worker owns its address space and has not started threads.
            if unsafe { libc::setrlimit(libc::RLIMIT_AS, &memory) } != 0 {
                return Err(Error::new(
                    "ANALYSIS_LIMITS",
                    "Cannot bound parser address space",
                ));
            }
        }
    }
    // All hosts additionally enforce byte/depth/node/row/stack budgets before
    // materialization; these are data budgets, not an OS resident-memory promise.
    Ok(())
}
pub async fn run(
    format: &str,
    raw: &[u8],
    cancelled: &mut watch::Receiver<Option<String>>,
) -> Result<Index> {
    if cancelled.borrow().is_some() {
        return Err(Error::new("ANALYSIS_CANCELLED", "Analysis stopped"));
    }
    let executable = std::env::current_exe()
        .map_err(|_| Error::new("ANALYSIS_IO", "Parser executable unavailable"))?;
    let mut child = tokio::process::Command::new(executable)
        .arg("diagnostic-parser-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Error::new("ANALYSIS_IO", "Cannot start isolated parser"))?;
    let request = serde_json::to_vec(
        &json!({"format":format,"rawBase64":base64::engine::general_purpose::STANDARD.encode(raw)}),
    )
    .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let io = async {
        let write = async move {
            stdin.write_all(&request).await?;
            stdin.shutdown().await?;
            drop(stdin);
            Ok::<(), std::io::Error>(())
        };
        let read = async {
            let mut output = Vec::new();
            stdout
                .take((MAX_INDEX + 1025) as u64)
                .read_to_end(&mut output)
                .await?;
            Ok::<_, std::io::Error>(output)
        };
        let (_, output) = tokio::try_join!(write, read)
            .map_err(|_| Error::new("ANALYSIS_IO", "Parser transport failed"))?;
        let status = child
            .wait()
            .await
            .map_err(|_| Error::new("ANALYSIS_IO", "Cannot reap parser"))?;
        if !status.success() {
            return Err(Error::new(
                "ANALYSIS_FAILED",
                "Parser exited before producing a valid index",
            ));
        }
        // The worker's index is already bounded by rows, per-row bytes and MAX_INDEX;
        // its token budget follows that byte bound (a Dart class snapshot has ~1,800 rows).
        let result = decode::bounded_json_tokens(&output, MAX_INDEX + 1024, MAX_INDEX / 4)?;
        if let Some(code) = result["error"].as_str() {
            return Err(Error::new(code, "Diagnostic decoder refused the artifact"));
        }
        serde_json::from_value(result["index"].clone())
            .map_err(|_| Error::new("ANALYSIS_FAILED", "Invalid parser output"))
    };
    let result = tokio::select! {
        result=tokio::time::timeout(Duration::from_secs(10),io)=>result.unwrap_or_else(|_|Err(Error::new("ANALYSIS_TIMEOUT","Parser exceeded 10 seconds"))),
        _=cancelled.changed()=>Err(Error::new("ANALYSIS_CANCELLED","Analysis stopped"))
    };
    // Explicitly reap on timeout/cancel; dropping a future is not finalization.
    if child.id().is_some() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}
