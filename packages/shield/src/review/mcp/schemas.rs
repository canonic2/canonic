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
        if ["policy_propose","rule_add","rule_edit","session_grant_propose"].contains(&name) { let mut definitions = crate::policy::schema::definitions(); definitions["normalized_input"] = input(); schema["$defs"] = definitions; }
        let mutation=super::super::admin::supports(name) || ["policy_propose","policy_defer","policy_draft_remove","rule_add","rule_edit","rule_remove","pack_configure","pack_add","pack_update","pack_remove","policy_withdraw","policy_apply","review_defer","request_approve","request_deny","session_grant_propose","session_grant_approve","session_grant_revoke"].contains(&name);
        json!({"name":name,"description":description,"inputSchema":schema,"outputSchema":output,"annotations":{"readOnlyHint":!mutation,"destructiveHint":(["policy_withdraw","policy_apply","protection_set","registry_remove","history_configure","policy_reject","mcp_setup"].contains(&name)),"idempotentHint":!["pack_configure","pack_add"].contains(&name),"openWorldHint":(["pack_search","pack_info","pack_add","pack_update","policy_propose","registry_add","mcp_setup"].contains(&name))}})
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
    [
        admin_inventory(),
        policy_inventory(),
        request_inventory(),
        rule_inventory(),
    ]
    .concat()
}

fn admin_inventory() -> Vec<Definition> {
    vec![
        (
            "cache_prepare",
            "Prepare derived cache from committed policy; changes no policy authority",
            || object(json!({}), &[]),
        ),
        (
            "protection_set",
            "Ask the user to change protected file access after an exact before/after review",
            || {
                let mut schema = object(
                    json!({"agent_settings_read":{"enum":["ask","allow"]},"agent_settings_change":{"enum":["block","allow"]},"shield_config_change":{"enum":["block","allow"]}}),
                    &[],
                );
                schema["minProperties"] = json!(1);
                schema
            },
        ),
        (
            "registry_add",
            "Return the terminal enrollment route without fetching the supplied location; the person runs shield registry add",
            || {
                object(
                    json!({"name":string(),"location":string(),"scope":{"enum":["project","user"],"default":"project"}}),
                    &["name", "location"],
                )
            },
        ),
        (
            "registry_remove",
            "Ask the user to remove a registry registration while preserving installed policy",
            || {
                object(
                    json!({"name":string(),"scope":{"enum":["project","user"],"default":"project"}}),
                    &["name"],
                )
            },
        ),
        (
            "registry_default",
            "Ask the user to select the default registry for future discovery and installation",
            || {
                object(
                    json!({"name":string(),"scope":{"enum":["project","user"],"default":"project"}}),
                    &["name"],
                )
            },
        ),
        (
            "policy_repair",
            "Ask the user to restore managed projections from committed active policy",
            || object(json!({}), &[]),
        ),
        (
            "history_configure",
            "Ask the user to change history retention or clear retained history",
            || {
                object(
                    json!({"enabled":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":10000},"purge":{"type":"boolean","default":false}}),
                    &[],
                )
            },
        ),
        (
            "policy_reject",
            "Ask the user to reject a pending proposal; distinct from withdrawing your own proposal",
            || {
                object(
                    json!({"proposal_id":id(),"reason":string()}),
                    &["proposal_id"],
                )
            },
        ),
        (
            "mcp_setup",
            "Review agent registration changes; dry_run shows the plan, otherwise the user confirms it",
            || {
                object(
                    json!({"agents":{"type":"array","maxItems":64,"uniqueItems":true,"items":string()},"remove":{"type":"boolean","default":false},"dry_run":{"type":"boolean","default":false},"scope":{"enum":["project"],"default":"project"}}),
                    &[],
                )
            },
        ),
    ]
}

fn policy_inventory() -> Vec<Definition> {
    let mut tools: Vec<Definition> = vec![
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
            "pack_show",
            "Show one installed pack's selection, rules and source",
            || object(json!({"reference":string()}), &["reference"]),
        ),
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
            "pack_update",
            "Prepare an inactive update of one installed pack, or all tracking packs when reference is omitted; pinned packs stay fixed. Copy returned apply_arguments unchanged to policy_apply for user approval.",
            || {
                object(
                    json!({"reference":string(),"idempotency_key":id(),"agent_reason":string()}),
                    &[],
                )
            },
        ),
        (
            "pack_remove",
            "Prepare an inactive removal of one installed pack using the CLI's dependency checks. Copy returned apply_arguments unchanged to policy_apply for user approval.",
            || {
                object(
                    json!({"reference":string(),"idempotency_key":id(),"agent_reason":string()}),
                    &["reference"],
                )
            },
        ),
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
    ];
    tools.extend(draft_inventory());
    tools.extend([
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
    ]);
    tools
}

fn draft_inventory() -> Vec<Definition> {
    vec![
        (
            "policy_defer",
            "Save intent only when the user explicitly asks to decide later; withdraw an optional owned proposal. To resume, prepare fresh; a draft grants no authority.",
            || {
                object(
                    json!({"intent":string(),"constraints":string(),"proposal_id":id(),"idempotency_key":id()}),
                    &["intent", "idempotency_key"],
                )
            },
        ),
        (
            "policy_draft_list",
            "Inspect explicitly deferred intent; drafts have no apply arguments and require fresh preparation.",
            || selectable("draft_id", json!({}), id()),
        ),
        (
            "policy_draft_remove",
            "Remove deferred intent; does not change policy or any committed receipt.",
            || object(json!({"draft_id":id()}), &["draft_id"]),
        ),
    ]
}

fn request_inventory() -> Vec<Definition> {
    vec![
        (
            "request_list",
            "List or show pending action requests; only the user can grant them",
            || selectable("request_id", json!({}), id()),
        ),
        request_approve(),
        (
            "request_show",
            "Inspect one stored request without changing its authority",
            || object(json!({"request_id":id()}), &["request_id"]),
        ),
        (
            "request_deny",
            "Ask the user to deny a pending request or revoke its unused grant",
            || object(json!({"request_id":id()}), &["request_id"]),
        ),
        ("history_show", "Inspect one retained history entry", || {
            object(json!({"entry_id":id()}), &["entry_id"])
        }),
        (
            "session_grant_list",
            "List retained session grants and candidates",
            || {
                object(
                    json!({"session_ref":id(),"cursor":id(),"limit":{"type":"integer","minimum":1,"maximum":64,"default":16}}),
                    &[],
                )
            },
        ),
        session_grant_show(),
        session_grant_propose(),
        (
            "session_grant_approve",
            "Ask the user to issue a retained conversation grant; copy apply_arguments unchanged",
            || {
                object(
                    json!({"proposal_id":id(),"candidate_digest":string()}),
                    &["proposal_id", "candidate_digest"],
                )
            },
        ),
    ]
}

fn rule_inventory() -> Vec<Definition> {
    vec![
        (
            "rule_add",
            "Prepare an inactive custom rule addition; apply with policy_apply after review",
            || {
                object(
                    json!({"rule":rule(),"layer":layer(),"idempotency_key":id(),"agent_reason":string()}),
                    &["rule"],
                )
            },
        ),
        (
            "rule_edit",
            "Prepare an inactive custom rule edit; apply with policy_apply after review",
            || {
                object(
                    json!({"rule_id":id(),"rule":rule(),"layer":layer(),"idempotency_key":id(),"agent_reason":string()}),
                    &["rule_id", "rule"],
                )
            },
        ),
        (
            "rule_remove",
            "Prepare an inactive custom rule removal; apply with policy_apply after review",
            || {
                object(
                    json!({"rule_id":id(),"layer":layer(),"idempotency_key":id(),"agent_reason":string()}),
                    &["rule_id"],
                )
            },
        ),
        (
            "session_grant_revoke",
            "Revoke your session grant or withdraw its pending proposal. Reduces authority only; another connection's grant needs the operator route.",
            || {
                let mut schema = object(json!({"grant_id":id(),"proposal_id":id()}), &[]);
                schema["minProperties"] = json!(1);
                schema["maxProperties"] = json!(1);
                schema
            },
        ),
        (
            "history_list",
            "List or show past requests and their decisions",
            || {
                selectable(
                    "entry_id",
                    json!({"denied":{"type":"boolean"},"approved":{"type":"boolean"},"agent":string(),"authorization_kind":{"enum":["session_grant"]},"grant_id":id()}),
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
        "Prepare 1-16 packs and dependencies as one inactive proposal. Inspect pack_info; supply known typed settings through set or collect declared required/requested values through a form. Preserve the user's restrictions and defaults; never invent answers or retry cancellation. Copy apply_arguments unchanged to policy_apply for approval. Forms have bounded supported fields; supplied values share CLI validation.",
        || {
            let variables =
                json!({"type":"array","minItems":1,"maxItems":16,"uniqueItems":true,"items":id()});
            let selection = json!({"reference":string(),"profile":{"enum":["loose","recommended","strict"],"description":"New selections default to recommended; installed selections keep their profile."},"variables":variables,"set":{"type":"object","maxProperties":64}});
            let mut properties = selection.clone();
            properties["layer"] = layer();
            properties["layer"]["description"] = json!(
                "Storage layer for all selections; defaults to project. Local storage does not restrict remote targets."
            );
            properties["packs"] = json!({"type":"array","minItems":1,"maxItems":16,"items":object(selection, &["reference"])});
            let mut schema = object(properties, &[]);
            schema["if"] = json!({"required":["packs"]});
            schema["then"] = json!({"not":{"anyOf":[{"required":["reference"]},{"required":["profile"]},{"required":["variables"]},{"required":["set"]}]}});
            schema["else"] = json!({"required":["reference"]});
            schema["examples"] = json!([{"packs":[{"reference":"acme/git"},{"reference":"acme/node","variables":["manager"]}],"layer":"local"}]);
            schema
        },
    )
}

fn session_grant_show() -> Definition {
    (
        "session_grant_show",
        "Inspect reusable session grants, pending proposals, expiry and capability. A native hook supplies session_ref; a supplied conversation name grants no authority.",
        || {
            let mut schema = object(
                json!({"session_ref":id(),"proposal_id":id(),"grant_id":id(),"cursor":id(),"limit":{"type":"integer","minimum":1,"maximum":64,"default":16}}),
                &[],
            );
            schema["if"] =
                json!({"anyOf":[{"required":["proposal_id"]},{"required":["grant_id"]}]});
            schema["then"] = json!({"maxProperties":1});
            schema
        },
    )
}

fn session_grant_propose() -> Definition {
    (
        "session_grant_propose",
        "Prepare temporary permission for a known batch or constrained argv family before repeated privileged operations. Inspect session_grant_show first. Default duration is two hours; project policy stays unchanged. Named Deny overrides are explicit, and the user confirms session_grant_approve. Do not repeat declined offers.",
        || {
            let definitions = crate::policy::schema::definitions();
            let conditions = definitions["rule_custom"]["properties"]["conditions"].clone();
            let requirements = definitions["rule_custom"]["properties"]["requirements"].clone();
            let selector = object(
                json!({"conditions":{"allOf":[conditions,{"type":"object","properties":{"domain":{"const":"argv"}}}]},"requirements":requirements}),
                &["conditions"],
            );
            object(
                json!({"session_ref":id(),"idempotency_key":id(),"scope":{"type":"array","minItems":1,"maxItems":32,"items":selector},
            "duration_seconds":{"type":"integer","minimum":60,"maximum":28800,"default":7200},"reason":{"type":"string","maxLength":1024},
            "override_denies":{"type":"array","maxItems":32,"uniqueItems":true,"items":object(json!({"rule_ref":rule_ref(),"branch":{"const":"met"},"rule_digest":string()}),&["rule_ref","branch","rule_digest"])}}),
                &["session_ref", "idempotency_key", "scope"],
            )
        },
    )
}

fn pack_configure() -> Definition {
    (
        "pack_configure",
        "Configure an installed pack with set, unset, override and clear_override, or collect 1-16 variables through a form. Omit edits and variables for guided settings and rule overrides. Form and direct modes cannot be mixed. Prepare an inactive proposal; copy apply_arguments unchanged to policy_apply for approval. Never invent form answers or retry cancellation.",
        || {
            let mut schema = object(
                json!({"reference":string(),"variables":{"type":"array","minItems":1,"maxItems":16,"items":id()},"set":{"type":"object","maxProperties":64},"unset":{"type":"array","maxItems":64,"items":string()},"override":{"type":"object","maxProperties":1024},"clear_override":{"type":"array","maxItems":1024,"items":string()},"layer":layer(),"idempotency_key":id(),"agent_reason":string()}),
                &["reference"],
            );
            schema["if"] = json!({"required":["variables"]});
            let direct = [
                "set",
                "unset",
                "override",
                "clear_override",
                "idempotency_key",
                "agent_reason",
            ]
            .map(|key| json!({"required":[key]}));
            schema["then"] = json!({"not":{"anyOf":direct}});
            schema
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
        for unknown in ["unknown", "policy_apply_extra", "docs_unknown"] {
            assert!(!crate::review::mcp::is_tool(unknown));
        }
    }
}
