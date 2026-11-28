//! Typed native debugger controls; pause identities fence stack operations.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Enable {
        #[serde(default)]
        exceptions: Exceptions,
    },
    Status {
        #[serde(default, rename = "includeFrames")]
        include_frames: bool,
    },
    Pause,
    Resume {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
    },
    Step {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
        step: Step,
    },
    Disable,
    BreakpointSet {
        url: String,
        line: u32,
        #[serde(default)]
        column: u32,
        #[serde(default)]
        condition: Option<String>,
    },
    BreakpointRemove {
        #[serde(rename = "breakpointId")]
        breakpoint_id: Uuid,
    },
    Evaluate {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
        #[serde(default)]
        frame: usize,
        expression: String,
    },
    /// Descriptor reads never invoke getters or evaluate an expression.
    Variables {
        #[serde(rename = "pauseId")]
        pause_id: Uuid,
        frame: usize,
        scope: usize,
        #[serde(default)]
        offset: usize,
        limit: usize,
    },
    LogpointSet {
        url: String,
        line: u32,
        #[serde(default)]
        column: u32,
        marker: String,
    },
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exceptions {
    #[default]
    None,
    Uncaught,
    All,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Into,
    Over,
    Out,
}
impl Action {
    pub(crate) fn responsive(&self) -> bool {
        !matches!(self, Self::Enable { .. })
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::BreakpointSet {
                url,
                line,
                column,
                condition,
            } if url.len() > 4096
                || url.is_empty()
                || url.chars().any(char::is_control)
                || *line > 10000000
                || *column > 10000000
                || condition.as_ref().is_some_and(|s| s.len() > 16384) =>
            {
                Err("BREAKPOINT_BOUNDS")
            }
            Self::Evaluate {
                frame, expression, ..
            } if *frame >= 100 || expression.len() > 65536 => Err("EVALUATION_BOUNDS"),
            Self::Variables {
                frame,
                scope,
                offset,
                limit,
                ..
            } if *frame >= 100 || *scope >= 32 || *offset > 10000 || !(1..=100).contains(limit) => {
                Err("VARIABLE_BOUNDS")
            }
            Self::LogpointSet {
                url,
                line,
                column,
                marker,
            } if url.is_empty()
                || url.len() > 4096
                || url.chars().any(char::is_control)
                || *line > 10000000
                || *column > 10000000
                || marker.is_empty()
                || marker.len() > 256
                || marker.chars().any(char::is_control) =>
            {
                Err("LOGPOINT_BOUNDS")
            }
            _ => Ok(()),
        }
    }
}
pub(crate) fn schema(reference: &Value) -> Value {
    let uuid = json!({"type":"string","format":"uuid"});
    let object = |properties: Value, required: Vec<&str>| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    json!({"type":"object","properties":{"operation":{"const":"page_debugger"},"page":reference,"action":{"oneOf":[
        object(json!({"kind":{"const":"enable"},"exceptions":{"enum":["none","uncaught","all"],"default":"none"}}),vec!["kind"]),
        object(json!({"kind":{"const":"status"},"includeFrames":{"type":"boolean","default":false}}),vec!["kind"]),
        object(json!({"kind":{"enum":["pause","disable"]}}),vec!["kind"]),
        object(json!({"kind":{"const":"resume"},"pauseId":uuid}),vec!["kind","pauseId"]),
        object(json!({"kind":{"const":"step"},"pauseId":uuid,"step":{"enum":["into","over","out"]}}),vec!["kind","pauseId","step"]),
        object(json!({"kind":{"const":"breakpoint_set"},"url":{"type":"string","minLength":1,"maxLength":4096},"line":{"type":"integer","minimum":0,"maximum":10000000},"column":{"type":"integer","minimum":0,"maximum":10000000,"default":0},"condition":{"type":["string","null"],"maxLength":16384}}),vec!["kind","url","line"]),
        object(json!({"kind":{"const":"breakpoint_remove"},"breakpointId":uuid}),vec!["kind","breakpointId"]),
        object(json!({"kind":{"const":"variables"},"pauseId":uuid,"frame":{"type":"integer","minimum":0,"maximum":99},"scope":{"type":"integer","minimum":0,"maximum":31},"offset":{"type":"integer","minimum":0,"maximum":10000,"default":0},"limit":{"type":"integer","minimum":1,"maximum":100}}),vec!["kind","pauseId","frame","scope","limit"]),
        object(json!({"kind":{"const":"logpoint_set"},"url":{"type":"string","minLength":1,"maxLength":4096},"line":{"type":"integer","minimum":0,"maximum":10000000},"column":{"type":"integer","minimum":0,"maximum":10000000,"default":0},"marker":{"type":"string","minLength":1,"maxLength":256}}),vec!["kind","url","line","marker"]),
        object(json!({"kind":{"const":"evaluate"},"pauseId":uuid,"frame":{"type":"integer","minimum":0,"maximum":99,"default":0},"expression":{"type":"string","maxLength":65536}}),vec!["kind","pauseId","expression"])
    ]}},"required":["operation","page","action"],"additionalProperties":false})
}
