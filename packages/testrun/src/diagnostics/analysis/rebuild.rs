//! Rebuilds derived indexes written by an earlier parser from their raw evidence.
//!
//! The raw collector payload is the recovery source: a rebuild reads it after
//! checking its digest, so the earlier index needs no backup. Publication
//! stages the new index, records its digest in the receipt and then renames it
//! over the earlier one; a reader that finds the receipt ahead of the index
//! finishes that rename.
use super::*;

/// Internal error code that asks [`Owners::inspect_analysis`] to rebuild one
/// index and retry; it never reaches a caller.
pub(super) const REBUILD: &str = "INDEX_REBUILD_REQUIRED";

/// The staged file of an index being rebuilt.
pub(super) fn staged_name(analysis: &str) -> String {
    format!("{analysis}.next.index.json")
}

/// The parser generation that wrote an index. Newer versions or generations
/// are refused unchanged; an unreadable header is damage.
pub(super) fn index_generation(bytes: &[u8], path: &Path) -> Result<u64> {
    #[derive(Deserialize)]
    struct Header {
        version: u64,
        parser: String,
    }
    let damaged = || {
        Error::new("STATE_IO", "Derived index is damaged").details(json!({"path":path,"recovery":"Run diagnostics_analyze with a new submissionId to derive another index"}))
    };
    let header: Header = serde_json::from_slice(bytes).map_err(|_| damaged())?;
    let generation = header
        .parser
        .strip_prefix("testrun-diagnostics/")
        .and_then(|g| g.parse::<u64>().ok())
        .ok_or_else(damaged)?;
    if header.version > 1 || generation > model::PARSER_GENERATION {
        return Err(Error::new(
            "STATE_FORMAT",
            format!(
                "The derived index was written by parser {} (version {}); this Testrun writes {PARSER} (version 1)",
                header.parser, header.version
            ),
        )
        .details(json!({"path":path,"observedVersion":header.version,"observedParser":header.parser,"supportedVersion":1,"supportedParser":PARSER,"recovery":"Upgrade Testrun, or use the newer Testrun that wrote it; this file is unchanged"})));
    }
    // No released Testrun wrote version 0; it is rebuilt like an earlier parser's.
    Ok(if header.version < 1 { 0 } else { generation })
}

impl Owners {
    /// Finishes an interrupted rebuild: the receipt already names the staged
    /// index. Returns the bytes of the index the receipt names.
    pub(super) fn resume_rebuilt_index(
        &self,
        dir: &Path,
        analysis: &str,
        receipt: &Receipt,
    ) -> Result<Vec<u8>> {
        let staged = dir.join(staged_name(analysis));
        let bytes = read_private(&staged, MAX_INDEX)
            .ok()
            .filter(|bytes| Some(digest(bytes)) == receipt.index_sha256)
            .ok_or_else(|| Error::new("EVIDENCE_CHANGED", "Derived evidence changed"))?;
        if !self.offline {
            publish(dir, analysis)?;
        }
        Ok(bytes)
    }

    /// Rebuilds one analysis's index from its verified raw evidence. Parsing
    /// runs without the state lock; the result is published under it only if
    /// the receipt still names the index this rebuild replaced. Without a
    /// manager the rebuilt index is kept in memory and nothing is written.
    pub(super) fn rebuild_index(
        &self,
        root: &Path,
        workspace: &str,
        investigation_id: &str,
        analysis: &str,
    ) -> Result<()> {
        let dir = directory(root, workspace, investigation_id)?;
        let (receipt, raw) = {
            let mut state = self.state.lock().unwrap();
            self.load(&mut state, &dir)?;
            let receipt = load_receipt(&dir, analysis)?;
            let raw = read_data(&dir, &receipt.collector_id)?;
            if digest(&raw) != receipt.raw_sha256 {
                return Err(Error::new("EVIDENCE_CHANGED", "Raw evidence changed"));
            }
            (receipt, raw)
        };
        let index = block_on(async {
            let (_sender, mut cancelled) = watch::channel(None);
            let mut index = worker::run(&receipt.format, &raw, &mut cancelled).await?;
            if receipt.metadata.symbolicate {
                symbolize::apply(&dir, &mut index, &receipt.metadata, &cancelled).await?;
            }
            Ok::<_, Error>(index)
        })
        .and_then(|index| index)
        .and_then(|index| {
            check_clocks(&receipt.metadata, &index)?;
            Ok(index)
        })
        .map_err(|error| {
            Error::new(
                "STATE_FORMAT",
                "The derived index was written by an earlier parser and could not be rebuilt from its raw evidence",
            )
            .details(json!({"path":dir.join(format!("{analysis}.index.json")),"supportedParser":PARSER,"cause":error.code,"recovery":"Run diagnostics_analyze with a new submissionId; the earlier index and raw evidence are unchanged"}))
        })?;
        let bytes = serde_json::to_vec(&index)
            .map_err(|_| Error::new("ANALYSIS_FAILED", "Cannot encode derived index"))?;
        if bytes.len() > MAX_INDEX {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Derived index exceeds its budget",
            ));
        }
        let mut state = self.state.lock().unwrap();
        let mut current = load_receipt(&dir, analysis)?;
        if current.index_sha256 != receipt.index_sha256 || current.phase != "complete" {
            // Another caller published first, or the analysis changed.
            return Ok(());
        }
        let key = dir.join(analysis);
        if state.analysis_indexes.len() >= investigation::MAX_SOURCES {
            state.analysis_indexes.clear();
        }
        if self.offline {
            // Keyed by the earlier file's digest, which the receipt still names.
            let earlier = current.index_sha256.clone().unwrap_or_default();
            state
                .analysis_indexes
                .insert(key, (earlier, Arc::new(index)));
            return Ok(());
        }
        let staged = dir.join(staged_name(analysis));
        match std::fs::remove_file(&staged) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(Error::new("EVIDENCE_IO", "Cannot replace a staged index"));
            }
            _ => {}
        }
        artifacts::write(&staged, &bytes)?;
        let hash = digest(&bytes);
        current.index_sha256 = Some(hash.clone());
        current.unsupported.clone_from(&index.unsupported);
        if let Some(cost) = &mut current.cost {
            cost.index_bytes = bytes.len() as u64;
        }
        save(&dir, &current)?;
        publish(&dir, analysis)?;
        state.analysis_indexes.insert(key, (hash, Arc::new(index)));
        Ok(())
    }
}

/// Replaces the index with its staged successor.
fn publish(dir: &Path, analysis: &str) -> Result<()> {
    std::fs::rename(dir.join(staged_name(analysis)), index_path(dir, analysis)?)
        .and_then(|()| artifacts::sync(&std::fs::File::open(dir)?))
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot publish a rebuilt index"))
}

/// Runs a rebuild to completion from synchronous inspection code, on its own
/// thread and runtime so it works inside or outside an existing runtime.
fn block_on<T: Send>(future: impl std::future::Future<Output = T> + Send) -> Result<T> {
    let run = || {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map(|runtime| runtime.block_on(future))
                })
                .join()
        })
    };
    let finished = match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(run)
        }
        _ => run(),
    };
    match finished {
        Ok(Ok(value)) => Ok(value),
        _ => Err(Error::new(
            "ANALYSIS_FAILED",
            "Parser supervision stopped unexpectedly",
        )),
    }
}
