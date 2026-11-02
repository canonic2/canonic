//! Installed-pack questions and inactive proposals bound to the active revision.
use super::{Checkpoint, Connection, Context, Error, changes, fields, packs, required};
use canonic_mcp_kit::elicitation::{Form, Request};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) struct PackQuestion {
    pub(crate) tool: &'static str,
    pub(crate) needs_input: bool,
    pub(crate) question: Request,
    arguments: Value,
    revision: String,
    selector: String,
    installation: Option<packs::installation::Installation>,
}

impl PackQuestion {
    pub(crate) fn prepare(context: &Context, tool: &str, args: &Value) -> Result<Self, Error> {
        if tool == "pack_add" {
            return Self::installation(context, args);
        }
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
            properties[name] = field(declaration, name)?;
        }
        let question = question(
            properties,
            names,
            &format!(
                "Choose settings for {reference}. This prepares an inactive proposal in the {} layer; a separate final review is required. Do not enter credentials.",
                args["layer"].as_str().unwrap_or("project")
            ),
        )?;
        Ok(Self {
            tool: "pack_configure",
            needs_input: true,
            question,
            arguments: args.clone(),
            revision: required(&installed, "policy_revision")?.into(),
            selector: required(&installed["pack"], "selector")?.into(),
            installation: None,
        })
    }

    fn installation(context: &Context, args: &Value) -> Result<Self, Error> {
        fields(args, &["reference", "profile", "layer"])?;
        let reference = required(args, "reference")?;
        let profile = args
            .get("profile")
            .map(|p| {
                serde_json::from_value(p.clone())
                    .map_err(|_| Error::Invalid("choose loose, recommended or strict profile"))
            })
            .transpose()?;
        if args
            .get("layer")
            .is_some_and(|layer| ![json!("project"), json!("local")].contains(layer))
        {
            return Err(Error::Invalid("choose project or local layer"));
        }
        let installation = packs::installation::Installation::prepare(
            context,
            reference,
            profile,
            args["layer"] == "local",
        )?;
        if installation.settings.len() > 16 {
            return Err(Error::Invalid(
                "installation needs more than 16 settings; use shield pack add in the terminal",
            ));
        }
        let mut properties = json!({});
        let mut names = Vec::new();
        for setting in &installation.settings {
            properties[&setting.key] = field(
                &setting.declaration,
                &format!("{}: {}", setting.pack, setting.name),
            )?;
            names.push(json!(setting.key));
        }
        let question = question(
            properties,
            &names,
            &format!(
                "Supply required settings to install {reference} in the {} layer. Values apply to the captured verified pack releases and dependencies. Submission prepares an inactive proposal; separate final approval is required. Do not enter credentials.",
                args["layer"].as_str().unwrap_or("project")
            ),
        )?;
        Ok(Self {
            tool: "pack_add",
            needs_input: !names.is_empty(),
            question,
            arguments: args.clone(),
            revision: installation.revision.clone(),
            selector: String::new(),
            installation: Some(installation),
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
            let mut change = if self.installation.is_some() {
                let mut change = self.arguments.clone();
                change["kind"] = json!("pack_add");
                change
            } else {
                json!({"kind":"pack_configure","reference":self.selector,"set":values})
            };
            if let Some(layer) = self.arguments.get("layer") {
                change["layer"] = layer.clone();
            }
            let mut input: changes::Input = serde_json::from_value(json!({
                "change":change,"idempotency_key":key,
                "agent_reason":"Pack review prepared through guided MCP input; final approval required."
            })).map_err(|_| Error::Invalid("invalid pack settings answer"))?;
            input.expected_policy_revision = Some(self.revision.clone());
            input.prepared = self
                .installation
                .as_ref()
                .map(|installation| installation.complete(values))
                .transpose()?;
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
                &format!(
                    "Policy or review context changed while the form was open. Call {} again so the user can re-answer; do not reconstruct their answers",
                    self.tool
                ),
                Some(self.tool),
            ),
            result => super::result_envelope(context, "policy_propose", result.map_err(Into::into)),
        }
    }
}

fn field(declaration: &Value, title: &str) -> Result<Value, Error> {
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
                "variable cannot be represented as an MCP form; use shield pack add or shield pack configure in the terminal",
            ));
        }
    };
    field["title"] = json!(title);
    field["description"] = declaration["description"].clone();
    // Defaults are deliberately omitted: every selected field needs an explicit answer.
    Ok(field)
}

fn question(properties: Value, names: &[Value], message: &str) -> Result<Request, Error> {
    let mut schema = json!({"type":"object","required":names});
    schema["properties"] = properties;
    let form = Form::new(schema)
        .map_err(|_| Error::Invalid("pack settings exceed the supported MCP form contract"))?;
    Request::form(message, form).map_err(|_| Error::Invalid("cannot construct pack settings form"))
}
