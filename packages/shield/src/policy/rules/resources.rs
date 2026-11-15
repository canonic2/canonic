//! Explicit resource evidence boundary; unavailable facts never prove containment.
use crate::policy::{Error, rules::patterns::Provider};
use serde_json::Value;

pub(crate) trait Resources {
    /// Check an MCP path in the declared server coordinate system. A caller
    /// without project-relative observations cannot infer that base.
    fn field_path(
        &self,
        path: &str,
        predicate: &Value,
        recursive: bool,
        relative_to: &str,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        if relative_to != "cwd" {
            return Ok(Some("evidence_unavailable"));
        }
        self.argument_path(path, predicate, recursive, patterns)
    }
    /// Bounded facts retained by the resource owner, never raw OS errors.
    fn predicate_evidence(&self, _key: &str) -> Option<Value> {
        None
    }
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
pub(in crate::policy) struct Unavailable;
impl Resources for Unavailable {}
