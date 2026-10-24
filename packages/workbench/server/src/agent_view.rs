//! The page the canvas is showing, for agents in the editor.
//!
//! Editor chats take the open text file as context, never a webview, so each
//! open canvas reports what it shows under its own client id and agents ask
//! the server. The canvas changed most recently is the current view. A canvas
//! that closes says so; one that vanishes stops counting once it has missed
//! its heartbeats. A Canonic project (one with a `.canonic` folder) also gets
//! the server's address in [`ANNOUNCE_FILE`] for hooks and MCP servers.

use crate::js;
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where canvases report and agents read the view.
pub(crate) const VIEW_PATH: &str = "/_workbench/view";
/// The announcement, relative to the project root.
pub(crate) const ANNOUNCE_FILE: &str = ".canonic/.workbench/server.json";
const STALE_MS: f64 = 3.0 * 60.0 * 1000.0;
const MAX_CLIENTS: usize = 32;
const MAX_TEXT: usize = 100_000;
const MAX_ARTBOARDS: usize = 32;
const MAX_CLIENT_ID: usize = 100;

/// Milliseconds since the Unix epoch.
pub(crate) type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

struct Entry {
    view: Value,
    changed_at: f64,
    seen_at: f64,
}

#[derive(Default)]
struct State {
    /// In first-report order, as the canvas's clients first appeared.
    clients: Vec<(String, Entry)>,
    announced: Option<String>,
}

/// Every open canvas's view, and this server's announcement.
pub(crate) struct AgentView {
    root: PathBuf,
    clock: Clock,
    stale_ms: f64,
    state: Mutex<State>,
}

fn system_clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64() * 1000.0)
}

fn some_text(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

fn clean_artboard(entry: &Value, ids: &mut Vec<String>) -> Result<Value> {
    let Some(board) = entry.as_object() else {
        bail!("Invalid artboard context.");
    };
    let id = board.get("id").and_then(Value::as_str);
    let src = board.get("src").and_then(Value::as_str);
    let space = board.get("space").filter(|space| js::truthy(Some(space)));
    let space_id = space
        .and_then(|space| space.get("id"))
        .and_then(Value::as_str);
    let space_name = space
        .and_then(|space| space.get("name"))
        .and_then(Value::as_str);
    let size = board.get("size").filter(|size| js::truthy(Some(size)));
    // The numbers as sent, so whole pixels stay integers in the report.
    let width = size
        .and_then(|size| size.get("width"))
        .filter(|width| width.is_number());
    let height = size
        .and_then(|size| size.get("height"))
        .filter(|height| height.is_number());
    let (Some(id), Some(src), Some(space_id), Some(space_name), Some(width), Some(height)) =
        (id, src, space_id, space_name, width, height)
    else {
        bail!("Invalid artboard context.");
    };
    if ids.iter().any(|seen| seen == id) {
        bail!("Invalid artboard context.");
    }
    ids.push(id.to_owned());
    let string_or_null = |key: &str| {
        board
            .get(key)
            .filter(|value| value.is_string())
            .cloned()
            .unwrap_or(Value::Null)
    };
    let mut out = Map::new();
    out.insert("id".into(), json!(id));
    out.insert(
        "space".into(),
        json!({ "id": space_id, "name": space_name }),
    );
    out.insert("src".into(), json!(src));
    out.insert("scenario".into(), string_or_null("scenario"));
    for key in ["artboard", "iteration"] {
        if let Some(value) = some_text(board.get(key)) {
            out.insert(key.into(), json!(value));
        }
    }
    out.insert("lens".into(), string_or_null("lens"));
    out.insert("size".into(), json!({ "width": width, "height": height }));
    out.insert(
        "status".into(),
        board
            .get("status")
            .filter(|value| value.is_string())
            .cloned()
            .unwrap_or_else(|| json!("loading")),
    );
    Ok(Value::Object(out))
}

/// Only what Copy reference says, and the ids it says it with: `None` for a
/// view with nothing in it, the whole canvas for a version 2 report.
///
/// # Errors
/// Returns why a report is too large or malformed; nothing is truncated.
pub(crate) fn clean_view(value: &Value) -> Result<Option<Value>> {
    let Some(view) = value.as_object() else {
        return Ok(None);
    };
    let Some(text) = view.get("text").and_then(Value::as_str) else {
        return Ok(None);
    };
    if js::blank(text) {
        return Ok(None);
    }
    if js::length(text) > MAX_TEXT {
        bail!("Canvas context exceeds 100000 characters; no artboards were truncated.");
    }
    let mut out = Map::new();
    out.insert("text".into(), json!(text));
    for key in ["src", "scenario", "iteration", "story", "lens"] {
        if let Some(found) = some_text(view.get(key)) {
            out.insert(key.into(), json!(found));
        }
    }
    if view.get("version").and_then(Value::as_f64) == Some(2.0) {
        let canvas = view.get("canvas").and_then(Value::as_str);
        let artboards = view
            .get("artboards")
            .and_then(Value::as_array)
            .filter(|boards| boards.len() <= MAX_ARTBOARDS);
        let (Some(canvas), Some(artboards)) = (canvas, artboards) else {
            bail!("Invalid canvas context.");
        };
        let mut ids = Vec::new();
        let cleaned = artboards
            .iter()
            .map(|entry| clean_artboard(entry, &mut ids))
            .collect::<Result<Vec<_>>>()?;
        let selected = view.get("selected");
        match selected {
            Some(Value::Null) => {}
            Some(Value::String(id)) if ids.contains(id) => {}
            _ => bail!("Invalid selected artboard."),
        }
        out.insert("artboards".into(), Value::Array(cleaned));
        out.insert("version".into(), json!(2));
        out.insert("canvas".into(), json!(canvas));
        out.insert("selected".into(), selected.cloned().unwrap_or(Value::Null));
    }
    Ok(Some(Value::Object(out)))
}

impl AgentView {
    /// The view for the project at `root`, on the system clock, dropping
    /// canvases silent for `stale_ms`.
    pub(crate) fn new(root: &Path, stale_ms: Option<f64>) -> Self {
        Self::with_clock(root, Arc::new(system_clock), stale_ms)
    }

    /// The view on `clock`, dropping canvases silent for `stale_ms`
    /// (three minutes when `None`).
    pub(crate) fn with_clock(root: &Path, clock: Clock, stale_ms: Option<f64>) -> Self {
        Self {
            root: std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf()),
            clock,
            stale_ms: stale_ms.unwrap_or(STALE_MS),
            state: Mutex::default(),
        }
    }

    fn prune(&self, state: &mut State, at: f64) {
        state
            .clients
            .retain(|(_, entry)| at - entry.seen_at <= self.stale_ms);
    }

    /// One canvas's report: its client id, and its view (null while a page is
    /// loading or nothing is picked), or `closed`.
    ///
    /// # Errors
    /// Returns why the report is refused: no client id, too many canvases, or
    /// a malformed view.
    pub(crate) fn report(&self, body: &Value) -> Result<()> {
        let Some(client) = body
            .get("client")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && js::length(id) <= MAX_CLIENT_ID)
        else {
            bail!("A canvas report needs its client id.");
        };
        let at = (self.clock)();
        let mut state = self.state.lock().expect("agent view lock");
        self.prune(&mut state, at);
        if js::truthy(body.get("closed")) {
            state.clients.retain(|(id, _)| id != client);
            return Ok(());
        }
        let view = clean_view(body.get("view").unwrap_or(&Value::Null))?.unwrap_or(Value::Null);
        let previous = state.clients.iter().position(|(id, _)| id == client);
        if previous.is_none() && state.clients.len() >= MAX_CLIENTS {
            bail!("Too many open canvases.");
        }
        let activity = body
            .get("activity")
            .and_then(Value::as_f64)
            .map(|activity| activity.min(at));
        match previous {
            Some(index) => {
                let entry = &mut state.clients[index].1;
                let changed = entry.view != view;
                entry.changed_at = activity.unwrap_or(if changed { at } else { entry.changed_at });
                entry.view = view;
                entry.seen_at = at;
            }
            None => state.clients.push((
                client.to_owned(),
                Entry {
                    view,
                    changed_at: activity.unwrap_or(at),
                    seen_at: at,
                },
            )),
        }
        Ok(())
    }

    /// The view changed most recently, with `root` so a reader that found this
    /// address in a leftover file can check it still serves its project.
    pub(crate) fn current(&self) -> Value {
        let at = (self.clock)();
        let mut state = self.state.lock().expect("agent view lock");
        self.prune(&mut state, at);
        let root = self.root.to_string_lossy();
        let mut latest: Option<&Entry> = None;
        for (_, entry) in &state.clients {
            if latest.is_none_or(|latest| entry.changed_at >= latest.changed_at) {
                latest = Some(entry);
            }
        }
        match latest {
            None => json!({ "root": root, "open": false, "view": null, "changedAt": null }),
            Some(entry) => json!({
                "root": root, "open": true, "view": entry.view,
                "changedAt": js::iso_time(entry.changed_at),
            }),
        }
    }

    /// Writes this server's address for agents, in a Canonic project only.
    ///
    /// # Errors
    /// Returns why the announcement could not be written.
    pub(crate) fn announce(&self, base_url: &str) -> Result<()> {
        if !self.root.join(".canonic").exists() {
            return Ok(());
        }
        let file = self.root.join(ANNOUNCE_FILE);
        let content =
            serde_json::to_string_pretty(&json!({ "url": base_url, "pid": std::process::id() }))?
                + "\n";
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&file, &content)?;
        self.state.lock().expect("agent view lock").announced = Some(content);
        Ok(())
    }

    /// Removes the announcement, unless another server has announced itself since.
    pub(crate) fn close(&self) {
        let Some(announced) = self.state.lock().expect("agent view lock").announced.take() else {
            return;
        };
        let file = self.root.join(ANNOUNCE_FILE);
        if std::fs::read_to_string(&file).is_ok_and(|content| content == announced) {
            // Already gone is as good as removed.
            let _removed = std::fs::remove_file(&file);
        }
    }
}

#[cfg(test)]
mod tests;
