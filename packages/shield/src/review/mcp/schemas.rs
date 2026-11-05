//! Canonical tool inventory and strict wire schemas, independent of operator authority.
use serde_json::{Value, json};
fn object(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({"type":"object","required":required,"additionalProperties":false});
    schema["properties"] = properties;
    schema
}
fn string() -> Value {
    json!({"type":"string","minLength":1,"maxLength":4096})
}
fn id() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128})
}
fn nullable(value: Value) -> Value {
    let mut schema = json!({});
    schema["anyOf"] = Value::Array(vec![value, json!({"type":"null"})]);
    schema
}
fn arbitrary() -> Value {
    json!({"type":"object"})
}
fn strings() -> Value {
    json!({"type":"array","maxItems":256,"items":{"type":"string","maxLength":4096}})
}
fn context() -> Value {
    object(
        json!({"cwd":string(),"environment":nullable(json!({"type":"object","additionalProperties":{"type":"string"}}))}),
        &["cwd"],
    )
}
fn variant(kind: &str, mut properties: Value, required: &[&str]) -> Value {
    properties["kind"] = json!({"const":kind});
    properties["original_input"] = arbitrary();
    let mut keys = vec!["kind", "original_input"];
    keys.extend_from_slice(required);
    object(properties, &keys)
}
fn input() -> Value {
    json!({"oneOf":[
        variant("argv",json!({"executable":string(),"arguments":strings()}),&["executable","arguments"]),
        variant("shell",json!({"source":{"type":"string","maxLength":65536},"dialect":{"enum":["bash","posix","powershell","cmd"]},"dialect_version":nullable(string())}),&["source","dialect"]),
        variant("file",json!({"action":{"enum":["read","write","copy","move","remove"]},"sources":strings(),"destination":nullable(string()),"recursive":{"type":"boolean"}}),&["action","sources","recursive"]),
        variant("web",json!({"action":{"enum":["search","fetch"]},"query":nullable(string()),"url":nullable(string()),"method":nullable(string()),"conditions":arbitrary()}),&["action","conditions"]),
        variant("mcp",json!({"server":string(),"tool":string(),"arguments":arbitrary()}),&["server","tool","arguments"]),
        variant("unknown",json!({"host_tool":string()}),&["host_tool"])
    ]})
}
fn rule_ref() -> Value {
    json!({"oneOf":[object(json!({"scope":{"const":"custom"},"rule_id":id()}),&["scope","rule_id"]),object(json!({"scope":{"const":"pack"},"pack_selector":id(),"rule_id":id()}),&["scope","pack_selector","rule_id"])]})
}
fn rule() -> Value {
    json!({"$ref":"#/$defs/rule_custom"})
}
fn layer() -> Value {
    json!({"enum":["project","local"]})
}
fn change() -> Value {
    let mut variants = vec![];
    for (kind, mut fields, mut required) in [
        ("project_files_adopt", json!({}), vec![]),
        (
            "rule_add",
            json!({"rule":rule(),"layer":layer()}),
            vec!["rule"],
        ),
        (
            "rule_edit",
            json!({"rule_id":id(),"rule":rule(),"layer":layer()}),
            vec!["rule_id", "rule"],
        ),
        (
            "rule_remove",
            json!({"rule_id":id(),"layer":layer()}),
            vec!["rule_id"],
        ),
        (
            "pack_add",
            json!({"reference":string(),"profile":{"enum":["loose","recommended","strict"]},"layer":layer()}),
            vec!["reference"],
        ),
        ("pack_update", json!({"reference":string()}), vec![]),
        (
            "pack_remove",
            json!({"reference":string()}),
            vec!["reference"],
        ),
        (
            "pack_configure",
            json!({"reference":string(),"set":{"type":"object","maxProperties":64},"unset":{"type":"array","maxItems":64,"items":string()},"override":{"type":"object","maxProperties":1024},"clear_override":{"type":"array","maxItems":1024,"items":string()},"layer":layer()}),
            vec!["reference"],
        ),
        (
            "access_path_add",
            json!({"access":{"enum":["read","write"]},"path":string(),"layer":layer()}),
            vec!["access", "path"],
        ),
        (
            "access_path_remove",
            json!({"access":{"enum":["read","write"]},"path":string(),"layer":layer()}),
            vec!["access", "path"],
        ),
    ] {
        fields["kind"] = json!({"const":kind});
        required.push("kind");
        variants.push(object(fields, &required));
    }
    json!({"oneOf":variants})
}
fn paging(mut properties: Value) -> Value {
    properties["cursor"] = id();
    properties["cursor"]["description"] = json!(
        "Copy next_cursor from the previous result; keep the same tool, connection, filters and limit, including omitted fields."
    );
    properties["limit"] = json!({"type":"integer","minimum":1,"maximum":100});
    properties
}
fn selectable(selector: &str, properties: Value, selector_type: Value) -> Value {
    let mut properties = paging(properties);
    properties[selector] = selector_type;
    let mut schema = object(properties, &[]);
    // Claude omits tools with a top-level oneOf/anyOf/allOf. Keep the same
    // selector exclusivity without a union at the tool schema's root.
    schema["if"] = json!({"required":[selector]});
    schema["then"] = json!({"maxProperties":1});
    schema
}
fn rules_selection() -> Value {
    let mut schema = selectable("rule_ref", json!({"candidate_id":id()}), rule_ref());
    schema["then"] = object(json!({"rule_ref":rule_ref(),"candidate_id":id()}), &[]);
    schema
}
pub(super) fn tools(agent: Option<&str>) -> Value {
    let output = object(
        json!({"schema_version":{"const":1},"project_id":string(),"policy_revision":nullable(string()),"data":{},"missing":{"type":"array"},"truncated":{"type":"boolean"},"redacted":{"type":"boolean"},"error":nullable(object(json!({"code":string(),"message":string(),"operator_route":nullable(string()),"alternatives":{"type":"array","maxItems":crate::review::MAX_ALTERNATIVES,"items":string()},"alternatives_total":{"type":"integer","minimum":0},"pack":string(),"dependents":{"type":"array","maxItems":crate::review::MAX_ALTERNATIVES,"items":string()},"dependents_total":{"type":"integer","minimum":0},"retry_arguments":object(json!({"request_ids":{"type":"array","minItems":1,"maxItems":8,"uniqueItems":true,"items":id()}}), &["request_ids"]),"remaining_request_ids":{"type":"array","minItems":1,"maxItems":7,"uniqueItems":true,"items":id()}}),&["code","message","operator_route"]))}),
        &[
            "schema_version",
            "project_id",
            "policy_revision",
            "data",
            "missing",
            "truncated",
            "error",
        ],
    );
    let tools=inventory().into_iter().map(|(name,description,schema)|{
        let mut schema = schema();
        if name == "policy_apply" {
            if let Some(agent) = agent {
                schema = crate::agents::review::resolve(Some(agent)).apply_schema().unwrap_or(schema);
            } else {
                // Client identity is negotiated after this catalog is built.
                // Validation still accepts only the eventual resolved adapter.
                let mut variants = vec![schema];
                for adapter in crate::agents::ADAPTERS {
                    if let Some(schema) = adapter.review().apply_schema() {
                        variants.push(schema);
                    }
                }
                schema = json!({"type":"object","oneOf":variants});
            }
        }
        if name == "policy_propose" { let mut definitions = crate::policy::schema::definitions(); definitions["normalized_input"] = input(); schema["$defs"] = definitions; }
        let mutation=["policy_propose","pack_configure","pack_add","policy_withdraw","policy_apply","review_defer","request_approve"].contains(&name);
        json!({"name":name,"description":description,"inputSchema":schema,"outputSchema":output,"annotations":{"readOnlyHint":!mutation,"destructiveHint":(["policy_withdraw","policy_apply"].contains(&name)),"idempotentHint":!["pack_configure","pack_add"].contains(&name),"openWorldHint":(["pack_search","pack_info","pack_add","policy_propose"].contains(&name))}})
    }).chain(super::documentation::tools().into_iter().map(|mut tool| {
        // The shared definitions carry their own schemas and annotations;
        // results arrive in Shield's envelope.
        tool["outputSchema"] = output.clone();
        tool
    })).collect::<Vec<_>>();
    json!({"tools":tools})
}
/// One change, or 1–16 to approve together.
fn policy_propose() -> Definition {
    (
        "policy_propose",
        "Prepare an inactive proposal from 1-16 changes for atomic policy_apply approval. Use pack_add for guided settings. Apply project_files_adopt separately before preparing other changes. After a deadline, inspect policy_show before retrying identical arguments on this connection. Report unresolved failures; never retry cancellation.",
        || {
            let mut schema = object(
                json!({"change":change(),"changes":{"type":"array","minItems":1,"maxItems":16,"items":change()},"idempotency_key":id(),"agent_reason":string(),"source_entry_id":id(),"review_ids":{"type":"array","maxItems":16,"items":{"type":"string","minLength":64,"maxLength":64}},"supersedes_proposal_id":id()}),
                &[],
            );
            schema["if"] = json!({"required":["change"]});
            schema["then"] = json!({"not":{"required":["changes"]}});
            schema["else"] = json!({"required":["changes"]});
            schema["examples"] = proposal_examples();
            schema
        },
    )
}

/// In-session approval of one refused request, or up to eight from one
/// session in one form; the person answers Shield's form.
fn request_approve() -> Definition {
    (
        "request_approve",
        "Request user approval for 1-8 still-needed requests from this session. Forms are limited to 12,000 bytes; oversized groups return retry_arguments and remaining_request_ids. After approval, repeat each identical call once.",
        || {
            let mut schema = object(
                json!({"request_id":id(),"request_ids":{"type":"array","minItems":1,"maxItems":8,"uniqueItems":true,"items":id()}}),
                &[],
            );
            schema["if"] = json!({"required":["request_id"]});
            schema["then"] = json!({"not":{"required":["request_ids"]}});
            schema["else"] = json!({"required":["request_ids"]});
            schema["examples"] = json!([
                {"request_id":"hook-request-acme-1"},
                {"request_ids":["hook-request-acme-1","hook-request-acme-2"]}
            ]);
            schema
        },
    )
}

pub(super) fn proposal_examples() -> Value {
    json!([
        {"change":{"kind":"rule_add","rule":{
            "id":"allow-reviewed-dev","decision":"allow",
            "conditions":{"domain":"argv","executable":"npm",
                "arguments":{"kind":"options","prefix":["run","dev"],"flags":[],"values":[],"operands":false},
                "cwd":{"exact":{"root":"project"}}}}},
            "agent_reason":"Example only: review dev, predev and postdev scripts and executed code before offering this project-root command. Script edits will also execute under this permission."},
        {"change":crate::proposal_draft::web_fetch("http://localhost:3000/health").expect("valid example URL"),
            "agent_reason":"Allow recurring GET/HEAD requests to local servers on any port, without following redirects."},
        {"changes":[
            {"kind":"pack_add","reference":"acme/git","profile":"recommended","layer":"local"},
            {"kind":"pack_add","reference":"acme/node","profile":"recommended","layer":"local"}
        ],"agent_reason":"Example only: inspect both packs before proposing them together. Use guided pack_add when settings need user input."}
    ])
}

type Definition = (&'static str, &'static str, fn() -> Value);

pub(super) fn metadata() -> impl Iterator<Item = (&'static str, &'static str)> {
    inventory()
        .into_iter()
        .map(|(name, description, _)| (name, description))
}

fn inventory() -> Vec<Definition> {
    vec![
        (
            "status",
            "Show policy status and protection coverage",
            || object(json!({}), &[]),
        ),
        ("doctor", "Diagnose setup and coverage problems", || {
            object(json!({}), &[])
        }),
        ("pack_list", "List installed packs, or show one", || {
            selectable("reference", json!({}), string())
        }),
        (
            "registry_list",
            "List the project's effective pack registries, or with scope user the user's own",
            || {
                object(
                    json!({"scope":{"enum":["project","user"],"default":"project"}}),
                    &[],
                )
            },
        ),
        ("pack_search", "Search registries for packs", || {
            object(paging(json!({"query":string()})), &["query"])
        }),
        pack_info(),
        pack_add(),
        pack_configure(),
        (
            "policy_show",
            "Show active policy, or select a proposal by proposal_id to retrieve its status and apply_arguments",
            || {
                selectable(
                    "proposal_id",
                    json!({"agent":string(),"session_id":string(),"status":{"enum":["pending","applied","rejected","withdrawn","stale","failed"]}}),
                    id(),
                )
            },
        ),
        (
            "rule_list",
            "List rules, or show one by rule_ref; candidate_id selects a retained snapshot",
            rules_selection,
        ),
        policy_propose(),
        suggestions_tool(),
        defer_tool(),
        (
            "policy_withdraw",
            "Withdraw your own pending proposal",
            || object(json!({"proposal_id":id()}), &["proposal_id"]),
        ),
        (
            "policy_apply",
            "Activate a proposal with per-call user approval. Copy its returned apply_arguments unchanged.",
            || {
                object(
                    json!({"proposal_id":id(),"candidate_digest":string(),"changes":{"type":"array","maxItems":4096,"items":{"type":"object"}},"review":object(json!({"packs":{"type":"object"},"comparison":{"type":"object"}}), &["packs", "comparison"])}),
                    &["proposal_id", "candidate_digest", "changes", "review"],
                )
            },
        ),
        (
            "policy_receipt",
            "Show a proposal's status and application receipt, if any",
            || object(json!({"proposal_id":id()}), &["proposal_id"]),
        ),
        (
            "request_list",
            "List or show pending action requests; only the user can grant them",
            || selectable("request_id", json!({}), id()),
        ),
        request_approve(),
        (
            "history_list",
            "List or show past requests and their decisions",
            || {
                selectable(
                    "entry_id",
                    json!({"denied":{"type":"boolean"},"approved":{"type":"boolean"},"agent":string()}),
                    id(),
                )
            },
        ),
        (
            "policy_evaluate",
            "Preview a request's policy decision without executing or authorizing it",
            || {
                object(
                    json!({"input":input(),"context":context(),"candidate_id":id()}),
                    &["input", "context"],
                )
            },
        ),
        (
            "policy_compare",
            "Compare two policy snapshots without authorizing execution",
            comparison,
        ),
    ]
}

fn pack_add() -> Definition {
    (
        "pack_add",
        "Prepare 1-16 packs and dependencies as one inactive proposal. Inspect pack_info; request declared variables needed for the user's restrictions, otherwise keep defaults. Report unsupported restrictions instead of broadening scope. A form collects required/requested values; never invent answers or retry cancellation. Copy apply_arguments unchanged to policy_apply for approval. Unsupported forms require shield pack add in the user's terminal.",
        || {
            let variables =
                json!({"type":"array","minItems":1,"maxItems":16,"uniqueItems":true,"items":id()});
            let selection = json!({"reference":string(),"profile":{"enum":["loose","recommended","strict"],"description":"New selections default to recommended; installed selections keep their profile."},"variables":variables});
            let mut properties = selection.clone();
            properties["layer"] = layer();
            properties["layer"]["description"] = json!(
                "Storage layer for all selections; defaults to project. Local storage does not restrict remote targets."
            );
            properties["packs"] = json!({"type":"array","minItems":1,"maxItems":16,"items":object(selection, &["reference"])});
            let mut schema = object(properties, &[]);
            schema["if"] = json!({"required":["packs"]});
            schema["then"] = json!({"not":{"anyOf":[{"required":["reference"]},{"required":["profile"]},{"required":["variables"]}]}});
            schema["else"] = json!({"required":["reference"]});
            schema["examples"] = json!([{"packs":[{"reference":"acme/git"},{"reference":"acme/node","variables":["manager"]}],"layer":"local"}]);
            schema
        },
    )
}

fn pack_configure() -> Definition {
    (
        "pack_configure",
        "Collect 1-16 installed-pack settings through a form; prepare an inactive proposal. Inspect pack_list for names. Copy apply_arguments unchanged to policy_apply for approval. If forms are unavailable, collect values in chat and use policy_propose with pack_configure. Never invent answers or retry cancellation.",
        || {
            object(
                json!({"reference":string(),"variables":{"type":"array","minItems":1,"maxItems":16,"items":id()},"layer":layer()}),
                &["reference", "variables"],
            )
        },
    )
}

fn pack_info() -> Definition {
    (
        "pack_info",
        "Inspect pack permissions and setup. Summary/settings include declared variables and pack_add arguments. Exact rule_id accepts view=rules or no view, without filters or paging. Installs nothing.",
        || {
            let mut schema = object(
                paging(
                    json!({"reference":string(),"view":{"enum":["summary","rules","settings"],"default":"summary"},"query":{"type":"string","minLength":1,"maxLength":4096,"description":"Filter rule IDs and reasons in the rules view."},"rule_id":id()}),
                ),
                &["reference"],
            );
            schema["if"] = json!({"required":["rule_id"]});
            schema["then"] = json!({"properties":{"view":{"const":"rules"}},"not":{"anyOf":[{"required":["query"]},{"required":["cursor"]},{"required":["limit"]}]}});
            schema
        },
    )
}

fn comparison() -> Value {
    let mut schema = object(
        json!({"before_candidate_id":id(),"after_candidate_id":id(),"domain":{"enum":["sample","literal_argv","supported_semantic"]},"input":input(),"context":context()}),
        &["before_candidate_id", "after_candidate_id", "domain"],
    );
    schema["if"] = json!({"properties":{"domain":{"const":"sample"}}});
    schema["then"] = json!({"required":["input","context"]});
    schema["else"] = json!({"not":{"anyOf":[{"required":["input"]},{"required":["context"]}]}});
    schema
}

fn suggestions_tool() -> Definition {
    (
        "suggestion_list",
        "Review recorded Ask requests, recurrence, coverage and next actions. Listing grants no permission.",
        || {
            object(
                json!({"session_id":id(),"cursor":id(),"limit":{"type":"integer","minimum":1,"maximum":100}}),
                &[],
            )
        },
    )
}
fn defer_tool() -> Definition {
    (
        "review_defer",
        "Defer unchanged reviews for this session; policy and approvals stay unchanged.",
        || {
            object(
                json!({"session_id":id(),"review_ids":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"string","minLength":64,"maxLength":64}}}),
                &["session_id", "review_ids"],
            )
        },
    )
}

#[cfg(test)]
mod metadata_tests {
    #[test]
    fn metadata_and_recognition_match_every_negotiated_catalog() {
        let summaries = crate::review::mcp::tool_summaries();
        for agent in std::iter::once(None).chain(
            crate::agents::ADAPTERS
                .iter()
                .map(|adapter| Some(adapter.spec().id)),
        ) {
            let catalog = crate::review::mcp::tools_for(agent);
            let listed: Vec<_> = catalog["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| {
                    let name = tool["name"].as_str().unwrap();
                    assert!(crate::review::mcp::is_tool(name), "{name}");
                    (
                        name.to_owned(),
                        tool["description"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            assert_eq!(summaries, listed);
        }
        for unknown in ["unknown", "policy_apply_extra", "shield_docs_unknown"] {
            assert!(!crate::review::mcp::is_tool(unknown));
        }
    }
}
