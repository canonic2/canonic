//! Explicit resource evidence boundary; unavailable facts never prove containment.
use super::{Error, patterns::Provider};
use serde_json::Value;

pub(crate) trait Resources {
    /// Check one explicitly parsed argument path, without claiming a complete
    /// runtime filesystem footprint for the executable.
    fn argument_path(
        &self,
        _path: &str,
        _predicate: &Value,
        _recursive: bool,
        _patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        Ok(Some("evidence_unavailable"))
    }
    fn executable(&self, _reference: &Value) -> Option<&'static str> {
        Some("evidence_unavailable")
    }
    fn cwd(
        &self,
        _predicate: &Value,
        _patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        Ok(Some("evidence_unavailable"))
    }
    fn predicate(
        &self,
        _key: &str,
        _predicate: &Value,
        _patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        Ok(Some("evidence_unavailable"))
    }
    fn coverage(&self, _predicates: &[&Value]) -> bool {
        false
    }
    fn ensure_budget(&self) -> Result<(), Error> {
        Ok(())
    }
}

// Pure source/argv evaluation cannot manufacture mutable filesystem observations.
pub(super) struct Unavailable;
impl Resources for Unavailable {}
