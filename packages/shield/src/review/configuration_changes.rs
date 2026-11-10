//! Nonauthorizing configuration summaries, independent of request observations.
use super::{
    Context, Error,
    model::State,
    storage::{objects, private_files},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const RECORDS: usize = 1000;
const BYTES: usize = 8192;
const QUEUE: usize = 16;
const STORE_BYTES: usize = RECORDS * (BYTES + 1) + 128;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Records {
    schema_version: u8,
    records: Vec<Value>,
}

/// Unknown optional metadata survives writes without affecting active policy.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum Log {
    Known(Records),
    Opaque(Value),
}
impl Default for Log {
    fn default() -> Self {
        Self::Known(Records {
            schema_version: 1,
            records: vec![],
        })
    }
}
impl Log {
    fn records(&self) -> Option<&Records> {
        match self {
            Self::Known(rows)
                if rows.schema_version == 1
                    && rows.records.len() <= RECORDS
                    && rows
                        .records
                        .iter()
                        .all(|row| row["schema_version"] == 1 && crate::json::fits(row, BYTES)) =>
            {
                Some(rows)
            }
            _ => None,
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.records().is_some_and(|rows| rows.records.is_empty())
    }
    pub(super) fn view(&self) -> Value {
        match self.records() {
            Some(rows) => json!({"schema_version":1,"available":true,
                "configuration_changes":rows.records.iter().rev().collect::<Vec<_>>(),
                "limit":RECORDS,"record_bytes_limit":BYTES,"authority":"none"}),
            None => json!({"schema_version":1,"available":false,
                "configuration_changes":[],"authority":"none",
                "reason":"Unsupported configuration history is preserved in the checkpoint."}),
        }
    }
    pub(super) fn record(&mut self, event: &Value, previous_revision: Option<&str>) -> bool {
        let kind = event["kind"].as_str().unwrap_or_default();
        if !matches!(
            kind,
            "runtime_activated"
                | "proposal_rejected"
                | "integration_review_confirmed"
                | "integration_recovery_cancelled"
                | "configuration_changed"
        ) || self.records().is_none()
        {
            return false;
        }
        let actor = if crate::json::fits(&event["provenance"], 4096) {
            event["provenance"].clone()
        } else {
            json!({"available":false,"digest":crate::json::fingerprint(&event["provenance"]).expect("event JSON"),"reason":"Actor evidence exceeds the summary bound."})
        };
        let mut affected = json!({});
        for name in ["activation_id", "proposal_id"] {
            if !event["links"][name].is_null() {
                affected[name] = event["links"][name].clone();
            }
        }
        for name in ["candidate_digest", "setting", "registry"] {
            if !event["details"][name].is_null() {
                affected[name] = event["details"][name].clone();
            }
        }
        let rejected = kind == "proposal_rejected";
        let mut row = json!({"schema_version":1,"change_id":event["event_id"],
            "recorded_at":event["recorded_at"],"operation":kind.replace('_', " "),
            "actor":actor,"operator":event["operator"],"affected":affected,
            "before_revision":previous_revision,"after_revision":event["details"].get("policy_revision").cloned().unwrap_or_else(|| json!(previous_revision)),
            "decision":if rejected {"rejected"} else {"confirmed"},
            "commit_outcome":if rejected {"not_applied"} else {"committed"},"authority":"none"});
        super::model::redact(&mut row);
        if !crate::json::fits(&row, BYTES) {
            row = json!({"schema_version":1,"change_id":event["event_id"],
                "recorded_at":event["recorded_at"],"operation":kind,
                "before_revision":previous_revision,"after_revision":event["details"].get("policy_revision").cloned().unwrap_or_else(|| json!(previous_revision)),
                "decision":if rejected {"rejected"} else {"confirmed"},
                "commit_outcome":if rejected {"not_applied"} else {"committed"},
                "evidence_digest":crate::json::fingerprint(event).expect("event JSON"),
                "omitted":"Detailed evidence exceeds the summary bound.","authority":"none"});
        }
        if !crate::json::fits(&row, BYTES) {
            return false;
        }
        let Self::Known(rows) = self else {
            unreachable!("validated log")
        };
        if rows
            .records
            .iter()
            .any(|old| old["change_id"] == row["change_id"])
        {
            return false;
        }
        rows.records.push(row);
        if rows.records.len() > RECORDS {
            rows.records.remove(0);
        }
        true
    }
    pub(super) fn trim_queue(&mut self) {
        if let Self::Known(rows) = self {
            let excess = rows.records.len().saturating_sub(QUEUE);
            rows.records.drain(..excess);
        }
    }
    fn merge(&mut self, other: &Self) -> Result<(), Error> {
        let incoming = other
            .records()
            .ok_or(Error::Invalid("unsupported configuration summary queue"))?;
        if self.records().is_none() {
            return Err(Error::Invalid("unsupported configuration history"));
        }
        let Self::Known(rows) = self else {
            unreachable!("validated log")
        };
        for row in &incoming.records {
            if !rows
                .records
                .iter()
                .any(|old| old["change_id"] == row["change_id"])
            {
                rows.records.push(row.clone());
            }
        }
        // Authority allocates event sequence numbers before releasing its
        // writer. Optional exports may arrive in the opposite order; wall
        // clocks and export arrival order cannot decide retention.
        if rows.records.iter().all(|row| sequence(row).is_some()) {
            rows.records.sort_by_key(sequence);
        }
        let excess = rows.records.len().saturating_sub(RECORDS);
        rows.records.drain(..excess);
        Ok(())
    }
}

fn sequence(row: &Value) -> Option<u64> {
    row["change_id"].as_str()?.rsplit('-').next()?.parse().ok()
}

fn directory(state: &State) -> std::path::PathBuf {
    std::path::Path::new(&state.project_root).join(".shield/review/configuration-changes")
}
fn load(directory: &std::path::Path) -> Result<Log, Error> {
    match private_files::directory(directory, false) {
        Ok(()) => {}
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Log::default());
        }
        Err(error) => return Err(error),
    }
    match private_files::read(&directory.join("summaries.json"), STORE_BYTES) {
        Ok(value) => serde_json::from_value(value)
            .map_err(|_| Error::Invalid("configuration history encoding")),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(Log::default()),
        Err(error) => Err(error),
    }
}

/// Only configuration publication reaches this path, after the authority commit.
/// A failure preserves the checkpoint queue and never rolls back permission state.
pub(super) fn export(state: &mut State) {
    if !state.configuration_changes_dirty {
        return;
    }
    let directory = directory(state);
    let _ = (|| -> Result<(), Error> {
        private_files::directory(&directory, true)?;
        let _guard = objects::Directory::try_enter(&directory)?;
        let result = (|| -> Result<(), Error> {
            let mut log = load(&directory)?;
            log.merge(&state.configuration_changes)?;
            private_files::write(
                &directory.join("summaries.json"),
                &serde_json::to_value(log)
                    .map_err(|_| Error::Invalid("configuration history encoding"))?,
                STORE_BYTES,
            )
        })();
        if result.is_err() {
            // Fixed-name publication requires this same namespace lease,
            // including the small sticky failure marker.
            let _ = private_files::write(
                &directory.join("incomplete.json"),
                &json!({"incomplete":true}),
                128,
            );
        }
        result
    })();
    state.configuration_changes_dirty = false;
}

/// Explicit menu inspection does not create directories, acquire a writer or
/// hydrate policy. The committed queue covers a crash before optional export.
pub(super) fn inspect(state: &State) -> Value {
    let directory = directory(state);
    let mut incomplete = !matches!(
        private_files::read(&directory.join("incomplete.json"), 128),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
    );
    let mut log = if let Ok(log) = load(&directory) {
        log
    } else {
        incomplete = true;
        Log::default()
    };
    if let Some(queue) = state.configuration_changes.records() {
        incomplete |= !log.records().is_some_and(|stored| {
            queue.records.iter().all(|row| {
                stored
                    .records
                    .iter()
                    .any(|old| old["change_id"] == row["change_id"])
            })
        });
    }
    if log.merge(&state.configuration_changes).is_err() {
        incomplete = true;
        log = state.configuration_changes.clone();
    }
    let mut view = log.view();
    // A readable bounded log is no proof of an uninterrupted lifetime stream.
    view["complete"] = if incomplete || view["available"] == false {
        json!(false)
    } else {
        Value::Null
    };
    if incomplete {
        view["reason"] = json!(
            "Configuration history may be incomplete; retained summaries and the committed recovery queue are shown."
        );
    }
    view
}

/// Emit a settings summary inside its owning successful checkpoint transaction.
pub(super) fn changed(state: &mut State, context: &Context, setting: &str) {
    let mut event = state.event(
        "configuration_changed",
        context,
        json!({}),
        json!({}),
        json!({"setting":setting,"policy_revision":state.policy_revision}),
    );
    event["operator"] =
        json!({"mechanism":if context.surface == "cli" {"operator-cli"} else {"host-elicitation"}});
    state.record(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_summaries_are_independent_bounded_and_non_authorizing() {
        let (root, context) = crate::review::tests::enrolled();
        let store = crate::review::storage::Store::open(root.path()).unwrap();
        store
            .update_checkpoint(&context, &|| Ok(()), |state| {
                state.history_enabled = false;
                changed(state, &context, "first setting");
                Ok(json!({}))
            })
            .unwrap();
        let state = store.read_checkpoint().unwrap();
        assert!(state.events.is_empty() && state.pending_history.is_empty());
        let view = inspect(&state);
        let rows = view["configuration_changes"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["affected"]["setting"], "first setting");
        assert!(
            rows.iter()
                .all(|row| crate::json::fits(row, BYTES) && row["authority"] == "none")
        );
        super::super::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
        let after = store.read_checkpoint().unwrap();
        let view = inspect(&after);
        assert!(
            view["configuration_changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["change_id"] == rows[0]["change_id"])
        );
        // A crash before export still exposes the committed queue without any
        // write from inspection or dependence on optional request history.
        std::fs::remove_file(directory(&after).join("summaries.json")).unwrap();
        assert!(
            !inspect(&after)["configuration_changes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn configuration_summary_retention_keeps_newest_thousand_without_live_authority() {
        let mut log = Log::Known(Records { schema_version: 1,
            records: (0..1000).map(|index| json!({"schema_version":1,"change_id":format!("event-{index}"),"authority":"none"})).collect() });
        for index in 1000..1002 {
            log.record(
                &json!({"kind":"runtime_activated","event_id":format!("event-{index}"),
                "details":{"policy_revision":"acme"}}),
                Some("before"),
            );
        }
        let view = log.view();
        let rows = view["configuration_changes"].as_array().unwrap();
        assert_eq!(rows.len(), RECORDS);
        assert_eq!(rows[0]["change_id"], "event-1001");
        assert_eq!(rows[999]["change_id"], "event-2");
        assert!(crate::json::fits(&log, STORE_BYTES));
        log.trim_queue();
        assert_eq!(log.records().unwrap().records.len(), QUEUE);
    }
    #[test]
    fn unsupported_configuration_summary_metadata_is_preserved_and_not_authority() {
        let value = json!({"future":"unknown format"});
        let mut log: Log = serde_json::from_value(value.clone()).unwrap();
        log.record(&json!({"kind":"runtime_activated"}), None);
        assert_eq!(log.view()["available"], false);
        assert_eq!(serde_json::to_value(log).unwrap(), value);
    }
    #[test]
    fn delayed_exports_cannot_displace_newer_configuration_changes() {
        let rows = |start: usize, end: usize| {
            Log::Known(Records {
                schema_version: 1,
                records: (start..end)
                    .map(|index| {
                        json!({"schema_version":1,
                "change_id":format!("event-acme-{index}"),"authority":"none"})
                    })
                    .collect(),
            })
        };
        let mut log = rows(1000, 2000);
        log.merge(&rows(0, 16)).unwrap();
        assert_eq!(log.records().unwrap().records.len(), RECORDS);
        let view = log.view();
        assert_eq!(
            view["configuration_changes"][0]["change_id"],
            "event-acme-1999"
        );
        assert_eq!(
            view["configuration_changes"][999]["change_id"],
            "event-acme-1000"
        );
        log.merge(&rows(1990, 2006)).unwrap();
        assert_eq!(
            log.view()["configuration_changes"][0]["change_id"],
            "event-acme-2005"
        );
        assert_eq!(
            log.view()["configuration_changes"][999]["change_id"],
            "event-acme-1006"
        );
    }
    #[test]
    fn failed_summary_export_preserves_committed_policy_and_reports_incomplete_history() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let store = crate::review::storage::Store::open(root.path()).unwrap();
        let state = store.read_checkpoint().unwrap();
        let log = directory(&state).join("summaries.json");
        std::fs::write(&log, b"preserve unsupported summaries").unwrap();
        store
            .update_checkpoint(&context, &|| Ok(()), |state| {
                changed(state, &context, "protected file access");
                Ok(json!({"committed":true}))
            })
            .unwrap();
        let state = store.read_checkpoint().unwrap();
        let checkpoint = root.path().join(".shield/review/state.json");
        let before = std::fs::read(&checkpoint).unwrap();
        let view = inspect(&state);
        assert_eq!(view["available"], true);
        assert_eq!(view["complete"], false);
        assert!(
            view["configuration_changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["affected"]["setting"] == "protected file access")
        );
        assert_eq!(
            std::fs::read(&log).unwrap(),
            b"preserve unsupported summaries"
        );
        assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
        crate::runtime::Project::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap();
    }
}
