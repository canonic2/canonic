//! Nonwaiting notification receipts; serialization precedes delivery accounting.
use super::{Context, DAY_MS, cache};
use crate::review::{Error, storage::objects::Directory};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::{collections::BTreeMap, path::PathBuf};

const RECEIPT_BYTES: usize = 32 * 1024;
const WEEK_MS: u64 = 7 * DAY_MS;
pub(crate) const CLI_NEXT_STEP: &str = "Run shield pack update to review compatible updates.";
const INSTRUCTION: &str = "Mention available pack releases briefly once after the current task. Updating is optional. Do not prepare or apply an update unless the user requested it. Availability does not establish compatibility or permission changes.";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    last_delivery: u64,
    tuples: BTreeMap<String, u64>,
    conversations: BTreeMap<String, u64>,
}

/// The held kernel lock prevents concurrent senders from offering the same
/// project/audience. Dropping without commit records no delivery.
pub(crate) struct Pending {
    _lock: Directory,
    path: PathBuf,
    staged: PathBuf,
    pub(crate) value: Value,
}
impl Pending {
    pub(crate) fn commit(self) -> bool {
        if crate::review::storage::replace(&self.staged, &self.path).is_err() {
            return false;
        }
        #[cfg(unix)]
        if let Some(parent) = self.path.parent() {
            let _ =
                std::fs::File::open(parent).and_then(|file| crate::review::storage::sync(&file));
        }
        true
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.staged);
    }
}

pub(crate) fn pending(context: &Context, audience: &str, updates: &Value) -> Option<Pending> {
    if !super::enabled().ok()? {
        return None;
    }
    pending_at(
        context,
        audience,
        updates,
        crate::review::clock::best_effort_ms(),
    )
}

fn pending_at(context: &Context, audience: &str, updates: &Value, now: u64) -> Option<Pending> {
    if now == 0
        || !updates["items"]
            .as_array()?
            .iter()
            .any(|row| row["freshness"] == "fresh")
    {
        return None;
    }
    let directory = cache::directory(context, "receipts", true).ok()?;
    let lock = Directory::try_enter(&directory).ok()?;
    let path = cache::receipt_path(&directory, &context.project_id, audience);
    let mut receipt = load_receipt(&path)?;
    if receipt.last_delivery > now
        || (receipt.last_delivery != 0 && now - receipt.last_delivery < DAY_MS)
    {
        return None;
    }
    let conversation = if audience == "mcp" {
        let id = context
            .session_id
            .as_ref()
            .or(context.connection_id.as_ref())?;
        Some(crate::policy::fingerprint(id.as_bytes()))
    } else {
        None
    };
    if conversation
        .as_ref()
        .is_some_and(|id| receipt.conversations.contains_key(id))
    {
        return None;
    }
    receipt
        .tuples
        .retain(|_, seen| *seen <= now && now - *seen < WEEK_MS);
    let rows: Vec<_> = updates["items"]
        .as_array()?
        .iter()
        .filter(|row| row["freshness"] == "fresh" && !receipt.tuples.contains_key(&tuple(row)))
        .cloned()
        .collect();
    if rows.is_empty() {
        return None;
    }
    for row in &rows {
        receipt.tuples.insert(tuple(row), now);
    }
    trim(&mut receipt.tuples, 64);
    if let Some(id) = conversation {
        receipt.conversations.insert(id, now);
    }
    trim(&mut receipt.conversations, 64);
    receipt.last_delivery = now;
    cache::room(&directory, &path).ok()?;
    // Establish that private presentation storage is writable before offering
    // anything. Cancellation drops the staged receipt; only delivery commits it.
    let staged = path.with_extension("ready");
    cache::write(&staged, &serde_json::to_value(receipt).ok()?, RECEIPT_BYTES).ok()?;
    Some(Pending {
        _lock: lock,
        path,
        staged,
        value: notice_value(&rows, updates, audience),
    })
}

fn load_receipt(path: &std::path::Path) -> Option<Receipt> {
    match cache::read(path, RECEIPT_BYTES) {
        Ok(value) => {
            let receipt: Receipt = serde_json::from_value(value).ok()?;
            if receipt.schema_version != 1
                || receipt.tuples.len() > 64
                || receipt.conversations.len() > 64
            {
                return None;
            }
            Some(receipt)
        }
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Some(Receipt {
            schema_version: 1,
            ..Receipt::default()
        }),
        Err(_) => None,
    }
}

fn notice_value(rows: &[Value], updates: &Value, audience: &str) -> Value {
    let items: Vec<_> = rows.iter().take(3).cloned().collect();
    let retained_fresh = updates["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["freshness"] == "fresh")
        .count();
    let omitted = updates["fresh_total"]
        .as_u64()
        .unwrap_or(retained_fresh as u64)
        .saturating_sub(retained_fresh as u64);
    let remaining =
        rows.len().saturating_sub(items.len()) + usize::try_from(omitted).unwrap_or(64).min(64);
    let next_step = if audience == "mcp" {
        "When the user requests updates, use pack_update to review compatible updates."
    } else {
        CLI_NEXT_STEP
    };
    let message = format!("{} {next_step}", message(&items, remaining));
    json!({"items":items,"remaining":remaining,"truncated":updates["truncated"],"message":message,"instruction":INSTRUCTION})
}

fn trim(rows: &mut BTreeMap<String, u64>, limit: usize) {
    while rows.len() > limit {
        let Some(key) = rows
            .iter()
            .min_by_key(|(key, time)| (**time, *key))
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        rows.remove(&key);
    }
}

fn tuple(row: &Value) -> String {
    crate::policy::fingerprint(
        json!([
            row["source_identity"],
            row["id"],
            row["installed_version"],
            row["available_version"]
        ])
        .to_string()
        .as_bytes(),
    )
}

pub(super) fn message(rows: &[Value], remaining: usize) -> String {
    let mut text = rows
        .iter()
        .map(|row| {
            format!(
                "{} {} → {}",
                safe(row["reference"].as_str().unwrap_or("pack")),
                safe(row["installed_version"].as_str().unwrap_or("unknown")),
                safe(row["available_version"].as_str().unwrap_or("unknown"))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if remaining > 0 {
        let _ = write!(text, ", and {remaining} more");
    }
    format!("Pack updates available: {text}.")
}

fn safe(value: &str) -> String {
    value
        .chars()
        .take(256)
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect()
}

pub(crate) fn summary(updates: &Value) -> String {
    let rows: Vec<_> = updates["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["freshness"] == "fresh")
        .take(3)
        .cloned()
        .collect();
    let text = if rows.is_empty() {
        match updates["availability"].as_str() {
            Some("none") => "No newer pack releases observed in the checked scope.".to_owned(),
            _ => "Pack update availability is unknown; refresh registry metadata to check."
                .to_owned(),
        }
    } else {
        message(
            &rows,
            usize::try_from(
                updates["fresh_total"]
                    .as_u64()
                    .unwrap_or(rows.len() as u64)
                    .saturating_sub(rows.len() as u64),
            )
            .unwrap_or(64)
            .min(64),
        )
    };
    if updates["availability"] == "partial" {
        format!("{text} Some sources have no fresh metadata.")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_updates_receipts_are_throttled_separate_and_transactional() {
        let root = assert_fs::TempDir::new().unwrap();
        let mut context = Context::new(
            "acme-project".into(),
            root.path().to_str().unwrap().into(),
            "mcp",
        );
        context.connection_id = Some("acme-connection".into());
        let mut updates = json!({"items":[{"source_identity":"acme-source","id":"acme/node","reference":"main/acme/node","installed_version":"1.0.0","available_version":"2.0.0","freshness":"fresh"}],"total":1,"truncated":false});
        let now = DAY_MS * 10;
        let dropped = pending_at(&context, "mcp", &updates, now).unwrap();
        assert!(
            pending_at(&context, "mcp", &updates, now).is_none(),
            "nonwaiting lock"
        );
        drop(dropped);
        assert!(pending_at(&context, "mcp", &updates, now).unwrap().commit());
        assert!(
            pending_at(&context, "mcp", &updates, now + DAY_MS).is_none(),
            "same conversation"
        );
        let cli = pending_at(&context, "cli", &updates, now).unwrap();
        assert!(
            cli.value["message"]
                .as_str()
                .unwrap()
                .contains("Run shield pack update")
        );
        assert!(cli.commit(), "independent audience");
        context.connection_id = Some("acme-reconnect".into());
        assert!(
            pending_at(&context, "mcp", &updates, now + DAY_MS).is_none(),
            "same tuple"
        );
        updates["items"][0]["available_version"] = json!("3.0.0");
        assert!(pending_at(&context, "mcp", &updates, now + DAY_MS - 1).is_none());
        assert!(
            pending_at(&context, "mcp", &updates, now + DAY_MS)
                .unwrap()
                .commit()
        );
        assert!(
            pending_at(&context, "cli", &updates, now - 1).is_none(),
            "clock rollback"
        );
    }
}
