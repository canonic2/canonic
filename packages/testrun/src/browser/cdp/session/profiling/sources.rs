//! Bounded event-time source reads retain document epochs and task ownership.
use super::*;
use std::sync::{Arc, Mutex};

const MAX_SOURCES: usize = 512;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone)]
pub(super) struct Sources(Arc<Owner>);
struct Owner {
    state: Arc<Mutex<State>>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    slots: Arc<tokio::sync::Semaphore>,
}
impl Default for Sources {
    fn default() -> Self {
        Self(Arc::new(Owner {
            state: Arc::default(),
            tasks: Mutex::default(),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }))
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().expect("Source tasks").drain(..) {
            task.abort();
        }
    }
}
#[derive(Default)]
struct State {
    epoch: u64,
    bytes: usize,
    entries: BTreeMap<(bool, String), Entry>,
    loss: bool,
}
#[derive(Clone)]
struct Entry {
    url: String,
    text: Option<String>,
    pending: bool,
    order: usize,
}
impl Sources {
    pub(super) fn len(&self) -> usize {
        self.0.state.lock().expect("Coverage sources").entries.len()
    }
    pub(super) fn order(&self, css: bool) -> Vec<String> {
        let state = self.0.state.lock().expect("Coverage sources");
        let mut entries = state
            .entries
            .iter()
            .filter(|((kind, _), _)| *kind == css)
            .map(|((_, id), entry)| (entry.order, id.clone()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.0);
        entries.into_iter().map(|entry| entry.1).collect()
    }
    pub(super) fn reset(&self) {
        let mut state = self.0.state.lock().expect("Coverage sources");
        state.epoch = state.epoch.saturating_add(1);
        state.entries.clear();
        state.bytes = 0;
    }
    pub(super) fn gap(&self) {
        self.0.state.lock().expect("Coverage sources").loss = true;
    }
    pub(super) fn pending(&self) -> bool {
        self.0
            .state
            .lock()
            .expect("Coverage sources")
            .entries
            .values()
            .any(|entry| entry.pending)
    }
    pub(super) fn complete(&self) -> bool {
        let state = self.0.state.lock().expect("Coverage sources");
        !state.loss && !state.entries.values().any(|entry| entry.pending)
    }
    /// A pending read at publication remains a gap even if its reply arrives
    /// after the immutable report has been projected.
    pub(super) fn snapshot(&self, css: bool) -> BTreeMap<String, (String, Option<String>)> {
        let mut state = self.0.state.lock().expect("Coverage sources");
        if state.entries.values().any(|entry| entry.pending) {
            state.loss = true;
        }
        state
            .entries
            .iter()
            .filter(|((kind, _), _)| *kind == css)
            .map(|((_, id), entry)| (id.clone(), (entry.url.clone(), entry.text.clone())))
            .collect()
    }
    pub(super) fn observe(
        &self,
        connection: &Connection,
        session: &str,
        css: bool,
        id: &str,
        url: &str,
        read: bool,
    ) {
        let key = (css, id.to_owned());
        let epoch = {
            let mut state = self.0.state.lock().expect("Coverage sources");
            if state.entries.contains_key(&key) {
                return;
            }
            if state.entries.len() >= MAX_SOURCES || id.len() > 256 || url.len() > 8192 {
                state.loss = true;
                return;
            }
            let order = state.entries.len();
            state.entries.insert(
                key.clone(),
                Entry {
                    url: url.into(),
                    text: None,
                    pending: read,
                    order,
                },
            );
            state.epoch
        };
        if !read {
            return;
        }
        let mut tasks = self.0.tasks.lock().expect("Source tasks");
        tasks.retain(|task| !task.is_finished());
        if tasks.len() >= 32 {
            let mut state = self.0.state.lock().expect("Coverage sources");
            state.loss = true;
            state.entries.get_mut(&key).expect("Known source").pending = false;
            return;
        }
        let connection = connection.clone();
        let session = session.to_owned();
        let state = self.0.state.clone();
        let slots = self.0.slots.clone();
        tasks.push(tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(2);
            let result = async {
                let _slot = tokio::time::timeout_at(deadline, slots.acquire())
                    .await
                    .ok()?
                    .ok()?;
                if state.lock().expect("Coverage sources").epoch != epoch {
                    return None;
                }
                let mut reply = connection
                    .call(
                        Some(&session),
                        if css {
                            "CSS.getStyleSheetText"
                        } else {
                            "Debugger.getScriptSource"
                        },
                        if css {
                            json!({"styleSheetId":key.1})
                        } else {
                            json!({"scriptId":key.1})
                        },
                        Lane::Ordinary,
                        deadline,
                    )
                    .await
                    .ok()?;
                match reply
                    .as_object_mut()?
                    .remove(if css { "text" } else { "scriptSource" })?
                {
                    Value::String(text) => Some(text),
                    _ => None,
                }
            }
            .await;
            let mut state = state.lock().expect("Coverage sources");
            if state.epoch != epoch {
                return;
            }
            let text = result.and_then(|text| {
                let bytes = serde_json::to_vec(&text).ok()?.len();
                (bytes <= MAX_TEXT_BYTES.saturating_sub(state.bytes)).then_some((text, bytes))
            });
            let text = if let Some((text, bytes)) = text {
                state.bytes += bytes;
                Some(text)
            } else {
                state.loss = true;
                None
            };
            if let Some(entry) = state.entries.get_mut(&key) {
                entry.text = text;
                entry.pending = false;
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_late_source_cannot_turn_an_incomplete_materialized_report_complete() {
        let sources = Sources::default();
        let key = (false, "Acme".to_owned());
        sources.0.state.lock().unwrap().entries.insert(
            key.clone(),
            Entry {
                url: "https://example.com/acme.js".into(),
                text: None,
                pending: true,
                order: 0,
            },
        );
        assert!(sources.snapshot(false)["Acme"].1.is_none());
        {
            let mut state = sources.0.state.lock().unwrap();
            let entry = state.entries.get_mut(&key).unwrap();
            entry.pending = false;
            entry.text = Some("Acme late source".into());
        }
        assert!(!sources.complete());
    }
}
