//! Typed queries over an owned, complete Chrome trace.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    Summary {},
    Vitals {
        #[serde(rename = "insightSetId")]
        insight_set_id: String,
    },
    Insights {
        #[serde(rename = "insightSetId")]
        insight_set_id: String,
        #[serde(default)]
        insight: Option<String>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
    Timings {
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
}
fn limit() -> usize {
    50
}
impl Query {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Vitals { insight_set_id } | Self::Insights { insight_set_id, .. }
                if insight_set_id.is_empty() || insight_set_id.len() > 256 =>
            {
                return Err("PERFORMANCE_QUERY_BOUNDS");
            }
            Self::Insights {
                insight: Some(name),
                ..
            } if name.is_empty() || name.len() > 128 => return Err("PERFORMANCE_QUERY_BOUNDS"),
            _ => {}
        }
        match self {
            Self::Insights { offset, limit, .. } | Self::Timings { offset, limit } => {
                if *offset > 100000 || !(1..=100).contains(limit) {
                    Err("PERFORMANCE_QUERY_BOUNDS")
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let id = json!({"type":"string","format":"uuid"});
    let pagination = json!({"offset":{"type":"integer","minimum":0,"maximum":100000,"default":0},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50}});
    let mut queries = Vec::new();
    for kind in ["summary", "vitals", "insights", "timings"] {
        let mut properties = json!({"kind":{"const":kind}});
        let mut required = vec!["kind"];
        if matches!(kind, "vitals" | "insights") {
            properties["insightSetId"] = json!({"type":"string","minLength":1,"maxLength":256});
            required.push("insightSetId");
        }
        if matches!(kind, "insights" | "timings") {
            properties
                .as_object_mut()
                .unwrap()
                .extend(pagination.as_object().unwrap().clone());
        }
        if kind == "insights" {
            properties["insight"] = json!({"type":["string","null"],"minLength":1,"maxLength":128});
        }
        queries.push(json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}));
    }
    vec![
        json!({"type":"object","properties":{"operation":{"const":"performance_load"},"developer":reference,"evidenceId":id},"required":["operation","developer","evidenceId"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"performance_query"},"developer":reference,"analysisId":id,"query":{"oneOf":queries}},"required":["operation","developer","analysisId","query"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"performance_release"},"developer":reference,"analysisId":id},"required":["operation","developer","analysisId"],"additionalProperties":false}),
    ]
}
