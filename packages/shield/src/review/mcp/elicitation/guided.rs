//! Input collection has no enrollment, removal or activation authority.
use super::{
    Checkpoint, Connection, Context, Error, PackQuestion, changes, fields, packs, question,
    required,
};
use crate::review::{project, storage::Store};
use canonic_mcp_kit::elicitation::Request;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

pub(crate) enum Question {
    Pack(Box<PackQuestion>),
    Guided(Box<Guided>),
}

pub(crate) struct Guided {
    tool: &'static str,
    question: Request,
    arguments: Value,
    binding: String,
    revision: String,
    selectors: Vec<String>,
}

impl Question {
    pub(crate) fn registry_binding(&self) -> Option<&str> {
        match self {
            Self::Guided(plan) if plan.tool == "registry_add" => Some(&plan.binding),
            _ => None,
        }
    }
    /// Captured policy sources retained while a pack-settings form is open.
    pub(crate) fn source_bytes(&self) -> usize {
        match self {
            Self::Pack(plan) => plan
                .installation
                .as_ref()
                .map_or(0, packs::installation::Installation::source_bytes),
            Self::Guided(_) => 0,
        }
    }

    pub(crate) fn prepare(context: &Context, tool: &str, args: &Value) -> Result<Self, Error> {
        if matches!(tool, "registry_add" | "pack_remove") {
            return Guided::prepare(context, tool, args).map(|plan| Self::Guided(Box::new(plan)));
        }
        PackQuestion::prepare(context, tool, args).map(|plan| Self::Pack(Box::new(plan)))
    }
    pub(crate) fn tool(&self) -> &'static str {
        match self {
            Self::Pack(plan) => plan.tool,
            Self::Guided(plan) => plan.tool,
        }
    }
    pub(crate) fn question(&self) -> &Request {
        match self {
            Self::Pack(plan) => &plan.question,
            Self::Guided(plan) => &plan.question,
        }
    }
    pub(crate) fn needs_input(&self) -> bool {
        match self {
            Self::Pack(plan) => plan.needs_input,
            Self::Guided(_) => true,
        }
    }
    pub(crate) fn propose(
        &self,
        context: &Context,
        connection: &Connection,
        values: &Value,
        key: &str,
        checkpoint: Checkpoint<'_>,
    ) -> Value {
        match self {
            Self::Pack(plan) => plan.propose(context, connection, values, key, checkpoint),
            Self::Guided(plan) => {
                let result = plan.complete(context, connection, values, key, checkpoint);
                if matches!(result, Err(Error::StaleSnapshot)) {
                    return super::super::refused(
                        context,
                        "stale_revision",
                        &format!(
                            "Policy or review context changed. Call {} again so the person can re-answer; do not reuse old answers",
                            self.tool()
                        ),
                        Some(self.tool()),
                    );
                }
                super::super::result_envelope(context, self.tool(), result.map_err(Into::into))
            }
        }
    }
}

impl Guided {
    fn prepare(context: &Context, tool: &str, args: &Value) -> Result<Self, Error> {
        let before = project::checkpoint(context)?;
        let (tool, properties, names, message, selectors) = if tool == "registry_add" {
            fields(args, &["name", "location", "scope"])?;
            let mut properties = json!({});
            let mut names = Vec::new();
            for (key, title, limit) in [
                ("name", "Registry alias", 128),
                (
                    "location",
                    "Local directory or HTTPS registry source (no credentials)",
                    4096,
                ),
            ] {
                if let Some(value) = args.get(key) {
                    if value
                        .as_str()
                        .is_none_or(|value| value.is_empty() || value.len() > limit)
                    {
                        return Err(Error::Invalid("invalid registry detail"));
                    }
                } else {
                    properties[key] =
                        json!({"type":"string","title":title,"minLength":1,"maxLength":limit});
                    names.push(json!(key));
                }
            }
            if args.get("scope").is_some()
                && ![json!("project"), json!("user")].contains(&args["scope"])
            {
                return Err(Error::Invalid("invalid registry scope"));
            }
            (
                "registry_add",
                properties,
                names,
                "Provide missing registry details. This does not discover or trust a source; a separate exact trust review follows.",
                Vec::new(),
            )
        } else {
            fields(args, &["idempotency_key", "agent_reason", "layer"])?;
            if args.get("layer").is_some() && args["layer"] != "project" {
                return Err(Error::Invalid(
                    "local pack removal requires an explicit reference",
                ));
            }
            let mut proposal = args.clone();
            proposal
                .as_object_mut()
                .expect("guided object")
                .remove("layer");
            let metadata: changes::Input = serde_json::from_value(proposal)
                .map_err(|_| Error::Invalid("invalid removal metadata"))?;
            changes::validate_input(&metadata)?;
            let selectors = before.operator["installed"]
                .as_array()
                .ok_or(Error::Invalid("installed packs missing"))?
                .iter()
                .map(|row| required(row, "selector").map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()?;
            if selectors.is_empty() || selectors.len() > 64 {
                return Err(Error::Invalid(
                    "guided removal needs 1-64 installed packs; supply an explicit reference",
                ));
            }
            let properties = json!({"references":{"type":"array","title":"Packs to remove","items":{"type":"string","enum":selectors},"minItems":1,"maxItems":selectors.len().min(16)}});
            (
                "pack_remove",
                properties,
                vec![json!("references")],
                "Select packs to remove. This prepares one inactive proposal; separate final approval is required. Remaining dependents can block removal.",
                selectors,
            )
        };
        Ok(Self {
            tool,
            question: question(properties, &names, message)?,
            arguments: args.clone(),
            binding: changes::prepared_binding(&before),
            revision: before.policy_revision,
            selectors,
        })
    }

    fn complete(
        &self,
        context: &Context,
        connection: &Connection,
        values: &Value,
        key: &str,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        checkpoint()?;
        connection.ensure_owner(context)?;
        let store = Store::open(Path::new(&context.project_root))?;
        let before = store.read_following_files()?;
        before.check(context)?;
        if changes::prepared_binding(&before) != self.binding
            || before.policy_revision != self.revision
        {
            return Err(Error::StaleSnapshot);
        }
        if self.tool == "registry_add" {
            let mut arguments = self.arguments.clone();
            for (key, value) in values
                .as_object()
                .ok_or(Error::Invalid("registry answers missing"))?
            {
                if !matches!(key.as_str(), "name" | "location") || self.arguments.get(key).is_some()
                {
                    return Err(Error::Invalid(
                        "registry form cannot replace captured arguments or scope",
                    ));
                }
                arguments[key] = value.clone();
            }
            if arguments.get("scope").is_none() {
                arguments["scope"] = json!("project");
            }
            return Ok(
                json!({"status":"input_collected","retry_arguments":arguments,"next_action":"Call registry_add with these exact retry_arguments to discover the source and request separate human trust confirmation. Input submission grants no trust.","committed":false}),
            );
        }
        let selected = values["references"]
            .as_array()
            .filter(|rows| !rows.is_empty() && rows.len() <= 16)
            .ok_or(Error::Invalid("select 1-16 packs"))?;
        let mut references = BTreeSet::new();
        for value in selected {
            let value = value
                .as_str()
                .ok_or(Error::Invalid("invalid removal selector"))?;
            if !self.selectors.iter().any(|selector| selector == value)
                || !references.insert(value.to_owned())
            {
                return Err(Error::Invalid("unknown or duplicate removal selector"));
            }
        }
        let (before, _, _) = crate::review::storage::Store::read_for_comparison(before)?;
        let references = references.into_iter().collect::<Vec<_>>();
        let (candidate, metadata) = packs::prepare_remove_many(&before, &references)?;
        let mut arguments = self.arguments.clone();
        arguments
            .as_object_mut()
            .expect("guided object")
            .remove("layer");
        arguments["changes"] = json!(
            references
                .iter()
                .map(|reference| json!({"kind":"pack_remove","reference":reference}))
                .collect::<Vec<_>>()
        );
        if arguments.get("idempotency_key").is_none() {
            arguments["idempotency_key"] = json!(key);
        }
        let mut input: changes::Input = serde_json::from_value(arguments)
            .map_err(|_| Error::Invalid("invalid removal metadata"))?;
        input.expected_policy_revision = Some(self.revision.clone());
        input.prepared = Some(changes::Prepared {
            binding: self.binding.clone(),
            candidate,
            metadata,
            comparison: None,
        });
        let mut views = connection.views.lock().map_err(|_| Error::Locked)?;
        let mut data = super::super::propose_input(context, &mut views, &input, checkpoint)?;
        super::super::bound_policy_result(&mut data)?;
        Ok(data)
    }
}
