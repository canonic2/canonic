//! Source shape definitions reused by agent proposal schemas. They use only
//! keywords the MCP argument validator accepts. Annotation keywords and
//! formats other than uuid are not enforced by that validator; admission adds
//! semantic checks, including expiry timestamps, unique list members, rule
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
    json!({"type":"object","minProperties":1,"maxProperties":256,"additionalProperties":{"oneOf":[object(json!({"exact":{}}),&["exact"]),object(json!({"text":reference("text")}),&["text"])]}})
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
    let leading = object(grammar.clone(), &["flags", "values"]);
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
        json!({"schemes":{"type":"array","minItems":1,"items":{"enum":["http","https"]}},"hosts":{"type":"array","minItems":1,"maxItems":256,"items":nonempty(4096)},"ports":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"integer","minimum":1,"maximum":65535}},"path":reference("text")}),
        &["schemes", "hosts"],
    );
    let common = json!({"cwd":reference("path"),"environment":reference("environment")});
    let argv = json!({"arguments":reference("arguments"),"targets":one_of(vec![reference("target"),list(reference("target"),1)]),"possible_effects":{"type":"array","minItems":1,"maxItems":11,"items":{"enum":["repository-write","worktree-write","data-removal","history-rewrite","network","code-execution","daemon-write","image-build","host-exposure","privileged-runtime","configuration"]}}});
    let shell = json!({"source":reference("text"),"dialect_version":one_of(vec![nonempty(4096),object(json!({"exact":nonempty(4096)}),&["exact"])])});
    let file = json!({"sources":reference("quantified"),"destination":reference("path"),"recursive":{"type":"boolean"},"targets":one_of(vec![reference("target"),list(reference("target"),1)])});
    let web = json!({"query":reference("text"),"url":reference("url"),"method":reference("method"),"request_fields":reference("field_map")});
    let mcp = json!({"arguments":{"type":"object"},"fields":reference("field_map"),"connection":reference("field_map")});
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
            json!({"dialect":enum_text(&["posix","bash","powershell","cmd"])}),
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
            json!({"action":{"enum":["search","fetch"]}}),
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
    let rule = object(
        json!({"id":{"type":"string","minLength":1,"maxLength":128},"decision":{"enum":["allow","ask","deny"]},"conditions":reference("conditions"),"requirements":reference("requirements"),"on_unmet":object(json!({"decision":{"enum":["ask","deny"]},"message":message,"alternatives":alternatives}),&["decision"]),"reason":nonempty(4096),"guidance":reference("guidance"),"expires_at":{"type":"string","format":"date-time"}}),
        &["id", "decision", "conditions"],
    );
    json!({"guidance":guidance,"rule_custom":rule})
}
pub(crate) fn definitions() -> Value {
    let mut value = base();
    for section in [arguments(), domains(), rule()] {
        if let Value::Object(fields) = section {
            value.as_object_mut().expect("schema object").extend(fields);
        }
    }
    value
}
