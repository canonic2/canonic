//! Native JSON response projection; index models never depend on this module.
use crate::model::Record;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize)]
pub(crate) struct Response {
    records: Vec<Listed>,
    timings: Value,
    needs_evaluator: bool,
    generation: String,
    reuse_answer: bool,
}
#[derive(Serialize)]
pub(crate) struct Listed {
    file: String,
    docs: bool,
    entry: Result<String, String>,
}
impl Response {
    pub(crate) fn waiting(timings: Value) -> Self {
        Self {
            records: vec![],
            timings,
            needs_evaluator: true,
            generation: String::new(),
            reuse_answer: false,
        }
    }
    pub(crate) fn reused(generation: String, timings: Value) -> Self {
        Self {
            records: vec![],
            timings,
            needs_evaluator: false,
            generation,
            reuse_answer: true,
        }
    }
    /// The listed records of a complete answer, as one comparable string.
    pub(crate) fn fingerprint(&self) -> Option<String> {
        (!self.needs_evaluator && !self.reuse_answer)
            .then(|| serde_json::to_string(&self.records).ok())
            .flatten()
    }

    /// Whether the evaluator must be supplied before this request can be answered.
    pub(crate) fn needs_evaluator(&self) -> bool {
        self.needs_evaluator
    }

    /// The answer's generation, when it is complete or reused rather than waiting for an evaluator.
    pub(crate) fn generation(&self) -> Option<&str> {
        (!self.needs_evaluator && !self.generation.is_empty()).then_some(self.generation.as_str())
    }
    pub(crate) fn complete(records: Vec<Listed>, generation: String, timings: Value) -> Self {
        Self {
            records,
            timings,
            needs_evaluator: false,
            generation,
            reuse_answer: false,
        }
    }
}
pub(crate) fn listed(files: &[String], records: &HashMap<String, Record>) -> Result<Vec<Listed>> {
    files
        .iter()
        .map(|file| {
            records
                .get(file)
                .map(|record| Listed {
                    file: record.file.clone(),
                    docs: record.docs,
                    entry: record.entry.clone(),
                })
                .context("evaluator omitted a definition")
        })
        .collect()
}
