//! Typed lifecycle actions for unpacked extensions owned by a browser connection.
use super::contract::Reference;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Install {
        directory: Reference,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u64>,
    },
    Reload {
        #[serde(rename = "extensionId")]
        extension_id: Uuid,
    },
    TriggerAction {
        #[serde(rename = "extensionId")]
        extension_id: Uuid,
        page: Reference,
    },
    Uninstall {
        #[serde(rename = "extensionId")]
        extension_id: Uuid,
    },
}

pub(super) fn schema(reference: &Value) -> Value {
    let extension = json!({"type":"string","format":"uuid"});
    let mut actions = vec![
        json!({"type":"object","properties":{"kind":{"const":"install"},"directory":reference},"required":["kind","directory"],"additionalProperties":false}),
        super::pwa::list_schema(),
        json!({"type":"object","properties":{"kind":{"const":"trigger_action"},"extensionId":extension,"page":reference},"required":["kind","extensionId","page"],"additionalProperties":false}),
    ];
    for kind in ["reload", "uninstall"] {
        actions.push(json!({"type":"object","properties":{"kind":{"const":kind},"extensionId":extension},"required":["kind","extensionId"],"additionalProperties":false}));
    }
    json!({"type":"object","properties":{"operation":{"const":"browser_extension"},"action":{"oneOf":actions}},"required":["operation","action"],"additionalProperties":false})
}
