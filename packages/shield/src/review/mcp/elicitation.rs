//! Installed-pack questions and inactive proposals bound to the active revision.
use super::{Checkpoint, Connection, Context, Error, changes, fields, packs, required};
use canonic_mcp_kit::elicitation::{Form, Request};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) struct PackQuestion {
    pub(crate) question: Request,
    arguments: Value,
    revision: String,
    selector: String,
}

impl PackQuestion {
    pub(crate) fn prepare(context: &Context, args: &Value) -> Result<Self, Error> {
        fields(args, &["reference", "variables", "layer"])?;
        let reference = required(args, "reference")?;
        if args
            .get("layer")
            .is_some_and(|layer| ![json!("project"), json!("local")].contains(layer))
        {
            return Err(Error::Invalid("choose project or local layer"));
        }
        let names = args["variables"]
            .as_array()
            .ok_or(Error::Invalid("select variable names"))?;
        if !(1..=16).contains(&names.len()) {
            return Err(Error::Invalid("select 1-16 variables"));
        }
        let installed = packs::show(context, reference)?;
        let mut properties = json!({});
        let mut seen = BTreeSet::new();
        for name in names {
            let name = name
                .as_str()
                .ok_or(Error::Invalid("invalid variable name"))?;
            if !seen.insert(name) {
                return Err(Error::Invalid("duplicate variable name"));
            }
            let declaration = installed["artifact"]["variables"].get(name).ok_or(
                Error::Invalid("unknown variable; inspect pack_list for this installed pack's variable definitions"),
            )?;
            let mut field = match declaration["type"].as_str() {
                Some("choice") => json!({"type":"string","enum":declaration["values"]}),
                Some("boolean") => json!({"type":"boolean"}),
                Some("string" | "path") => json!({"type":"string","minLength":1,"maxLength":4096}),
                Some("list") if declaration["items"]["type"] == "choice" => json!({
                    "type":"array","items":{"type":"string","enum":declaration["items"]["values"]},
                    "minItems":declaration["min_items"].as_u64().unwrap_or(1),
                    "maxItems":declaration["max_items"].as_u64().unwrap_or(256)}),
                _ => {
                    return Err(Error::Invalid(
                        "variable cannot be represented as an MCP form; use policy_propose or shield pack configure",
                    ));
                }
            };
            field["title"] = json!(name);
            field["description"] = declaration["description"].clone();
            // Defaults are deliberately omitted: every selected field needs an explicit answer.
            properties[name] = field;
        }
        let form = Form::new(json!({"type":"object","properties":properties,"required":names}))
            .map_err(|_| Error::Invalid("pack settings exceed the supported MCP form contract"))?;
        let question = Request::form(
            &format!("Choose settings for {reference}. This prepares an inactive proposal in the {} layer; a separate final review is required. Do not enter credentials.", args["layer"].as_str().unwrap_or("project")),
            form,
        ).map_err(|_| Error::Invalid("cannot construct pack settings form"))?;
        Ok(Self {
            question,
            arguments: args.clone(),
            revision: required(&installed, "policy_revision")?.into(),
            selector: required(&installed["pack"], "selector")?.into(),
        })
    }

    pub(crate) fn propose(
        &self,
        context: &Context,
        connection: &Connection,
        values: &Value,
        key: &str,
        checkpoint: Checkpoint<'_>,
    ) -> Value {
        let result: Result<Value, Error> = (|| {
            checkpoint()?;
            let mut change =
                json!({"kind":"pack_configure","reference":self.selector,"set":values});
            if let Some(layer) = self.arguments.get("layer") {
                change["layer"] = layer.clone();
            }
            let mut input: changes::Input = serde_json::from_value(json!({
                "change":change,"idempotency_key":key,
                "agent_reason":"Pack settings explicitly supplied through an MCP form; final approval required."
            })).map_err(|_| Error::Invalid("invalid pack settings answer"))?;
            input.expected_policy_revision = Some(self.revision.clone());
            let mut views = connection
                .views
                .lock()
                .map_err(|_| Error::Invalid("connection state invalid"))?;
            let mut data = super::propose_input(context, &mut views, &input, checkpoint)?;
            super::bound_policy_result(&mut data)?;
            Ok(data)
        })();
        match result {
            Err(Error::StaleSnapshot) => super::refused(
                context,
                "stale_revision",
                "Policy or review context changed while the form was open. Call pack_configure again so the user can re-answer; do not reconstruct their answers",
                Some("pack_configure"),
            ),
            result => super::result_envelope(context, "policy_propose", result.map_err(Into::into)),
        }
    }
}
