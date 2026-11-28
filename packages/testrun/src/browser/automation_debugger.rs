//! Context automation-call controls, distinct from JavaScript engine debugging.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}
impl Location {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.file.is_empty()
            || self.file.len() > 1024
            || self.file.chars().any(char::is_control)
            || self.line.is_some_and(|value| value > i32::MAX as u32)
            || self.column.is_some_and(|value| value > i32::MAX as u32)
        {
            return Err("CALL_SOURCE_BOUNDS");
        }
        Ok(())
    }
}
pub(crate) fn location_schema() -> Value {
    json!({"type":"object","properties":{"file":{"type":"string","minLength":1,"maxLength":1024},"line":{"type":"integer","minimum":0,"maximum":i32::MAX},"column":{"type":"integer","minimum":0,"maximum":i32::MAX}},"required":["file"],"additionalProperties":false})
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Enable,
    Calls {
        #[serde(default)]
        offset: u16,
        #[serde(default = "call_limit")]
        limit: u16,
        #[serde(default, rename = "includeDetails")]
        include_details: bool,
    },
    Status {
        #[serde(default, rename = "includeDetails")]
        include_details: bool,
    },
    Pause,
    Resume {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
    },
    Next {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
    },
    RunTo {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
        location: Location,
    },
}
fn call_limit() -> u16 {
    16
}
impl Action {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::RunTo { location, .. } = self {
            location.validate()?;
        }
        if let Self::Calls { offset, limit, .. } = self
            && (*offset > 32 || !(1..=32).contains(limit))
        {
            return Err("AUTOMATION_CALL_BOUNDS");
        }
        Ok(())
    }
    pub(crate) fn fenced(&self) -> bool {
        matches!(
            self,
            Self::Enable | Self::Pause | Self::Next { .. } | Self::RunTo { .. }
        )
    }
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let action = |kind: &str, extra: Value, required: &[&str]| {
        let mut properties = json!({"kind":{"const":kind}});
        properties
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut fields = vec!["kind"];
        fields.extend(required.iter().copied());
        json!({"type":"object","properties":properties,"required":fields,"additionalProperties":false})
    };
    vec![
        json!({"type":"object","properties":{"operation":{"const":"context_debugger"},"context":reference,"action":{"oneOf":[
        action("enable",json!({}),&[]),
        action("calls",json!({"offset":{"type":"integer","minimum":0,"maximum":32,"default":0},"limit":{"type":"integer","minimum":1,"maximum":32,"default":16},"includeDetails":{"type":"boolean","default":false}}),&[]),
        action("status",json!({"includeDetails":{"type":"boolean","default":false,"description":"Include bounded private automation title/source metadata, not a JavaScript stack"}}),&[]),
        action("pause",json!({}),&[]),
        action("resume",json!({"pauseId":{"type":"string","format":"uuid"}}),&["pauseId"]),
        action("next",json!({"pauseId":{"type":"string","format":"uuid"}}),&["pauseId"]),
        action("run_to",json!({"pauseId":{"type":"string","format":"uuid"},"location":location_schema()}),&["pauseId","location"])
    ]}},"required":["operation","context","action"],"additionalProperties":false}),
    ]
}
