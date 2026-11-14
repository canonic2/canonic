//! Source schema shared with Shield's actual proposal admission.
use serde_json::{Value, json};
pub(crate) fn definitions() -> Value {
    let text = json!({"$ref":"#/$defs/text"});
    let named = |max, shape| json!({"type":"object","minProperties":1,"maxProperties":max,"additionalProperties":shape});
    let argument = json!({"$ref":"#/$defs/graphql_argument"});
    let argument_definition = json!({"type":"object","required":["match"],"additionalProperties":false,"properties":{
        "required":{"type":"boolean"},"nullable":{"type":"boolean"},"cursor":{"type":"boolean"},
        "match":{"oneOf":[
            {"type":"object","required":["text"],"additionalProperties":false,"properties":{"text":text}},
            {"type":"object","required":["exact"],"additionalProperties":false,"properties":{"exact":{"type":["string","number","boolean"]}}},
            {"type":"object","required":["scalar"],"additionalProperties":false,"properties":{"scalar":{"enum":["string","integer","number","boolean","enum"]}}},
            {"type":"object","required":["object"],"additionalProperties":false,"properties":{"object":named(64,argument.clone())}}
        ]}
    }});
    let field = json!({"type":"object","additionalProperties":false,"properties":{
        "select":{"type":"string","minLength":1,"maxLength":64},"leaf":{"const":true},
        "arguments":{"type":"object","maxProperties":64,"additionalProperties":argument}
    },"oneOf":[{"required":["select"],"not":{"required":["leaf"]}},{"required":["leaf"],"not":{"required":["select"]}}]});
    let state = json!({"type":"object","required":["types","fields"],"additionalProperties":false,"properties":{
        "types":{"type":"array","minItems":1,"maxItems":64,"uniqueItems":true,"items":{"type":"string","minLength":1}},
        "fields":named(256,field)
    }});
    json!({"graphql_argument":argument_definition,"graphql":{"type":"object","required":["carrier","operation"],"additionalProperties":false,"properties":{
        "carrier":{"const":"gh-api"},"operation":{"enum":["query","mutation"]},
        "root_fields":{"type":"array","minItems":1,"maxItems":256,"uniqueItems":true,"items":{"type":"string","minLength":1}},
        "selection":{"type":"object","required":["root","states"],"additionalProperties":false,"properties":{
            "root":{"type":"string","minLength":1,"maxLength":64},"states":named(256,state)
        }}
    },"if":{"properties":{"operation":{"const":"mutation"}}},"then":{"required":["selection","root_fields"]},"else":{"not":{"required":["root_fields"]}}}})
}
