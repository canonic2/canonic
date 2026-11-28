//! Explicit retained Mach-O/DWARF symbolication. UUID/architecture checked before atos.
use super::{model::*, *};
use crate::diagnostics::apple::{Temporary, reports};
pub(super) async fn apply(
    dir: &Path,
    index: &mut Index,
    metadata: &Metadata,
    cancelled: &watch::Receiver<Option<String>>,
) -> Result<()> {
    for r in &mut index.rows {
        if let Some(data) = r.data.as_object_mut() {
            data.remove("symbolication");
            data.insert("resolved".into(), json!(false));
        }
    }
    if !cfg!(target_os = "macos") && std::env::var_os("TESTRUN_XCRUN").is_none() {
        index
            .unsupported
            .push("apple_symbolication_requires_macos_xcode".into());
        return Ok(());
    }
    let began = Instant::now();
    let temporary = Temporary::new()?;
    let xcrun = crate::simulator::device_set::executable();
    let mut state = Symbolication {
        began,
        xcrun,
        remaining_frames: 256,
        cancelled,
    };
    for (n, symbol) in metadata.symbols.iter().enumerate() {
        if cancelled.borrow().is_some() {
            return Err(Error::new(
                "ANALYSIS_CANCELLED",
                "Apple symbolication cancelled before another tool",
            ));
        }
        let budget = Duration::from_secs(10)
            .saturating_sub(began.elapsed())
            .min(Duration::from_secs(3));
        if budget.is_zero() || state.remaining_frames == 0 {
            index
                .unsupported
                .push("apple_symbolication_time_or_frame_limit".into());
            break;
        }
        let bytes = read_data(dir, &symbol.evidence_id)?;
        if digest(&bytes) != symbol.sha256 {
            return Err(Error::new(
                "EVIDENCE_CHANGED",
                "Retained Apple symbol bytes changed",
            ));
        }
        let file = temporary.0.join(format!("image-{n}"));
        artifacts::write(&file, &bytes)?;
        let budget = Duration::from_secs(10)
            .saturating_sub(began.elapsed())
            .min(Duration::from_secs(3));
        if budget.is_zero() {
            index
                .unsupported
                .push("apple_symbolication_deadline".into());
            break;
        }
        let dump = match crate::process::run_diagnostic(
            &state.xcrun,
            &["dwarfdump", "--uuid", file.to_str().unwrap()],
            budget,
            65536,
        )
        .await
        {
            Ok(o) => String::from_utf8_lossy(&o.bytes).into_owned(),
            Err(_) => {
                index
                    .unsupported
                    .push("apple_symbol_uuid_tool_unavailable_or_failed".into());
                continue;
            }
        };
        let expected = reports::uuid(&symbol.build_id).unwrap();
        let verified = dump.lines().any(|line| {
            let w: Vec<_> = line.split_whitespace().collect();
            w.len() >= 3
                && w[0] == "UUID:"
                && reports::uuid(w[1]) == Some(expected.clone())
                && w[2].trim_matches(['(', ')']) == symbol.architecture
        });
        if !verified {
            index
                .unsupported
                .push("apple_symbol_uuid_or_architecture_mismatch".into());
            continue;
        }
        let groups = symbol_groups(index, symbol, &expected, state.remaining_frames);
        state.resolve_groups(index, symbol, &file, groups).await?;
    }
    if cancelled.borrow().is_some() {
        return Err(Error::new(
            "ANALYSIS_CANCELLED",
            "Apple symbolication cancelled after tool settlement",
        ));
    }
    validate_symbolicated_rows(index)
}
fn valid_address(s: &str) -> bool {
    s.strip_prefix("0x")
        .is_some_and(|v| !v.is_empty() && v.len() <= 16 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

struct Symbolication<'a> {
    began: Instant,
    xcrun: PathBuf,
    remaining_frames: usize,
    cancelled: &'a watch::Receiver<Option<String>>,
}
impl Symbolication<'_> {
    async fn resolve_groups(
        &mut self,
        index: &mut Index,
        symbol: &Symbol,
        file: &Path,
        groups: std::collections::BTreeMap<String, Vec<usize>>,
    ) -> Result<()> {
        for (load, rows) in groups {
            if self.cancelled.borrow().is_some() {
                return Err(Error::new(
                    "ANALYSIS_CANCELLED",
                    "Apple symbolication cancelled",
                ));
            }
            let budget = Duration::from_secs(10)
                .saturating_sub(self.began.elapsed())
                .min(Duration::from_secs(3));
            if budget.is_zero() {
                index
                    .unsupported
                    .push("apple_symbolication_deadline".into());
                break;
            }
            let addresses: Vec<_> = rows
                .iter()
                .map(|i| index.rows[*i].data["address"].as_str().unwrap().to_string())
                .collect();
            if !valid_address(&load) || addresses.iter().any(|a| !valid_address(a)) {
                index.unsupported.push("apple_frame_address_invalid".into());
                continue;
            }
            let mut args = vec![
                "atos",
                "-arch",
                &symbol.architecture,
                "-o",
                file.to_str().unwrap(),
                "-l",
                &load,
            ];
            args.extend(addresses.iter().map(String::as_str));
            match crate::process::run_diagnostic(&self.xcrun, &args, budget, 128 * 1024).await {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.bytes);
                    let names: Vec<_> = text.lines().collect();
                    if names.len() != rows.len() {
                        index
                            .unsupported
                            .push("apple_atos_output_unrecognized".into());
                        continue;
                    }
                    for ((i, address), name) in rows.iter().zip(addresses.iter()).zip(names) {
                        self.remaining_frames -= 1;
                        if name.len() > 4096
                            || name.trim() == address
                            || name.trim_start().starts_with("0x")
                            || name.trim().is_empty()
                            || !name.contains(" (in ")
                        {
                            index
                                .unsupported
                                .push("apple_atos_output_unresolved".into());
                            continue;
                        }
                        index.rows[*i].data["resolved"] = json!(true);
                        index.rows[*i].data["symbolication"] = json!({"symbol":name,"method":"atos_uuid_arch_verified","artifactEvidenceId":symbol.evidence_id,"artifactSha256":symbol.sha256,"buildId":symbol.build_id,"architecture":symbol.architecture,"loadAddress":load,"address":address});
                    }
                }
                Err(_) => index
                    .unsupported
                    .push("apple_atos_unavailable_or_failed".into()),
            }
        }
        Ok(())
    }
}

fn symbol_groups(
    index: &Index,
    symbol: &Symbol,
    expected: &str,
    remaining_frames: usize,
) -> std::collections::BTreeMap<String, Vec<usize>> {
    let rows: Vec<_> = index
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.kind == "stack"
                && r.data["buildId"].as_str().and_then(reports::uuid) == Some(expected.to_owned())
                && r.data["architecture"] == symbol.architecture
                && r.data["loadAddress"].is_string()
                && r.data["address"].is_string()
        })
        .take(remaining_frames)
        .map(|(i, _)| i)
        .collect();
    // One bounded invocation per distinct load address; ASLR loads are never guessed.
    let mut groups = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for i in rows {
        groups
            .entry(index.rows[i].data["loadAddress"].as_str().unwrap().into())
            .or_default()
            .push(i);
    }
    groups
}

fn validate_symbolicated_rows(index: &Index) -> Result<()> {
    for r in &index.rows {
        r.validate()?;
        if serde_json::to_vec(r)
            .map_err(|_| Error::new("ANALYSIS_FAILED", "Cannot encode symbolicated row"))?
            .len()
            > 65536
        {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Symbolicated row exceeds bound",
            ));
        }
    }
    Ok(())
}
