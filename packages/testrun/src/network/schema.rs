//! Exact public schema for the managed network control tool.
use serde_json::{Value, json};

pub(crate) fn tools() -> Vec<Value> {
    vec![json!({
        "name":"network_set",
        "description":"Turn the connected Android device's network off or back on; airplane mode also turns Bluetooth off; the change stays until you request the opposite mode",
        "inputSchema":{
            "type":"object",
            "properties":{
                "mode":{"enum":["offline","online"]},
                "method":{"enum":["airplane","radios"],"default":"airplane"},
                "timeoutMs":{"type":"integer","minimum":1000,"maximum":60000,"description":"How long to wait for the switches to read back"}
            },
            "required":["mode"],
            "additionalProperties":false
        },
        "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":true}
    })]
}
