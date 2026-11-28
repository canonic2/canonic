//! Typed requests for document-scoped application WebMCP tools.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Enable {},
    Status {},
    Invoke {
        name: String,
        #[serde(default = "parameters")]
        parameters: Value,
    },
    Read {
        #[serde(rename = "invocationId")]
        invocation_id: Uuid,
    },
    Cancel {
        #[serde(rename = "invocationId")]
        invocation_id: Uuid,
    },
    Disable {},
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PageAction {
    Enable {},
    Status {},
    Invoke {
        #[serde(rename = "toolRef")]
        tool_ref: Uuid,
        #[serde(default = "parameters")]
        parameters: Value,
    },
    Read {
        #[serde(rename = "invocationId")]
        invocation_id: Uuid,
    },
    Cancel {
        #[serde(rename = "invocationId")]
        invocation_id: Uuid,
    },
    Disable {},
}

fn parameters() -> Value {
    json!({})
}
impl Action {
    pub(crate) fn responsive(&self) -> bool {
        matches!(
            self,
            Self::Status {} | Self::Read { .. } | Self::Cancel { .. } | Self::Disable {}
        )
    }
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::Invoke { name, parameters } = self {
            if name.is_empty() || name.len() > 256 {
                return Err("WEB_MCP_TOOL_NAME_BOUNDS");
            }
            super::developer_tools::validate_parameters(parameters)?;
        }
        Ok(())
    }
}

impl PageAction {
    pub(crate) fn responsive(&self) -> bool {
        matches!(
            self,
            Self::Status {} | Self::Read { .. } | Self::Cancel { .. } | Self::Disable {}
        )
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::Invoke { parameters, .. } = self {
            super::developer_tools::validate_parameters(parameters)?;
        }
        Ok(())
    }
}

pub(crate) fn schema(reference: &Value, debugging: bool) -> Value {
    let mut actions = Vec::new();
    for kind in ["enable", "status", "disable"] {
        actions.push(json!({"type":"object","properties":{"kind":{"const":kind}},"required":["kind"],"additionalProperties":false}));
    }
    for kind in ["read", "cancel"] {
        actions.push(json!({"type":"object","properties":{"kind":{"const":kind},"invocationId":{"type":"string","format":"uuid"}},"required":["kind","invocationId"],"additionalProperties":false}));
    }
    let (operation, selector, selector_schema) = if debugging {
        (
            "web_mcp_debug",
            "name",
            json!({"type":"string","minLength":1,"maxLength":256}),
        )
    } else {
        (
            "web_mcp",
            "toolRef",
            json!({"type":"string","format":"uuid"}),
        )
    };
    actions.push(json!({"type":"object","properties":{"kind":{"const":"invoke"},(selector):selector_schema,"parameters":{"type":"object","default":{}}},"required":["kind",selector],"additionalProperties":false}));
    json!({"type":"object","properties":{"operation":{"const":operation},"scope":reference,"action":{"oneOf":actions}},"required":["operation","scope","action"],"additionalProperties":false})
}
