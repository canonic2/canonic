//! Versioned private analysis models. Missing identity and clock facts stay unknown.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_RAW: usize = 8 * 1024 * 1024;
pub const MAX_INDEX: usize = 16 * 1024 * 1024;
pub const MAX_ROWS: usize = 20000;
/// The parser that writes derived indexes, `testrun-diagnostics/<generation>`.
/// Change both together when decoding changes what an index holds: indexes of
/// earlier generations are then rebuilt from their raw evidence when read.
pub const PARSER: &str = "testrun-diagnostics/1";
pub const PARSER_GENERATION: u64 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Metadata {
    pub clocks: Vec<Clock>,
    pub symbols: Vec<Symbol>,
    pub sampling: Option<String>,
    pub heap_id: Option<String>,
    pub observation_complete: bool,
    pub source_loss: Vec<String>,
    #[serde(skip_serializing_if = "disabled")]
    pub symbolicate: bool,
}
fn disabled(value: &bool) -> bool {
    !*value
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Clock {
    pub id: String,
    pub unit: String,
    pub reference_domain: Option<String>,
    pub max_drift_ppm: Option<f64>,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Anchor {
    pub source: f64,
    pub reference: f64,
    pub uncertainty: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Symbol {
    pub kind: String,
    pub evidence_id: String,
    pub sha256: String,
    pub build_id: String,
    pub architecture: String,
}
impl Metadata {
    pub fn validate(&self) -> Result<()> {
        if self.symbolicate
            && (self.symbols.is_empty()
                || self.symbols.len() > 8
                || self.symbols.iter().any(|s| {
                    !matches!(s.kind.as_str(), "dsym" | "mach-o")
                        || crate::diagnostics::apple::reports::uuid(&s.build_id).is_none()
                        || !crate::diagnostics::apple::reports::architecture(&s.architecture)
                }))
        {
            return Err(Error::invalid(
                "Explicit Apple symbolication requires 1..8 retained dsym/mach-o artifacts with UUID and supported architecture",
            ));
        }
        if self.clocks.len() > 32 || self.symbols.len() > 32 || self.source_loss.len() > 32 {
            return Err(Error::invalid(
                "At most 32 clocks, symbol references and loss reasons",
            ));
        }
        for text in self
            .source_loss
            .iter()
            .chain(self.sampling.iter())
            .chain(self.heap_id.iter())
        {
            super::super::model::label(text, 256)?;
        }
        for (i, clock) in self.clocks.iter().enumerate() {
            super::super::model::label(&clock.id, 256)?;
            if !matches!(
                clock.unit.as_str(),
                "seconds" | "milliseconds" | "microseconds" | "nanoseconds"
            ) || self.clocks[..i].iter().any(|c| c.id == clock.id)
                || clock.anchors.len() > 32
                || clock
                    .max_drift_ppm
                    .is_some_and(|n| !n.is_finite() || !(0.0..=1000000.0).contains(&n))
                || (!clock.anchors.is_empty()
                    && (clock.reference_domain.is_none() || clock.max_drift_ppm.is_none()))
                || clock.anchors.iter().any(|a| {
                    !a.source.is_finite()
                        || !a.reference.is_finite()
                        || !a.uncertainty.is_finite()
                        || a.uncertainty < 0.0
                })
            {
                return Err(Error::invalid(
                    "Clock anchors require a unique domain, time unit, referenceDomain and maxDriftPpm; uncertainty uses the source unit",
                ));
            }
            if let Some(reference) = &clock.reference_domain {
                super::super::model::label(reference, 256)?;
            }
        }
        for symbol in &self.symbols {
            if !matches!(
                symbol.kind.as_str(),
                "source-map" | "dsym" | "elf" | "mach-o" | "pdb" | "r8" | "proguard" | "dart"
            ) || symbol.sha256.len() != 64
                || !symbol.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::invalid(
                    "Use a supported symbol kind and an exact SHA-256",
                ));
            }
            for text in [&symbol.evidence_id, &symbol.build_id, &symbol.architecture] {
                super::super::model::label(text, 256)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Row {
    pub kind: String,
    pub name: String,
    pub timestamp: Option<f64>,
    pub clock_domain: Option<String>,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub definition: Option<String>,
    #[serde(default)]
    pub correlations: Vec<String>,
    #[serde(default)]
    pub source: String,
    pub data: Value,
}
impl Row {
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.kind.as_str(),
            "event"
                | "failure"
                | "metric"
                | "stack"
                | "thread"
                | "allocation"
                | "retention"
                | "span"
                | "frame"
                | "action"
                | "marker"
                | "process"
                | "screenshot"
                | "video"
                | "audio"
        ) || self.timestamp.is_some_and(|n| !n.is_finite())
            || self.value.is_some_and(|n| !n.is_finite())
            || self.timestamp.is_some() != self.clock_domain.is_some()
            || self.value.is_some() && (self.unit.is_none() || self.definition.is_none())
            || self.correlations.len() > 32
        {
            return Err(Error::invalid(
                "Invalid diagnostic row kind, timestamp/domain or measurement definition",
            ));
        }
        for value in [&self.name, &self.source]
            .into_iter()
            .chain(self.clock_domain.iter())
            .chain(self.unit.iter())
            .chain(self.definition.iter())
            .chain(self.correlations.iter())
        {
            super::super::model::label(value, 1024)?;
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Index {
    #[serde(skip)]
    pub model_bytes: usize,
    pub version: u32,
    pub parser: String,
    pub format: String,
    pub clocks: Vec<Clock>,
    pub rows: Vec<Row>,
    pub unsupported: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Receipt {
    pub version: u32,
    pub id: String,
    pub collector_id: String,
    pub signature: String,
    pub owner: String,
    pub phase: String,
    pub raw_sha256: String,
    pub raw_bytes: u64,
    pub format: String,
    pub capture_phase: String,
    pub capture_reason: Option<String>,
    pub binding: Value,
    pub metadata: Metadata,
    pub index_sha256: Option<String>,
    #[serde(default)]
    pub unsupported: Vec<String>,
    pub reason: Option<String>,
    #[serde(default)]
    pub cost: Option<AnalysisCost>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AnalysisCost {
    pub run_wall_ms: f64,
    pub raw_bytes: u64,
    pub index_bytes: u64,
}
pub fn scale(unit: &str) -> Option<f64> {
    match unit {
        "seconds" => Some(1.0),
        "milliseconds" => Some(1e-3),
        "microseconds" => Some(1e-6),
        "nanoseconds" => Some(1e-9),
        _ => None,
    }
}
