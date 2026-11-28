//! Append-only operation schemas preserve existing public variant identities.
use super::*;
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let fetch = super::super::schemas(reference)
        .into_iter()
        .find(|schema| schema["properties"]["operation"]["const"] == "http_fetch")
        .expect("Registered HTTP fetch schema");
    let mut options = fetch["properties"]["request"].clone();
    options.as_object_mut().unwrap().remove("required");
    options["properties"]["method"]
        .as_object_mut()
        .unwrap()
        .remove("default");
    options["properties"]["method"]["minLength"] = json!(0);
    options["properties"]["method"]["description"] =
        json!("Omitted, null or empty follows the retained Request's current method");
    options["properties"]["method"]["type"] = json!(["string", "null"]);
    options["properties"]["headers"] = json!({"anyOf":[options["properties"]["headers"],{"type":"null"}],"description":"Omitted/null copies retained current caller headers; an explicit list, including [], replaces them"});
    let id = json!({"type":"string","format":"uuid"});
    vec![
        json!({"type":"object","properties":{"operation":{"const":"http_request_inputs_set"},"context":reference,"enabled":{"type":"boolean"}},"required":["operation","context","enabled"],"additionalProperties":false,"description":"Override automatic private Request input retention during network diagnostics. The explicit choice survives stop/start in this context; it does not erase earlier copies, which survive source closure until explicit release or connection cleanup."}),
        json!({"type":"object","properties":{"operation":{"const":"http_fetch_request"},"httpContext":reference,"sourceRequestId":id,"options":options},"required":["operation","httpContext","sourceRequestId"],"additionalProperties":false,"description":"Fetch from this connection's retained current Request inputs, with explicit option overrides and ordinary HTTP transport/deadline ownership"}),
        json!({"type":"object","properties":{"operation":{"const":"http_request_input_remove"},"sourceRequestId":id},"required":["operation","sourceRequestId"],"additionalProperties":false,"description":"Release an owned Request initializer without cancelling already dispatched HTTP work"}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_request_input_schema_inherits_fetch_fields_without_requiring_source_overrides() {
        let schemas = schemas(&json!({"type":"object"}));
        assert_eq!(schemas.len(), 3);
        let fetch = &schemas[1];
        assert_eq!(
            fetch["required"],
            json!(["operation", "httpContext", "sourceRequestId"])
        );
        let options = &fetch["properties"]["options"];
        assert!(options.get("required").is_none());
        assert!(options["properties"]["method"].get("default").is_none());
        assert_eq!(options["properties"]["method"]["minLength"], 0);
        assert_eq!(
            options["properties"]["method"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(options["properties"]["headers"]["anyOf"][1]["type"], "null");
        for value in [
            json!({}),
            json!({"method":null,"headers":null}),
            json!({"method":"","headers":[]}),
        ] {
            let overrides: Overrides = serde_json::from_value(value).unwrap();
            overrides.validate().unwrap();
        }
    }
}
