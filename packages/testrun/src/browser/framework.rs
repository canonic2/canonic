//! Document-bound React registries with explicit hook preparation.
use super::{contract::Reference, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
pub(crate) const HOOK: &str = include_str!("reading/react-hook.js");
pub(crate) const HELPER: &str = include_str!("reading/framework.js");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Tree,
    Inspect,
    Suspense,
    RendersStart,
    RendersStatus,
    RendersStop,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub kind: Kind,
    #[serde(default)]
    pub renderer_id: Option<u32>,
    #[serde(default)]
    pub id: Option<u32>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "limit")]
    pub limit: usize,
    #[serde(default)]
    pub only_dynamic: bool,
    #[serde(default = "duration")]
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_change_descriptions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_timeline: Option<bool>,
}
fn limit() -> usize {
    20
}
fn duration() -> u64 {
    30000
}
impl Query {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.offset > 2000
            || !(1..=100).contains(&self.limit)
            || !(1..=60000).contains(&self.duration_ms)
            || matches!(self.kind, Kind::Inspect) && self.id.is_none()
            || (self.record_change_descriptions.is_some() || self.record_timeline.is_some())
                && !matches!(self.kind, Kind::RendersStart)
        {
            Err("FRAMEWORK_QUERY_INVALID")
        } else {
            Ok(())
        }
    }
}
#[derive(Default)]
pub(crate) struct Registries {
    items: BTreeSet<Reference>,
    closed: VecDeque<Reference>,
    allocating: usize,
}
impl Registries {
    pub(crate) fn reserve(&mut self) -> Result<(), Error> {
        if self.items.len() + self.allocating >= 4 {
            return Err("FRAMEWORK_CAPACITY".into());
        }
        self.allocating += 1;
        Ok(())
    }
    pub(crate) fn failed(&mut self) {
        self.allocating = self.allocating.saturating_sub(1);
    }
    pub(crate) fn insert(&mut self, reference: Reference) {
        self.failed();
        self.items.insert(reference);
    }
    pub(crate) fn references(&self) -> Vec<Reference> {
        self.items.iter().copied().collect()
    }
    pub(crate) fn require(&self, reference: Reference) -> Result<(), Error> {
        if self.items.contains(&reference) {
            Ok(())
        } else {
            Err("UNKNOWN_FRAMEWORK".into())
        }
    }
    pub(crate) fn closed(&self, reference: Reference) -> bool {
        self.closed.contains(&reference)
    }
    pub(crate) fn forget(&mut self, reference: Reference) {
        self.items.remove(&reference);
        if self.closed.len() == 128 {
            self.closed.pop_front();
        }
        self.closed.push_back(reference);
    }
}
pub(crate) fn result(value: Value) -> Result<Value, Error> {
    let text = value["value"]["value"]
        .as_str()
        .ok_or("FRAMEWORK_PROTOCOL")?;
    if text.len() > 65536 {
        return Err("FRAMEWORK_RESULT_BOUNDS".into());
    }
    let report: Value =
        serde_json::from_str(text).map_err(|_| Error::from("FRAMEWORK_PROTOCOL"))?;
    Ok(
        json!({"report":report,"untrustedApplicationContent":true,"potentiallyMutating":true,"dispatchCompleted":true}),
    )
}
pub(crate) fn retained(mut error: Error, registry: Reference) -> Error {
    let cause = error.partial.take();
    error.partial = Some(
        json!({"registry":registry,"cause":cause,"potentiallyMutating":true,"nextAction":"framework_release"}),
    );
    error
}
pub(crate) fn capabilities(available: bool) -> Value {
    json!({"available":available,"framework":"React","prepareBeforeStartup":true,"prepareScopes":["page","context"],"documentBound":true,"maxRegistries":4,"maxRows":2000,"maxResultBytes":65536,"maxProfilingMs":60000,"profilingOptions":["recordChangeDescriptions","recordTimeline"],"profilingDefaults":{"recordChangeDescriptions":true,"recordTimeline":false},"source":"pinned-React-DevTools-renderer-interfaces"})
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let mut schemas = vec![
        json!({"type":"object","properties":{"operation":{"const":"framework_prepare"},"page":reference},"required":["operation","page"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_framework_prepare"},"context":reference},"required":["operation","context"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"framework_open"},"scope":reference},"required":["operation","scope"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"framework_query"},"registry":reference,"query":{"type":"object","properties":{"kind":{"enum":["tree","inspect","suspense","renders_start","renders_status","renders_stop"]},"rendererId":{"type":["integer","null"],"minimum":0},"id":{"type":["integer","null"],"minimum":0},"offset":{"type":"integer","minimum":0,"maximum":2000,"default":0},"limit":{"type":"integer","minimum":1,"maximum":100,"default":20},"onlyDynamic":{"type":"boolean","default":false},"durationMs":{"type":"integer","minimum":1,"maximum":60000,"default":30000}},"required":["kind"],"additionalProperties":false}},"required":["operation","registry","query"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"framework_release"},"registry":reference},"required":["operation","registry"],"additionalProperties":false}),
    ];
    schemas[3]["properties"]["query"]["properties"]["recordChangeDescriptions"] = json!({"type":"boolean","default":true,"description":"renders_start only: record per-commit component change descriptions using the renderer profiler"});
    schemas[3]["properties"]["query"]["properties"]["recordTimeline"] = json!({"type":"boolean","default":false,"description":"renders_start only: request native React timeline data when the renderer/build supports it"});
    schemas
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framework_profile_options_preserve_defaults_and_explicit_booleans() {
        let query: Query = serde_json::from_value(json!({"kind":"renders_start"})).unwrap();
        assert!(query.validate().is_ok());
        let serialized = serde_json::to_value(query).unwrap();
        assert!(serialized.get("recordChangeDescriptions").is_none());
        assert!(serialized.get("recordTimeline").is_none());
        for changes in [false, true] {
            for timeline in [false, true] {
                let input = json!({"kind":"renders_start","recordChangeDescriptions":changes,"recordTimeline":timeline});
                let query: Query = serde_json::from_value(input).unwrap();
                assert!(query.validate().is_ok());
                assert_eq!(query.record_change_descriptions, Some(changes));
                assert_eq!(query.record_timeline, Some(timeline));
                let reference = Reference {
                    connection_id: uuid::Uuid::new_v4(),
                    id: uuid::Uuid::new_v4(),
                    generation: uuid::Uuid::new_v4(),
                };
                let request =
                    json!({"operation":"framework_query","registry":reference,"query":query});
                crate::schema::validate(&crate::browser::contract::schemas()["request"], &request)
                    .unwrap();
            }
        }
        for kind in [
            "tree",
            "inspect",
            "suspense",
            "renders_status",
            "renders_stop",
        ] {
            for option in ["recordChangeDescriptions", "recordTimeline"] {
                let mut input = json!({"kind":kind,"id":1});
                input[option] = json!(false);
                let query: Query = serde_json::from_value(input).unwrap();
                assert_eq!(query.validate(), Err("FRAMEWORK_QUERY_INVALID"));
            }
        }
    }
}
