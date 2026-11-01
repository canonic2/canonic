//! Schema semantics and diagnostics through the public validation boundary.

use super::*;
use serde_json::json;

fn message(schema: &Value, value: &Value) -> String {
    validate(schema, value).unwrap_err().message
}

#[test]
fn schema_type_diagnostics_use_expected_types_across_references() {
    for reference in ["#/$defs/name", "https://example.com/name"] {
        let schema = json!({"type":"object","$defs":{"name":{"$id":"https://example.com/name","type":"string"}},"properties":{"name":{"$ref":reference}}});
        assert_eq!(
            message(&schema, &json!({"name":5})),
            "arguments.name must be string",
            "{reference}"
        );
    }
    let error = message(&json!({"type":["string","null"]}), &json!(5));
    assert!(
        error.contains("must be") && error.contains("string") && error.contains("null"),
        "{error}"
    );
    let error = PreparedSchema::new(&json!({"type":"string","format":"date-time"}))
        .unwrap()
        .validate_at(&json!("invalid"), "structuredContent")
        .unwrap_err();
    assert!(error.message.contains("date-time"), "{}", error.message);
}

#[test]
fn nested_objects_report_the_failing_path() {
    let point = json!({"type":"object","properties":{"x":{"type":"number","minimum":0,"maximum":1}},"required":["x"],"additionalProperties":false});
    let list = json!({"type":"array","items":point,"maxItems":2});
    assert!(validate(&list, &json!([{"x":0.5}])).is_ok());
    assert_eq!(message(&list, &json!([{}])), "arguments[0].x is required");
    assert_eq!(
        message(&list, &json!([{"x":"a"}])),
        "arguments[0].x must be number"
    );
    assert_eq!(
        message(&list, &json!([{"x":0,"y":1}])),
        "arguments[0].y is not a supported field"
    );
    assert_eq!(
        message(&list, &json!([{"x":2}])),
        "arguments[0].x must be between 0 and 1; received 2"
    );
    assert_eq!(
        message(&list, &json!([{"x":0},{"x":0},{"x":0}])),
        "arguments must contain at most 2 items; received 3"
    );
}

#[test]
fn unions_keep_unique_nested_errors_and_refuse_ambiguous_matches() {
    for union in ["oneOf", "anyOf"] {
        let mut alternatives = json!({});
        alternatives[union] = json!([
            {"type":"object","properties":{"exact":{"type":"object"}},"required":["exact"],"additionalProperties":false},
            {"type":"object","properties":{"within":{"type":"object"}},"required":["within"],"additionalProperties":false}
        ]);
        assert_eq!(
            message(&alternatives, &json!({"exact":"/acme/project"})),
            "arguments.exact must be object"
        );
        let location = json!({"oneOf":[
            {"type":"object","properties":{"exact":{"oneOf":[
                {"type":"object","properties":{"root":{"const":"project"}},"required":["root"]},
                {"type":"object","properties":{"root":{"const":"absolute"},"path":{"type":"string"}},"required":["root","path"]}
            ]}},"required":["exact"]},
            {"type":"object","properties":{"within":{"type":"object"}},"required":["within"]}
        ]});
        let mut schema = json!({});
        schema[union] = json!([
            {"type":"object","properties":{"kind":{"const":"create"},"location":location},"required":["kind","location"]},
            {"type":"object","properties":{"kind":{"const":"remove"}},"required":["kind"]}
        ]);
        let bad = json!({"kind":"create","location":{"exact":"/acme/project"}});
        let error = validate(&schema, &bad).unwrap_err();
        assert_eq!(error.path, "arguments.location.exact");
        assert_eq!(error.message, "arguments.location.exact must be object");
        assert!(
            validate(
                &schema,
                &json!({"kind":"create","location":{"exact":{"root":"project"}}})
            )
            .is_ok()
        );
        assert!(validate(&schema, &json!({"kind":"other"})).is_err());
    }
    let ambiguous = json!({"oneOf":[
        {"type":"object","properties":{"left":{"type":"number"}}},
        {"type":"object","properties":{"right":{"type":"number"}}}
    ]});
    assert_eq!(
        validate(&ambiguous, &json!({"left":"bad","right":"bad"}))
            .unwrap_err()
            .path,
        "arguments"
    );
    assert!(
        validate(
            &json!({"oneOf":[{"type":"integer"},{"type":"number"}]}),
            &json!(1)
        )
        .is_err()
    );
}

#[test]
fn unions_references_conditionals_and_formats_validate() {
    let schema = json!({
        "$defs": {"id": {"type": "string", "format": "uuid"}},
        "type": "object",
        "properties": {
            "domain": {"enum": ["sample", "literal"]},
            "input": {"type": ["string", "null"]},
            "handle": {"$ref": "#/$defs/id"},
            "labels": {"type": "object", "additionalProperties": {"type": "string"}, "maxProperties": 2}
        },
        "if": {"properties": {"domain": {"const": "sample"}}},
        "then": {"required": ["input"]},
        "else": {"not": {"required": ["input"]}}
    });
    assert!(validate(&schema, &json!({"domain":"sample","input":null})).is_ok());
    assert!(validate(&schema, &json!({"domain":"sample"})).is_err());
    assert!(validate(&schema, &json!({"domain":"literal","input":"a"})).is_err());
    assert!(validate(&schema, &json!({"input":5,"domain":"sample"})).is_err());
    assert!(
        validate(
            &schema,
            &json!({"domain":"literal","handle":"123e4567-e89b-12d3-a456-426614174000"})
        )
        .is_ok()
    );
    assert!(validate(&schema, &json!({"domain":"literal","handle":"123e4567"})).is_err());
    assert!(validate(&schema, &json!({"domain":"literal","labels":{"a":1}})).is_err());
    assert!(
        validate(
            &schema,
            &json!({"domain":"literal","labels":{"a":"1","b":"2","c":"3"}})
        )
        .is_err()
    );
    assert_eq!(
        message(&schema, &json!({"domain":"other"})),
        r#"arguments.domain must match one of ["sample","literal"]"#
    );
    let exactly_one = json!({"oneOf":[{"type":"integer"},{"type":"number"}]});
    assert!(validate(&exactly_one, &json!(1.5)).is_ok());
    assert!(validate(&exactly_one, &json!(1)).is_err());
}

#[test]
fn schema_reference_siblings_and_annotations_are_enforced_together() {
    let schema = json!({
        "type": "object",
        "$defs": {"id": {"type": "string"}},
        "properties": {
            "id": {
                "$ref": "#/$defs/id",
                "title": "Acme ID",
                "description": "The Acme identifier.",
                "default": "acme",
                "examples": ["acme"]
            }
        }
    });
    assert!(check(&schema).is_ok());
    assert!(validate(&schema, &json!({"id": "acme"})).is_ok());
    assert!(validate(&schema, &json!({"id": 1})).is_err());
    for (keyword, value) in [
        ("maxLength", json!(8)),
        ("type", json!("string")),
        ("enum", json!(["acme"])),
        ("allOf", json!([{"minLength": 1}])),
    ] {
        let mut constrained = schema.clone();
        constrained["properties"]["id"][keyword] = value;
        assert!(check(&constrained).is_ok());
        assert!(validate(&constrained, &json!({"id":"acme"})).is_ok());
        assert!(validate(&constrained, &json!({"id":1})).is_err());
    }
    let mut composed = schema;
    composed["properties"]["id"] = json!({
        "allOf": [{"$ref": "#/$defs/id"}, {"maxLength": 8}]
    });
    assert!(check(&composed).is_ok());
    assert!(validate(&composed, &json!({"id": "acme"})).is_ok());
    assert!(validate(&composed, &json!({"id": "acme-long"})).is_err());
    assert!(validate(&composed, &json!({"id": 1})).is_err());
}

#[test]
fn check_refuses_keywords_validation_would_ignore() {
    let supported = json!({"type":"object","properties":{"a":{"type":"string","description":"An Acme label.","default":"x"}},"additionalProperties":false});
    assert!(check(&supported).is_ok());
    let error = check(
        &json!({"type":"object","properties":{"a":{"type":"string","unsupportedAssertion":"^a"}}}),
    )
    .unwrap_err();
    assert!(error.problem.contains("unsupportedAssertion"));
    assert_eq!(error.pointer, "/properties/a");
    assert!(check(&json!({"$ref":"#/$defs/missing"})).is_err());
    assert!(check(&json!({"type":"tuple"})).is_err());
    assert!(check(&json!({"maxItems":-1})).is_err());
}

#[test]
fn schema_202012_applicators_numbers_and_meta_validation() {
    let schema = json!({"$schema":DIALECT,"type":"object",
        "properties":{"id":{"type":"integer"},"price":{"type":"number","multipleOf":0.1},"pair":{"type":"array","prefixItems":[{"type":"string"},{"type":"integer"}],"items":false},"values":{"type":"array","contains":{"type":"integer"},"minContains":1,"maxContains":2,"uniqueItems":true}},
        "dependentRequired":{"id":["price"]},"propertyNames":{"pattern":"^[a-z]+$"},"unevaluatedProperties":false});
    let prepared = PreparedSchema::new(&schema).unwrap();
    assert!(
        prepared
            .validate(&json!({"id":1.0,"price":0.3,"pair":["acme",2],"values":[1,"x",2]}))
            .is_ok()
    );
    for value in [
        json!({"id":1}),
        json!({"price":0.15}),
        json!({"pair":["acme",2,3]}),
        json!({"pair":[1,2]}),
        json!({"values":[1,2,3]}),
        json!({"values":[1,1]}),
        json!({"values":["x"]}),
        json!({"unknown":true}),
        json!({"ID":1}),
    ] {
        assert!(prepared.validate(&value).is_err(), "{value}");
    }
    assert!(
        validate(
            &json!({"type":"integer","maximum":18_446_744_073_709_551_614_u64}),
            &json!(u64::MAX)
        )
        .is_err()
    );
    for schema in [
        json!({"type":[]}),
        json!({"required":[1]}),
        json!({"required":["a","a"]}),
        json!({"allOf":[]}),
        json!({"minContains":-1}),
        json!({"pattern":42}),
        json!({"format":true}),
    ] {
        assert!(check(&schema).is_err(), "{schema}");
    }
    assert!(check(&json!({"enum":[]})).is_ok());
    assert!(validate(&json!({"enum":[]}), &Value::Null).is_err());
    assert!(validate(&json!({"contentEncoding":"base64","contentMediaType":"application/json","contentSchema":{"type":"object"}}), &json!("not encoded JSON")).is_ok());
}

#[test]
fn schema_reference_targets_in_annotations_check_declarations() {
    for keyword in ["$ref", "$dynamicRef"] {
        for target in [
            json!({"additionalPropertes":false}),
            json!({"properties":{"nested":{"additionalPropertes":false}}}),
            json!({"$schema":"http://json-schema.org/draft-07/schema#"}),
            json!({"$vocabulary":{"https://example.com/custom":true}}),
            json!({"$ref":"#/examples/0"}),
        ] {
            let mut schema = json!({"type":"object","default":target,"examples":[{"additionalPropertes":false}]});
            // Annotation data remains arbitrary until referenced as a schema.
            assert!(check(&schema).is_ok());
            schema[keyword] = json!("#/default");
            assert!(check(&schema).is_err(), "{schema}");
        }
        let mut schema = json!({"type":"object","default":{"additionalProperties":false}});
        schema[keyword] = json!("#/default");
        let prepared = PreparedSchema::new(&schema).unwrap();
        assert!(prepared.validate(&json!({})).is_ok());
        assert!(prepared.validate(&json!({"extra":true})).is_err());
    }
}

#[test]
fn schema_bundled_ids_anchors_and_external_refusals() {
    let schema = json!({"$id":"https://example.com/schema/root","type":"object","$defs":{
        "name":{"$anchor":"name","type":"string","pattern":"^acme"},
        "part":{"$id":"part","type":"integer","minimum":1}
    },"properties":{"name":{"$ref":"#name","maxLength":8},"part":{"$ref":"part"}}});
    let prepared = PreparedSchema::new(&schema).unwrap();
    assert!(prepared.validate(&json!({"name":"acme","part":2})).is_ok());
    for value in [
        json!({"name":"other"}),
        json!({"name":"acme-long"}),
        json!({"part":0}),
    ] {
        assert!(prepared.validate(&value).is_err());
    }
    let dynamic = json!({"$defs":{"name":{"$dynamicAnchor":"name","type":"string"}},"type":"object","properties":{"name":{"$dynamicRef":"#name"}}});
    assert!(validate(&dynamic, &json!({"name":"acme"})).is_ok());
    assert!(validate(&dynamic, &json!({"name":1})).is_err());
    assert!(check(&json!({"$vocabulary":{"https://example.com/unknown":true}})).is_err());
    for schema in [
        json!({"$schema":"http://json-schema.org/draft-07/schema#"}),
        json!({"$schema":42}),
        json!({"$ref":"file:///acme/schema.json"}),
        json!({"$ref":"https://example.com/missing"}),
        json!({"$ref":"#missing"}),
        json!({"$ref":"#/$defs/missing"}),
        json!({"$ref":"#"}),
        json!({"not":{"pattern":"(?=a)"}}),
    ] {
        assert!(check(&schema).is_err(), "{schema}");
    }
}

#[test]
fn schema_work_and_cancellation_refuse_before_logical_evaluation() {
    let prepared = PreparedSchema::new(&json!({"not":false})).unwrap();
    assert!(prepared.validate(&json!(vec![0; 2000])).is_ok());
    assert_eq!(
        PreparedSchema::new(&json!({"not":{"uniqueItems":true}}))
            .unwrap()
            .validate(&json!(vec![0; 2100]))
            .unwrap_err()
            .kind,
        ArgumentErrorKind::Limit
    );
    let calls = std::cell::Cell::new(0);
    let result = prepared.validate_checked(&json!([0, 1, 2, 3]), "output", &|| {
        calls.set(calls.get() + 1);
        if calls.get() > 3 {
            Err(crate::Interrupted)
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err().kind, ArgumentErrorKind::Interrupted);
    let mut definitions = serde_json::Map::new();
    definitions.insert("n0".into(), json!(true));
    for index in 1..13 {
        let reference = format!("#/$defs/n{}", index - 1);
        definitions.insert(
            format!("n{index}"),
            json!({"anyOf":[{"$ref":reference},{"$ref":reference}]}),
        );
    }
    assert!(check(&json!({"$defs":definitions,"$ref":"#/$defs/n12"})).is_err());
    let root = PreparedSchema::new(&json!({})).unwrap();
    assert!(
        root.validate(&json!("x".repeat(MAX_VALUE_BYTES - 2)))
            .is_ok()
    );
    assert_eq!(
        root.validate(&json!("x".repeat(MAX_VALUE_BYTES - 1)))
            .unwrap_err()
            .kind,
        ArgumentErrorKind::Limit
    );
    let mut schema = json!({"$comment":""});
    let overhead = serde_json::to_vec(&schema).unwrap().len();
    schema["$comment"] = json!("x".repeat(MAX_SCHEMA_BYTES - overhead));
    assert!(check(&schema).is_ok());
    schema["$comment"] = json!("x".repeat(MAX_SCHEMA_BYTES - overhead + 1));
    assert!(check(&schema).is_err());
}

#[test]
fn schema_raw_node_and_depth_boundaries_are_distinct_from_mismatch() {
    let mut schema = json!({"default":vec![Value::Null; MAX_SCHEMA_NODES - 2]});
    assert!(check(&schema).is_ok());
    schema["default"].as_array_mut().unwrap().push(Value::Null);
    assert!(check(&schema).is_err());
    let prepared = PreparedSchema::new(&json!({})).unwrap();
    let mut value = Value::Null;
    for _ in 0..MAX_DEPTH {
        value = json!([value]);
    }
    assert!(prepared.validate(&value).is_ok());
    assert!(check(&value).is_err()); // A schema itself must be an object or boolean.
    assert_eq!(
        prepared.validate(&json!([value])).unwrap_err().kind,
        ArgumentErrorKind::Limit
    );
    prepared.validate(&json!(vec![0; 4097])).unwrap();
    let mut annotation = Value::Null;
    for _ in 1..MAX_DEPTH {
        annotation = json!([annotation]);
    }
    assert!(check(&json!({"default":annotation})).is_ok());
    assert!(check(&json!({"default":[annotation]})).is_err());
}

#[test]
fn descending_recursive_schemas_validate_children_and_bound_repeated_work() {
    let schema = json!({"$defs":{"node":{"type":"object","properties":{
        "name":{"type":"string"},"children":{"type":"array","items":{"$ref":"#/$defs/node"}}
    },"required":["name"],"additionalProperties":false}},"$ref":"#/$defs/node"});
    let prepared = PreparedSchema::new(&schema).unwrap();
    prepared
        .validate(&json!({"name":"Acme","children":[{"name":"Child"}]}))
        .unwrap();
    for invalid in [
        json!({"name":"Acme","children":[{}]}),
        json!({"name":"Acme","children":[{"name":5}]}),
        json!({"name":"Acme","children":[{"name":"Child","extra":true}]}),
    ] {
        assert_eq!(
            prepared.validate(&invalid).unwrap_err().kind,
            ArgumentErrorKind::Mismatch
        );
    }
    for schema in [
        json!({"$ref":"#"}),
        json!({"anyOf":[true,{"$ref":"#"}]}),
        json!({"$defs":{"unused":{"not":{"$ref":"#/$defs/unused"}}}}),
    ] {
        assert!(check(&schema).is_err());
    }
    // Each cycle descends, but duplicate applicators can still amplify work.
    // Refuse before logical negation could turn budget exhaustion into success.
    let bomb = PreparedSchema::new(&json!({"$defs":{"node":{"anyOf":[{"properties":{"child":{"$ref":"#/$defs/node"}}},{"properties":{"child":{"$ref":"#/$defs/node"}}}]}},"not":{"$ref":"#/$defs/node"}})).unwrap();
    let mut value = json!({});
    for _ in 0..24 {
        value = json!({"child":value});
    }
    assert_eq!(
        bomb.validate(&value).unwrap_err().kind,
        ArgumentErrorKind::Limit
    );
}
