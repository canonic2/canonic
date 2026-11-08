//! Installed-pack questions and inactive proposals bound to the active revision.
use super::{Checkpoint, Connection, Context, Error, changes, fields, packs, required};
use canonic_mcp_kit::elicitation::{Form, Request};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct PackQuestion {
    pub(crate) tool: &'static str,
    pub(crate) needs_input: bool,
    pub(crate) question: Request,
    arguments: Value,
    revision: String,
    selector: String,
    installation: Option<packs::installation::Installation>,
    overrides: BTreeMap<String, String>,
    replay: Option<changes::Input>,
}

impl PackQuestion {
    pub(crate) fn prepare(context: &Context, tool: &str, args: &Value) -> Result<Self, Error> {
        if tool == "pack_add" {
            return Self::installation(context, args);
        }
        if tool == "pack_update" {
            return Self::update(context, args);
        }
        Self::configuration(context, args)
    }

    fn update(context: &Context, args: &Value) -> Result<Self, Error> {
        fields(args, &["reference", "idempotency_key", "agent_reason"])?;
        let reference = super::text(args, "reference")?;
        let mut proposal = json!({"change":{"kind":"pack_update"},
                "agent_reason":"Pack review prepared through guided MCP input; final approval required."});
        if let Some(reference) = reference {
            proposal["change"]["reference"] = json!(reference);
        }
        for key in ["idempotency_key", "agent_reason"] {
            if let Some(value) = args.get(key) {
                proposal[key] = value.clone();
            }
        }
        let input: changes::Input = serde_json::from_value(proposal)
            .map_err(|_| Error::Invalid("invalid pack update arguments"))?;
        if changes::applied_retry(context, &input)? {
            return Ok(Self {
                tool: "pack_update",
                needs_input: false,
                question: question(json!({}), &[], "Return the retained update receipt")?,
                arguments: args.clone(),
                revision: String::new(),
                selector: String::new(),
                installation: None,
                overrides: BTreeMap::new(),
                replay: Some(input),
            });
        }
        let installation = packs::installation::Installation::update(context, reference)?;
        Self::captured("pack_update", args, installation, "updated packs")
    }

    fn configuration(context: &Context, args: &Value) -> Result<Self, Error> {
        configuration_fields(args)?;
        let reference = required(args, "reference")?;
        if args
            .get("layer")
            .is_some_and(|layer| ![json!("project"), json!("local")].contains(layer))
        {
            return Err(Error::Invalid("choose project or local layer"));
        }
        let installed = packs::show(context, reference)?;
        let all_names: Vec<Value> = installed["artifact"]["variables"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, _)| json!(name))
            .collect();
        let names = match args.get("variables") {
            Some(value) => value
                .as_array()
                .ok_or(Error::Invalid("select variable names"))?,
            None => &all_names,
        };
        if names.len() > 16 || (args.get("variables").is_some() && names.is_empty()) {
            return Err(Error::Invalid("select 1-16 variables"));
        }
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
        let mut names = names.clone();
        let mut overrides = BTreeMap::new();
        if args.get("variables").is_none() {
            use crate::policy::resolution::targets::Kind;
            let artifact = serde_json::to_string(&installed["artifact"])
                .map_err(|_| Error::Invalid("pack artifact missing"))?;
            for target in crate::policy::override_targets(&artifact)? {
                let mut key = format!("override_{}", overrides.len());
                while properties.get(&key).is_some() {
                    key.push('_');
                }
                let mut choices = vec!["keep", "default", "off"];
                if matches!(target.kind, Kind::Rule { .. }) {
                    choices.extend(["allow", "ask", "deny"]);
                }
                if matches!(target.kind, Kind::Rule { unmet: true } | Kind::Precondition) {
                    choices.extend(["unmet-ask", "unmet-deny"]);
                }
                properties[&key] = json!({"type":"string","enum":choices,
                    "title":format!("Rule override: {}", target.id),
                    "description":format!("{}. keep preserves the current override; default uses the pack's decision.", target.detail)});
                names.push(json!(key));
                overrides.insert(key, target.id);
            }
        }
        if names.len() > 16 {
            return Err(Error::Invalid(
                "guided configuration supports at most 16 fields; select variables or use direct edits",
            ));
        }
        let question = question(
            properties,
            &names,
            &format!(
                "Choose settings for {reference} ({} layer). This prepares a proposal; final approval is separate. Do not enter credentials.",
                args["layer"].as_str().unwrap_or("project")
            ),
        )?;
        Ok(Self {
            tool: "pack_configure",
            needs_input: !names.is_empty(),
            question,
            arguments: args.clone(),
            revision: required(&installed, "policy_revision")?.into(),
            selector: required(&installed["pack"], "selector")?.into(),
            installation: None,
            overrides,
            replay: None,
        })
    }

    fn installation(context: &Context, args: &Value) -> Result<Self, Error> {
        fields(
            args,
            &["reference", "profile", "variables", "set", "packs", "layer"],
        )?;
        let selections: Vec<packs::installation::Selection> = if let Some(packs) = args.get("packs")
        {
            if ["reference", "profile", "variables", "set"]
                .iter()
                .any(|key| args.get(key).is_some())
            {
                return Err(Error::Invalid(
                    "supply reference or packs, without mixing selection fields",
                ));
            }
            serde_json::from_value(packs.clone())
                .map_err(|_| Error::Invalid("invalid pack selections"))?
        } else {
            required(args, "reference")?;
            let mut selection = args.clone();
            selection
                .as_object_mut()
                .ok_or(Error::Invalid("invalid selection"))?
                .remove("layer");
            vec![
                serde_json::from_value(selection)
                    .map_err(|_| Error::Invalid("invalid pack selection"))?,
            ]
        };
        if args
            .get("layer")
            .is_some_and(|layer| ![json!("project"), json!("local")].contains(layer))
        {
            return Err(Error::Invalid("choose project or local layer"));
        }
        let installation = packs::installation::Installation::prepare(
            context,
            &selections,
            args["layer"] == "local",
        )?;
        Self::captured(
            "pack_add",
            args,
            installation,
            &selections
                .iter()
                .map(|s| s.reference.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    fn captured(
        tool: &'static str,
        args: &Value,
        installation: packs::installation::Installation,
        title: &str,
    ) -> Result<Self, Error> {
        if installation.settings.len() > 16 {
            return Err(Error::Invalid(if tool == "pack_update" {
                "update needs more than 16 settings; use shield pack update in the terminal"
            } else {
                "installation needs more than 16 settings; supply typed values or use shield pack add in the terminal"
            }));
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
                "Choose settings for {} ({} layer). This prepares one proposal; final approval is separate. Do not enter credentials.",
                title,
                args["layer"].as_str().unwrap_or("project")
            ),
        )?;
        Ok(Self {
            tool,
            needs_input: !names.is_empty(),
            question,
            arguments: args.clone(),
            revision: installation.revision.clone(),
            selector: String::new(),
            installation: Some(installation),
            overrides: BTreeMap::new(),
            replay: None,
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
            connection.ensure_owner(context)?;
            if let Some(input) = &self.replay {
                let mut views = connection
                    .views
                    .lock()
                    .map_err(|_| Error::Invalid("connection state invalid"))?;
                let mut data = super::propose_input(context, &mut views, input, checkpoint)?;
                super::bound_policy_result(&mut data)?;
                return Ok(data);
            }
            let mut proposal = json!({
                "idempotency_key":key,
                "agent_reason":"Pack review prepared through guided MCP input; final approval required."
            });
            if self.tool == "pack_update"
                || (self.tool == "pack_configure" && self.arguments.get("variables").is_none())
            {
                if let Some(key) = self.arguments.get("idempotency_key") {
                    proposal["idempotency_key"] = key.clone();
                } else if self.tool == "pack_update"
                    && !self.needs_input
                    && let Some(fields) = proposal.as_object_mut()
                {
                    fields.remove("idempotency_key");
                }
                if let Some(reason) = self.arguments.get("agent_reason") {
                    proposal["agent_reason"] = reason.clone();
                }
            }
            if let Some(installation) = &self.installation {
                if installation.changes.len() == 1 {
                    proposal["change"] = installation.changes[0].clone();
                } else {
                    proposal["changes"] = json!(installation.changes);
                }
            } else {
                proposal["change"] = self.configuration_change(values)?;
            }
            // A bare guided configure on a pack without variables is an
            // unchanged inspection, rather than an invalid empty edit.
            if self.installation.is_none()
                && ["set", "override", "clear_override"].iter().all(|key| {
                    proposal["change"][key].as_object().map_or_else(
                        || {
                            proposal["change"][key]
                                .as_array()
                                .is_some_and(Vec::is_empty)
                        },
                        serde_json::Map::is_empty,
                    )
                })
            {
                let installed = packs::show(context, &self.selector)?;
                if installed["policy_revision"] != self.revision {
                    return Err(Error::StaleSnapshot);
                }
                return Ok(
                    json!({"unchanged":true,"committed":false,"policy_revision":self.revision}),
                );
            }
            let mut input: changes::Input = serde_json::from_value(proposal)
                .map_err(|_| Error::Invalid("invalid pack settings answer"))?;
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
                    "Policy or review context changed. Call {} again so the user can re-answer; do not reuse old answers",
                    self.tool
                ),
                Some(self.tool),
            ),
            result => super::result_envelope(context, "policy_propose", result.map_err(Into::into)),
        }
    }

    fn configuration_change(&self, values: &Value) -> Result<Value, Error> {
        let mut settings = values.clone();
        let mut overrides = json!({});
        let mut clear = Vec::new();
        for (key, rule) in &self.overrides {
            let value = settings
                .as_object_mut()
                .and_then(|settings| settings.remove(key))
                .ok_or(Error::Invalid("answer every rule override"))?;
            match value.as_str() {
                Some("keep") => {}
                Some("default") => clear.push(json!(rule)),
                Some("off" | "allow" | "ask" | "deny") => overrides[rule] = value,
                Some("unmet-ask") => overrides[rule] = json!({"on_unmet":"ask"}),
                Some("unmet-deny") => overrides[rule] = json!({"on_unmet":"deny"}),
                _ => return Err(Error::Invalid("invalid rule override choice")),
            }
        }
        let mut change = json!({"kind":"pack_configure","reference":self.selector,"set":settings,
            "override":overrides,"clear_override":clear});
        if let Some(layer) = self.arguments.get("layer") {
            change["layer"] = layer.clone();
        }
        Ok(change)
    }
}

fn configuration_fields(args: &Value) -> Result<(), Error> {
    fields(
        args,
        &[
            "reference",
            "variables",
            "layer",
            "idempotency_key",
            "agent_reason",
        ],
    )?;
    let mut metadata = json!({});
    for key in ["idempotency_key", "agent_reason"] {
        if let Some(value) = args.get(key) {
            metadata[key] = value.clone();
        }
    }
    let metadata: changes::Input =
        serde_json::from_value(metadata).map_err(|_| Error::Invalid("invalid proposal fields"))?;
    changes::validate_input(&metadata)?;
    if args.get("variables").is_some()
        && ["idempotency_key", "agent_reason"]
            .iter()
            .any(|key| args.get(key).is_some())
    {
        return Err(Error::Invalid(
            "variable forms do not accept proposal metadata",
        ));
    }
    Ok(())
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
