//! Source shape definitions reused by agent proposal schemas. They use only
//! keywords the MCP argument validator accepts. Known formats are enforced;
//! annotations carry no constraints. Admission adds semantic checks, including
//! expiry timestamps, unique list members, rule
//! identifiers and method names.
use serde_json::{Value, json};

fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/$defs/{name}")})
}
fn object(properties: Value, required: &[&str]) -> Value {
    let mut value = json!({"type":"object","required":required,"additionalProperties":false});
    value["properties"] = properties;
    value
}
fn string(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}
fn nonempty(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn list(items: Value, min: usize) -> Value {
    let mut value = json!({"type":"array","minItems":min,"maxItems":256});
    value["items"] = items;
    value
}
fn names() -> Value {
    list(nonempty(128), 0)
}
fn one_of(values: Vec<Value>) -> Value {
    let mut value = json!({});
    value["oneOf"] = Value::Array(values);
    value
}
fn count() -> Value {
    json!({"type":"integer","minimum":0,"maximum":256})
}
fn enum_text(values: &[&str]) -> Value {
    one_of(vec![
        json!({"enum":values}),
        object(
            json!({"one_of":{"type":"array","minItems":1,"maxItems":256,"items":{"enum":values}}}),
            &["one_of"],
        ),
    ])
}
fn fields() -> Value {
    json!({"type":"object","minProperties":1,"maxProperties":256,"additionalProperties":{"oneOf":[object(json!({"exact":{}}),&["exact"]),object(json!({"text":reference("text")}),&["text"]),object(json!({"not":object(json!({"exact":{}}),&["exact"])}),&["not"])]}})
}
fn context() -> Value {
    object(
        json!({"cwd":nonempty(4096),"environment":{"oneOf":[{"type":"null"},{"type":"object","additionalProperties":{"type":"string"}}]}}),
        &["cwd", "environment"],
    )
}

fn base() -> Value {
    let mut text = one_of(vec![
        string(4096),
        object(json!({"exact":string(4096)}), &["exact"]),
        object(json!({"prefix":nonempty(4096)}), &["prefix"]),
        object(
            json!({"one_of":{"type":"array","minItems":1,"maxItems":256,"items":string(4096)}}),
            &["one_of"],
        ),
        object(json!({"glob":string(4096)}), &["glob"]),
        object(json!({"regex":string(4096)}), &["regex"]),
        object(json!({"not":reference("text")}), &["not"]),
    ]);
    text["description"] = json!(
        "Text matcher: a literal string or an exact, prefix, one_of, glob, regex or not object. not matches a present value that its inner matcher does not match. This is not a filesystem path predicate."
    );
    let mut path_ref = one_of(vec![
        object(
            json!({"root":{"enum":["project","read","write"]},"path":nonempty(4096)}),
            &["root"],
        ),
        object(
            json!({"root":{"const":"absolute"},"path":nonempty(4096)}),
            &["root", "path"],
        ),
    ]);
    path_ref["description"] = json!(
        "Path-reference object, never a string. Use {\"root\":\"project\"} for the checkout root; add a relative path for a child. read/write select approved access roots. absolute requires path and is not portable in project rule proposals."
    );
    let mut path = one_of(vec![
        object(json!({"exact":reference("path_reference")}), &["exact"]),
        object(json!({"outside":reference("path_reference")}), &["outside"]),
        object(
            json!({"within":reference("path_reference"),"exclude":list(reference("path_reference"),0),"match":reference("text")}),
            &["within"],
        ),
    ]);
    path["description"] = json!(
        "Filesystem predicate. exact matches only the referenced location; within includes that root and descendants; outside excludes that root and descendants. Each value is a path-reference object, unlike text.exact's string. Choose the intended scope; do not replace exact with within to fix syntax."
    );
    path["examples"] = json!([
        {"exact":{"root":"project"}},
        {"within":{"root":"project","path":"src"}}
    ]);
    let quantified = one_of(vec![
        object(json!({"all":reference("path")}), &["all"]),
        object(json!({"any":reference("path")}), &["any"]),
    ]);
    let target = one_of(vec![
        object(
            json!({"action":{"enum":["read","write","remove"]},"all":reference("path")}),
            &["action", "all"],
        ),
        object(
            json!({"action":{"enum":["read","write","remove"]},"any":reference("path")}),
            &["action", "any"],
        ),
    ]);
    let environment = one_of(vec![
        object(
            json!({"exact":{"type":"object","maxProperties":256,"additionalProperties":string(4096)}}),
            &["exact"],
        ),
        object(
            json!({"fields":{"type":"object","minProperties":1,"maxProperties":256,"additionalProperties":reference("text")}}),
            &["fields"],
        ),
    ]);
    json!({"text":text,"path_reference":path_ref,"path":path,"quantified":quantified,"target":target,"environment":environment,"field_map":fields(),"method":one_of(vec![json!({"type":"string"}),object(json!({"one_of":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"string"}}}),&["one_of"])])})
}
fn arguments() -> Value {
    let operand = object(
        json!({"match":reference("text"),"min":count(),"max":count()}),
        &["match"],
    );
    let valued = one_of(vec![
        nonempty(128),
        object(
            json!({"name":nonempty(128),"match":reference("text"),"repeat":{"enum":["all","forbid","last"]}}),
            &["name"],
        ),
    ]);
    let grammar = json!({"flags":names(),"values":list(valued.clone(),0),"optional_values":list(valued,0),"short_clusters":{"type":"boolean","default":true},"long_equals":{"type":"boolean","default":true},"short_attached":{"type":"boolean","default":true},"long_abbreviations":{"type":"boolean","default":false}});
    let mut leading = grammar.clone();
    leading["interleaved"] = json!({"type":"boolean","default":false});
    let leading = object(leading, &["flags", "values"]);
    let mut options = grammar;
    options["kind"] = json!({"const":"options"});
    options["unknown_options"] = json!({"enum":["refuse","skip"],"default":"refuse"});
    options["prefix"] = list(reference("text"), 0);
    options["leading"] = reference("leading");
    options["operands"] = one_of(vec![json!({"type":"boolean"}), operand.clone()]);
    for key in ["require_all", "require_any", "forbid"] {
        options[key] = names();
    }
    options["parse"] = json!({"enum":["intermixed","stop_at_operand"],"default":"intermixed"});
    let rest = one_of(vec![
        object(json!({"kind":{"const":"none"}}), &["kind"]),
        object(json!({"kind":{"const":"any"}}), &["kind"]),
        object(
            json!({"kind":{"const":"each"},"match":reference("text"),"min":count(),"max":count()}),
            &["kind", "match"],
        ),
    ]);
    let mut arguments = one_of(vec![
        object(json!({"kind":{"const":"any"}}), &["kind"]),
        object(
            json!({"kind":{"const":"exact"},"values":list(string(4096),0)}),
            &["kind", "values"],
        ),
        object(
            json!({"kind":{"const":"prefix"},"values":list(reference("text"),1),"leading":reference("leading")}),
            &["kind", "values"],
        ),
        object(
            json!({"kind":{"const":"sequence"},"prefix":list(reference("text"),0),"rest":rest}),
            &["kind", "prefix", "rest"],
        ),
        object(options, &["kind", "prefix", "flags", "values", "operands"]),
        object(
            json!({"kind":{"const":"risk_tokens"},"prefix":list(reference("text"),0),"any_of":list(reference("text"),1)}),
            &["kind", "prefix", "any_of"],
        ),
    ]);
    arguments["description"] = json!(
        "Command argument grammar, excluding the executable. options requires kind, prefix, flags, values and operands. For a base-command-only rule use empty flags/values, operands=false and no optional_values. After --, flags are operands. Review forwarded flags before allowing them; any and prefix permit broader command families."
    );
    arguments["examples"] = json!([
        {"kind":"options","prefix":["run","dev"],"flags":[],"values":[],"operands":false}
    ]);
    json!({"leading":leading,"arguments":arguments})
}
fn domains() -> Value {
    let url = object(
        json!({"schemes":{"type":"array","minItems":1,"items":{"enum":["http","https"]}},"hosts":{"type":"array","minItems":1,"maxItems":256,"items":nonempty(4096)},"ports":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"integer","minimum":1,"maximum":65535}},"path":reference("text"),"subdomains":{"type":"boolean"}}),
        &["schemes", "hosts"],
    );
    let common = json!({"cwd":reference("path"),"environment":reference("environment")});
    let argv = json!({"arguments":reference("arguments"),"graphql":reference("graphql"),"targets":one_of(vec![reference("target"),list(reference("target"),1)]),"possible_effects":{"type":"array","minItems":1,"maxItems":11,"items":{"enum":["repository-write","worktree-write","data-removal","history-rewrite","network","code-execution","daemon-write","image-build","host-exposure","privileged-runtime","configuration"]}}});
    let shell = json!({"source":reference("text"),"dialect_version":one_of(vec![nonempty(4096),object(json!({"exact":nonempty(4096)}),&["exact"])])});
    let file = json!({"sources":reference("quantified"),"destination":reference("path"),"recursive":{"type":"boolean"},"targets":one_of(vec![reference("target"),list(reference("target"),1)])});
    let web = json!({"query":reference("text"),"url":reference("url"),"method":reference("method"),"request_fields":reference("field_map")});
    let mcp = json!({"arguments":{"type":"object"},"fields":reference("field_map"),"connection":reference("field_map"),"field_paths":{"type":"object","minProperties":1,"maxProperties":256,"additionalProperties":object(json!({"path":reference("path"),"recursive":{"type":"boolean"},"absolute":{"type":"boolean"},"relative_to":{"enum":["cwd","project"]},"each":string(1024)}),&["path"])}});
    let mut conditions = Vec::new();
    let mut requirements = Vec::new();
    for (domain, predicates, identity, required) in [
        (
            "argv",
            argv,
            json!({"executable":one_of(vec![reference("text"),reference("path_reference")])}),
            vec!["domain", "executable"],
        ),
        (
            "shell",
            shell,
            json!({"dialect":enum_text(&["posix","bash","powershell","cmd","unsupported"])}),
            vec!["domain", "source", "dialect"],
        ),
        (
            "file",
            file,
            json!({"action":enum_text(&["read","write","copy","move","remove"])}),
            vec!["domain", "action"],
        ),
        (
            "web",
            web,
            json!({"action":{"enum":["search","fetch","request"]}}),
            vec!["domain", "action"],
        ),
        (
            "mcp",
            mcp,
            json!({"server":reference("text"),"tool":reference("text")}),
            vec!["domain", "server", "tool"],
        ),
    ] {
        let mut fields = common.clone();
        fields
            .as_object_mut()
            .expect("schema object")
            .extend(predicates.as_object().expect("schema object").clone());
        let mut req = object(fields.clone(), &[]);
        if domain == "argv" {
            req["properties"]["docker_isolated_bind"] = object(
                json!({"context":nonempty(128),"image":nonempty(4096),"command":list(nonempty(4096),1),"source":reference("path"),"target":nonempty(4096),"environment":list(nonempty(4096),1)}),
                &["context", "image", "command", "source", "target"],
            );
        }
        req["minProperties"] = json!(1);
        requirements.push(req);
        fields
            .as_object_mut()
            .expect("schema object")
            .extend(identity.as_object().expect("schema object").clone());
        fields["domain"] = json!({"const":domain});
        conditions.push(object(fields, &required));
    }
    conditions.push(object(json!({"domain":{"const":"exact_request"},"input":reference("normalized_input"),"context":context(),"executable":{"oneOf":[nonempty(4096),{"type":"null"}]}}),&["domain","input","context","executable"]));
    json!({"url":url,"conditions":one_of(conditions),"requirements":{"anyOf":requirements}})
}
fn rule() -> Value {
    let message = nonempty(512);
    let alternatives = json!({"type":"array","minItems":1,"maxItems":3,"items":nonempty(256)});
    let mut guidance = object(
        json!({"message":message.clone(),"alternatives":alternatives.clone()}),
        &[],
    );
    guidance["minProperties"] = json!(1);
    let mut rule = object(
        json!({"id":{"type":"string","minLength":1,"maxLength":128},"decision":{"enum":["allow","ask","deny"]},"conditions":{"anyOf":[reference("conditions"),reference("historical_conditions")]},"requirements":reference("requirements"),"on_unmet":object(json!({"decision":{"enum":["ask","deny"]},"message":message,"alternatives":alternatives}),&["decision"]),"reason":string(4096),"guidance":reference("guidance"),"expires_at":{"type":"string"},"historical_matcher":reference("historical_matcher"),"historical_normalized":reference("historical_normalized")}),
        &["id", "decision", "conditions"],
    );
    rule["if"] = json!({"required":["historical_normalized"]});
    rule["then"] = json!({"properties":{"conditions":reference("historical_conditions")},"not":{"anyOf":[{"required":["historical_matcher"]},{"required":["requirements"]},{"required":["on_unmet"]},{"required":["guidance"]}]}});
    rule["else"] = json!({"properties":{"conditions":reference("conditions"),"reason":nonempty(4096),"expires_at":{"type":"string","format":"date-time"}}});
    json!({"guidance":guidance,"rule_custom":rule})
}

fn historical_arguments(prefix_min: usize) -> Value {
    one_of(vec![
        object(json!({"kind":{"const":"any"}}), &["kind"]),
        object(
            json!({"kind":{"const":"exact"},"values":list(string(4096),0)}),
            &["kind", "values"],
        ),
        object(
            json!({"kind":{"const":"prefix"},"values":list(string(4096),prefix_min)}),
            &["kind", "values"],
        ),
    ])
}

fn historical_effects() -> Value {
    json!({"type":"array","minItems":1,"maxItems":11,"items":{"enum":["repository-write","worktree-write","data-removal","history-rewrite","network","code-execution","daemon-write","image-build","host-exposure","privileged-runtime","configuration"]}})
}

fn historical() -> Value {
    let matcher = object(
        json!({"executable":nonempty(4096),"arguments":historical_arguments(1),"cwd":nonempty(4096),"project":nonempty(4096),"possible_effects":historical_effects()}),
        &["executable", "arguments"],
    );
    let authority = object(
        json!({"matcher":matcher,"scope":{"const":"project"},"mandatory":{"const":false}}),
        &["matcher", "scope", "mandatory"],
    );
    let nullable = |value| one_of(vec![json!({"type":"null"}), value]);
    let normalized = object(
        json!({"schema_version":{"const":1},"id":nonempty(128),"decision":{"enum":["allow","ask","deny"]},"conditions":reference("historical_conditions"),"reason":nullable(string(4096)),"expires_at":nullable(string(20))}),
        &[
            "schema_version",
            "id",
            "decision",
            "conditions",
            "reason",
            "expires_at",
        ],
    );
    json!({"historical_matcher":authority,"historical_normalized":normalized,"historical_conditions":historical_conditions(),"historical_input":historical_input(),"policy_capabilities":{"type":"array","maxItems":crate::policy::CAPABILITIES.len(),"uniqueItems":true,"items":{"enum":crate::policy::CAPABILITIES}}})
}

fn historical_normalized_arguments() -> Value {
    let values = json!({"type":"array","maxItems":50_000,"items":string(1_048_576)});
    one_of(vec![
        object(json!({"kind":{"const":"any"},"values":{}}), &["kind"]),
        object(
            json!({"kind":{"const":"exact"},"values":values.clone()}),
            &["kind", "values"],
        ),
        object(
            json!({"kind":{"const":"prefix"},"values":values}),
            &["kind", "values"],
        ),
    ])
}

fn historical_input() -> Value {
    let nullable = |value| one_of(vec![json!({"type":"null"}), value]);
    let strings = json!({"type":"array","maxItems":256,"items":string(1_048_576)});
    let mut inputs = Vec::new();
    for (kind, fields, mut required) in [
        (
            "argv",
            json!({"executable":nonempty(4096),"arguments":list(string(4096),0)}),
            vec!["executable", "arguments"],
        ),
        (
            "shell",
            json!({"source":string(65536),"dialect":{"enum":["bash","posix","powershell","cmd","unsupported"]},"dialect_version":nullable(string(1_048_576))}),
            vec!["source", "dialect"],
        ),
        (
            "file",
            json!({"action":{"enum":["read","write","copy","move","remove"]},"sources":strings,"destination":nullable(string(1_048_576)),"recursive":{"type":"boolean"}}),
            vec!["action", "sources", "recursive"],
        ),
        (
            "web",
            json!({"action":{"enum":["search","fetch","request"]},"query":nullable(string(1_048_576)),"url":nullable(string(1_048_576)),"method":nullable(string(1_048_576)),"conditions":{"type":"object"}}),
            vec!["action", "conditions"],
        ),
        (
            "mcp",
            json!({"server":nonempty(1_048_576),"tool":nonempty(1_048_576),"arguments":{"type":"object"}}),
            vec!["server", "tool", "arguments"],
        ),
        (
            "unknown",
            json!({"host_tool":nonempty(1_048_576)}),
            vec!["host_tool"],
        ),
    ] {
        let mut fields = fields;
        fields["kind"] = json!({"const":kind});
        fields["original_input"] = json!({"type":"object"});
        required.extend(["kind", "original_input"]);
        inputs.push(object(fields, &required));
    }
    one_of(inputs)
}

fn historical_conditions() -> Value {
    let nullable = |value| one_of(vec![json!({"type":"null"}), value]);
    let path = object(
        json!({"kind":{"enum":["exact","within"]},"path":nonempty(4096)}),
        &["kind", "path"],
    );
    let common = json!({"cwd":nullable(path.clone()),"environment":nullable(json!({"type":"object","additionalProperties":{"type":"string"}}))});
    let mut conditions = Vec::new();
    for (domain, predicates, required) in [
        (
            "argv",
            json!({"executable":nonempty(1_048_576),"arguments":historical_normalized_arguments(),"possible_effects":historical_effects()}),
            vec!["domain", "executable", "arguments"],
        ),
        (
            "shell",
            json!({"source":string(1_048_576),"dialect":{"enum":["bash","posix","powershell","cmd"]},"dialect_version":nullable(string(1_048_576))}),
            vec!["domain", "source", "dialect"],
        ),
        (
            "file",
            json!({"action":{"enum":["read","write","copy","move","remove"]},"sources":nullable(list(path.clone(),0)),"destination":nullable(path),"recursive":{"type":"boolean"}}),
            vec!["domain", "action"],
        ),
        (
            "web",
            json!({"action":{"enum":["fetch","search","request"]},"url":nullable(string(1_048_576)),"query":nullable(string(1_048_576)),"method":nullable(string(1_048_576)),"conditions":{"type":"object"}}),
            vec!["domain", "action"],
        ),
        (
            "mcp",
            json!({"server":string(1_048_576),"tool":string(1_048_576),"arguments":{"type":"object"}}),
            vec!["domain", "server", "tool"],
        ),
    ] {
        let mut properties = common.clone();
        properties
            .as_object_mut()
            .expect("schema object")
            .extend(predicates.as_object().expect("schema object").clone());
        properties["domain"] = json!({"const":domain});
        conditions.push(object(properties, &required));
    }
    let context = object(
        json!({"cwd":nonempty(4096),"environment":nullable(json!({"type":"object","additionalProperties":{"type":"string"}}))}),
        &["cwd"],
    );
    conditions.push(object(json!({"domain":{"const":"exact_request"},"input":reference("historical_input"),"context":context,"executable":nullable(nonempty(4096))}), &["domain","input","context","executable"]));
    one_of(conditions)
}
pub(crate) fn definitions() -> Value {
    let mut value = base();
    for section in [arguments(), domains(), historical(), rule()] {
        if let Value::Object(fields) = section {
            value.as_object_mut().expect("schema object").extend(fields);
        }
    }
    value.as_object_mut().expect("schema object").extend(
        crate::graphql::schema()
            .as_object()
            .expect("schema object")
            .clone(),
    );
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_schema() -> Value {
        let mut schema = json!({"$defs":definitions(),"$ref":"#/$defs/rule_custom"});
        schema["$defs"]["normalized_input"] = json!({"type":"object"});
        schema
    }

    #[test]
    fn reviewed_historical_rows_pass_closed_proposal_schemas() {
        for row in [
            json!({"id":"read","scope":"project","decision":"allow","reason":"Acme read",
                "matcher":{"executable":"acme","arguments":{"kind":"exact","values":["read"]},"project":"/project"}}),
            json!({"schema_version":1,"id":"Acme read rule","decision":"allow","reason":null,"expires_at":null,
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":[]},"cwd":null}}),
        ] {
            let source = json!({"schema_version":1,"rules":[row]}).to_string();
            let normalized = crate::policy::normalize_retained_custom(&source).unwrap();
            let row = &normalized["rules"][0];
            canonic_mcp_kit::schema::validate(&rule_schema(), row).unwrap();
            let mut unknown = row.clone();
            unknown["rank"] = json!(255);
            assert!(canonic_mcp_kit::schema::validate(&rule_schema(), &unknown).is_err());
            let mut nested = row.clone();
            let field = if row.get("historical_matcher").is_some() {
                "historical_matcher"
            } else {
                "historical_normalized"
            };
            nested[field]["tier"] = json!(0);
            assert!(canonic_mcp_kit::schema::validate(&rule_schema(), &nested).is_err());
        }
    }

    #[test]
    fn historical_shell_versions_keep_empty_and_long_retained_values() {
        for version in [String::new(), "v".repeat(65), "v".repeat(4096)] {
            let original = json!({"schema_version":1,"rules":[{
                "schema_version":1,"id":"old-shell","decision":"deny","reason":null,"expires_at":null,
                "conditions":{"domain":"exact_request","input":{
                    "kind":"shell","source":"git status","dialect":"powershell","dialect_version":version,"original_input":{}
                },"context":{"cwd":"/project","environment":{}},"executable":"/bin/powershell"}
            }]}).to_string();
            let normalized = crate::policy::normalize_retained_custom(&original).unwrap();
            canonic_mcp_kit::schema::validate(&rule_schema(), &normalized["rules"][0]).unwrap();
            crate::policy::CustomRules::from_yaml(&normalized.to_string()).unwrap();
        }
    }

    #[test]
    fn historical_conditions_require_their_explicit_annotation() {
        let row = json!({"id":"read","decision":"allow","conditions":{"domain":"argv","executable":"acme",
            "arguments":{"kind":"exact","values":["read"]},"cwd":null}});
        assert!(canonic_mcp_kit::schema::validate(&rule_schema(), &row).is_err());
        let mut source = row;
        source["historical_matcher"] = json!({"matcher":{"executable":"acme","arguments":{"kind":"exact","values":["read"]}},"scope":"organization","mandatory":false});
        assert!(canonic_mcp_kit::schema::validate(&rule_schema(), &source).is_err());
    }

    #[test]
    fn historical_normalized_reasons_keep_the_original_four_kib_limit() {
        // The historical normalized rule validator bounds reason bytes at 4096.
        for (length, accepted) in [(4096, true), (4097, false)] {
            let original = json!({"schema_version":1,"rules":[{
                "schema_version":1,"id":"read","decision":"allow",
                "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},
                "reason":"x".repeat(length),"expires_at":null
            }]})
            .to_string();
            let normalized = crate::policy::normalize_retained_custom(&original);
            assert_eq!(normalized.is_ok(), accepted);
            let mut modern = json!({"id":"read","decision":"allow","conditions":{
                "domain":"argv","executable":"acme","arguments":{"kind":"any"}},
                "reason":"x".repeat(length)});
            assert_eq!(
                canonic_mcp_kit::schema::validate(&rule_schema(), &modern).is_ok(),
                accepted
            );
            if let Ok(normalized) = normalized {
                canonic_mcp_kit::schema::validate(&rule_schema(), &normalized["rules"][0]).unwrap();
                modern = normalized["rules"][0].clone();
                modern["reason"] = json!("x".repeat(4097));
                modern["historical_normalized"]["reason"] = modern["reason"].clone();
                assert!(canonic_mcp_kit::schema::validate(&rule_schema(), &modern).is_err());
            }
        }
    }
}
