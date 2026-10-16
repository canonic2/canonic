//! Shared transaction inputs, index records and filesystem observations; no I/O.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stamp(pub(crate) Option<(u128, u64, bool)>);

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Directory {
    pub(crate) stamp: Option<Stamp>,
    pub(crate) children: Vec<(String, bool)>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    pub(crate) file: String,
    pub(crate) watched: Vec<(String, Stamp)>,
    /// JSON entry, validated and projected, or the compiler's error message.
    pub(crate) entry: Result<String, String>,
    pub(crate) docs: bool,
}

/// Inputs for one page-index transaction.
#[derive(Deserialize)]
pub(crate) struct Request {
    pub(crate) root: PathBuf,
    pub(crate) saved: PathBuf,
    pub(crate) identity: String,
    pub(crate) evaluator: PathBuf,
    pub(crate) node: PathBuf,
    #[serde(default)]
    pub(crate) options: serde_json::Value,
    #[serde(default)]
    pub(crate) walk: bool,
    #[serde(default)]
    pub(crate) cached_generation: String,
}
