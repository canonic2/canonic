//! Per-transaction phase diagnostics; no index policy or external I/O.
use serde_json::{Value, json};
use std::time::Instant;
pub(super) struct Timings {
    started: Instant,
    values: Value,
}
impl Timings {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            values: json!({}),
        }
    }
    pub(super) fn elapsed(&mut self, key: &str, start: Instant) {
        self.values[key] = json!(start.elapsed().as_secs_f64() * 1000.0);
    }
    pub(super) fn count(&mut self, key: &str, value: usize) {
        self.values[key] = json!(value);
    }
    pub(super) fn save_error(&mut self, error: &std::io::Error) {
        self.values["saveError"] = json!(error.to_string());
    }
    pub(super) fn counts(
        &mut self,
        definitions: usize,
        reused: usize,
        parsed: usize,
        evaluated: usize,
    ) {
        self.count("definitions", definitions);
        self.count("reused", reused);
        self.count("parsed", parsed);
        self.count("evaluated", evaluated);
    }
    pub(super) fn unchanged(&mut self, definitions: usize, reused: usize) {
        self.counts(definitions, reused, 0, 0);
        for key in ["parseMs", "evaluateMs", "saveMs"] {
            self.values[key] = json!(0);
        }
    }
    pub(super) fn into_json(self) -> Value {
        self.values
    }
    pub(super) fn finish(mut self) -> Value {
        self.elapsed("totalMs", self.started);
        self.values
    }
}
