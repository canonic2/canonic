//! Folds pack variables, `when` tests, templates and rule overrides into ordinary
//! rules and preconditions before validation, so evaluation never sees a variable.
use super::{
    Decision, Error,
    model::PackDocument,
    rules::{Decisions, Precondition, Rule},
    templates,
    variables::{self, Declaration, Test},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) mod targets;
#[cfg(test)]
mod tests;

const MAX_OVERRIDES: usize = 1024;

/// Per-selection values and overrides applied when a pack is prepared.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Binding {
    /// Bound values by declared variable name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Value>,
    /// Rule, precondition and member overrides by source ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Override>,
}

impl Binding {
    pub(crate) fn is_empty(&self) -> bool {
        self.variables.is_empty() && self.overrides.is_empty()
    }
}

/// A per-rule change in one selection: off, a new decision, or new branch decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Override {
    /// The entry contributes nothing.
    Off,
    /// Replace the successful-branch decision.
    Decision(Decision),
    /// Replace either branch; the unmet branch accepts Ask or Deny only.
    Branches {
        /// Successful-branch decision.
        decision: Option<Decision>,
        /// Unmet-branch decision.
        on_unmet: Option<Decision>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum RawOverride {
    Word(String),
    Fields(Branches),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Branches {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decision: Option<Decision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_unmet: Option<Decision>,
}

impl<'de> Deserialize<'de> for Override {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        match RawOverride::deserialize(input)? {
            RawOverride::Word(word) => match word.as_str() {
                "off" => Ok(Self::Off),
                "allow" => Ok(Self::Decision(Decision::Allow)),
                "ask" => Ok(Self::Decision(Decision::Ask)),
                "deny" => Ok(Self::Decision(Decision::Deny)),
                _ => Err(D::Error::custom("override is off, allow, ask or deny")),
            },
            RawOverride::Fields(Branches { decision, on_unmet }) => {
                if decision.is_none() && on_unmet.is_none() {
                    return Err(D::Error::custom("override names a decision"));
                }
                Ok(Self::Branches { decision, on_unmet })
            }
        }
    }
}

impl Serialize for Override {
    fn serialize<S: serde::Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Off => RawOverride::Word("off".into()),
            Self::Decision(decision) => RawOverride::Word(
                match decision {
                    Decision::Allow => "allow",
                    Decision::Ask => "ask",
                    Decision::Deny => "deny",
                }
                .into(),
            ),
            Self::Branches { decision, on_unmet } => RawOverride::Fields(Branches {
                decision: *decision,
                on_unmet: *on_unmet,
            }),
        }
        .serialize(output)
    }
}

/// How one source entry resolved, for inspection.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) kind: &'static str,
    pub(crate) state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) variables: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) overridden: Option<Override>,
}

/// Whether a source document needs resolution before ordinary validation.
pub(super) fn dynamic(document: &PackDocument) -> bool {
    !document.variables.is_empty()
        || !document.preconditions.is_empty()
        || document.templates_enabled()
        || document.rules.iter().any(|rule| {
            let mut names = BTreeSet::new();
            let domain = rule.conditions["domain"].as_str().unwrap_or_default();
            variables::references(&rule.conditions, domain, &mut names);
            if let Some(requirements) = &rule.requirements {
                variables::references(requirements, domain, &mut names);
            }
            rule.when.is_some() || !names.is_empty()
        })
}

/// Capabilities that a dynamic source document uses beyond its resolved rules.
pub(super) struct Features {
    pub(super) variables: bool,
    pub(super) templates: bool,
    pub(super) patterns: BTreeSet<String>,
}

/// Resolve every entry with representative values, all activation tests holding,
/// so the result exercises every entry through ordinary validation.
pub(super) fn sample(document: &PackDocument) -> Result<(PackDocument, Features), Error> {
    variables::validate(&document.variables)?;
    let values = document
        .variables
        .iter()
        .map(|(name, declaration)| (name.clone(), declaration.sample()))
        .collect();
    let mut resolver = Resolver {
        declarations: &document.variables,
        values: &values,
        templates: document.templates_enabled(),
        sample: true,
        used: Used {
            variables: !document.variables.is_empty(),
            templates: false,
        },
    };
    let output = resolver.document(document.clone(), &BTreeMap::new(), &mut Vec::new())?;
    Ok((
        output,
        Features {
            variables: resolver.used.variables,
            templates: resolver.used.templates,
            patterns: document
                .variables
                .values()
                .fold(BTreeSet::new(), |mut found, d| {
                    d.capabilities(&mut found);
                    found
                }),
        },
    ))
}

/// Resolve a validated source document with one selection's binding. Required
/// variables without a value are reported as unset entries of kind `variable`.
pub(super) fn resolve(
    document: PackDocument,
    binding: &Binding,
) -> Result<(PackDocument, Vec<Entry>), Error> {
    let mut values = BTreeMap::new();
    let mut missing = Vec::new();
    for name in binding.variables.keys() {
        if !document.variables.contains_key(name) {
            return Err(Error::binding(name, "binding names an undeclared variable"));
        }
    }
    for (name, declaration) in &document.variables {
        if let Some(value) = binding.variables.get(name) {
            declaration.check(value).map_err(|error| match error {
                Error::Contract(reason) => Error::binding(name, reason),
                other => other,
            })?;
            values.insert(name.clone(), value.clone());
        } else if let Some(default) = &declaration.default {
            values.insert(name.clone(), default.clone());
        } else if declaration.required() {
            missing.push(Entry {
                id: name.clone(),
                kind: "variable",
                state: "unset",
                reason: Some("required".into()),
                variables: BTreeMap::new(),
                overridden: None,
            });
        }
    }
    let declarations = document.variables.clone();
    let mut resolver = Resolver {
        declarations: &declarations,
        values: &values,
        templates: document.templates_enabled(),
        sample: false,
        used: Used::default(),
    };
    let mut entries = missing;
    let output = resolver.document(document, &binding.overrides, &mut entries)?;
    Ok((output, entries))
}

struct Resolver<'a> {
    declarations: &'a BTreeMap<String, Declaration>,
    values: &'a BTreeMap<String, Value>,
    templates: bool,
    sample: bool,
    used: Used,
}

#[derive(Default)]
struct Used {
    variables: bool,
    templates: bool,
}

enum Activity {
    Active(BTreeSet<String>),
    Inactive(String),
}

impl Resolver<'_> {
    fn mcp_defaults(&mut self, document: &mut PackDocument) -> Result<(), Error> {
        for (name, integration) in &mut document.mcp_integrations {
            if !crate::mcp_connections::valid_name(name) {
                return Err(Error::Contract("invalid pack MCP integration name"));
            }
            if let Some(defaults) = &mut integration.defaults {
                fn references(value: &Value, names: &mut BTreeSet<String>) {
                    match value {
                        Value::Object(fields) => {
                            if let Some(name) = fields.get("var").and_then(Value::as_str) {
                                names.insert(name.into());
                            }
                            for child in fields.values() {
                                references(child, names);
                            }
                        }
                        Value::Array(items) => {
                            for child in items {
                                references(child, names);
                            }
                        }
                        _ => {}
                    }
                }
                references(defaults, &mut integration.default_variables);
                let available = self.connection_defaults(defaults)?;
                if available {
                    // Samples establish transport shape, not endpoint validity:
                    // an unbound string's representative value is just "sample".
                    integration.defaults_transport(name)?;
                }
                if !available || self.sample {
                    integration.defaults = None;
                }
            }
        }
        Ok(())
    }
    fn document(
        &mut self,
        mut document: PackDocument,
        overrides: &BTreeMap<String, Override>,
        entries: &mut Vec<Entry>,
    ) -> Result<PackDocument, Error> {
        validate_overrides(&document, overrides)?;
        self.mcp_defaults(&mut document)?;
        for (set, members) in &mut document.command_sets {
            members.retain(|_, member| {
                let off = overrides.get(member) == Some(&Override::Off);
                if off {
                    entries.push(Entry {
                        id: member.clone(),
                        kind: "member",
                        state: "off",
                        reason: Some(format!("member of {set} turned off")),
                        variables: BTreeMap::new(),
                        overridden: Some(Override::Off),
                    });
                }
                !off
            });
        }
        document
            .command_sets
            .retain(|_, members| !members.is_empty());
        let rules = std::mem::take(&mut document.rules);
        for mut rule in rules {
            if rule
                .command_set
                .as_ref()
                .is_some_and(|set| !document.command_sets.contains_key(set))
            {
                entries.push(inactive(&rule.id, "rule", "every member is off".into()));
                continue;
            }
            let overridden = overrides.get(&rule.id).cloned();
            if overridden == Some(Override::Off) {
                entries.push(off(&rule.id, "rule"));
                continue;
            }
            match self.rule(&mut rule)? {
                Activity::Inactive(reason) => entries.push(inactive(&rule.id, "rule", reason)),
                Activity::Active(names) => {
                    if let Some(change) = &overridden {
                        apply_rule(&mut rule, change);
                    }
                    if !names.is_empty() || overridden.is_some() {
                        entries.push(self.active(&rule.id, "rule", &names, overridden));
                    }
                    document.rules.push(rule);
                }
            }
        }
        let preconditions = std::mem::take(&mut document.preconditions);
        for mut precondition in preconditions {
            let overridden = overrides.get(&precondition.id).cloned();
            if overridden == Some(Override::Off) {
                entries.push(off(&precondition.id, "precondition"));
                continue;
            }
            match self.precondition(&mut precondition)? {
                Activity::Inactive(reason) => {
                    entries.push(inactive(&precondition.id, "precondition", reason));
                }
                Activity::Active(names) => {
                    if let Some(Override::Branches {
                        on_unmet: Some(decision),
                        ..
                    }) = &overridden
                    {
                        precondition.on_unmet.decision = Decisions::Scalar(*decision);
                    }
                    entries.push(self.active(&precondition.id, "precondition", &names, overridden));
                    document.preconditions.push(precondition);
                }
            }
        }
        let invocations: BTreeSet<_> = document
            .rules
            .iter()
            .filter_map(|r| r.invocation.clone())
            .collect();
        let sets: BTreeSet<_> = document
            .rules
            .iter()
            .filter_map(|r| r.command_set.clone())
            .collect();
        document
            .invocations
            .retain(|name, _| invocations.contains(name));
        document.command_sets.retain(|name, _| sets.contains(name));
        document.variables.clear();
        Ok(document)
    }

    fn active(
        &self,
        id: &str,
        kind: &'static str,
        names: &BTreeSet<String>,
        overridden: Option<Override>,
    ) -> Entry {
        Entry {
            id: id.into(),
            kind,
            state: "active",
            reason: None,
            variables: names
                .iter()
                .filter_map(|n| self.values.get(n).map(|v| (n.clone(), v.clone())))
                .collect(),
            overridden,
        }
    }

    fn rule(&mut self, rule: &mut Rule) -> Result<Activity, Error> {
        let domain = rule.conditions["domain"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let mut trees = vec![&mut rule.conditions];
        trees.extend(rule.requirements.as_mut());
        let mut texts: Vec<&mut String> = Vec::new();
        texts.extend(rule.reason.as_mut());
        if let Some(guidance) = rule.guidance.as_mut() {
            texts.extend(guidance.message.as_mut());
            texts.extend(guidance.alternatives.iter_mut().flatten());
        }
        if let Some(unmet) = rule.on_unmet.as_mut() {
            texts.extend(unmet.message.as_mut());
            texts.extend(unmet.alternatives.iter_mut().flatten());
        }
        let activity = self.entry(rule.when.take().as_deref(), &domain, trees, texts)?;
        Ok(activity)
    }

    /// Typed whole-value references, never textual command interpolation.
    fn connection_defaults(&mut self, value: &mut Value) -> Result<bool, Error> {
        match value {
            Value::Object(fields) if fields.contains_key("var") => {
                if fields.len() != 1 {
                    return Err(Error::Contract("MCP default reference has only var"));
                }
                let name = fields["var"].as_str().ok_or(Error::Syntax)?;
                if !self.declarations.contains_key(name) {
                    return Err(Error::Contract("MCP defaults name an undeclared variable"));
                }
                self.used.variables = true;
                let Some(resolved) = self.values.get(name) else {
                    return Ok(false);
                };
                *value = resolved.clone();
            }
            Value::Object(fields) => {
                for child in fields.values_mut() {
                    if !self.connection_defaults(child)? {
                        return Ok(false);
                    }
                }
            }
            Value::Array(items) => {
                for child in items {
                    if !self.connection_defaults(child)? {
                        return Ok(false);
                    }
                }
            }
            _ => {}
        }
        Ok(true)
    }

    fn precondition(&mut self, precondition: &mut Precondition) -> Result<Activity, Error> {
        let domain = precondition.domain().to_owned();
        let mut trees = vec![&mut precondition.conditions];
        trees.extend(precondition.requires_any.iter_mut());
        let mut texts: Vec<&mut String> = Vec::new();
        texts.extend(precondition.reason.as_mut());
        texts.extend(precondition.on_unmet.message.as_mut());
        texts.extend(precondition.on_unmet.alternatives.iter_mut().flatten());
        self.entry(precondition.when.take().as_deref(), &domain, trees, texts)
    }

    fn entry(
        &mut self,
        when: Option<&[Test]>,
        domain: &str,
        trees: Vec<&mut Value>,
        texts: Vec<&mut String>,
    ) -> Result<Activity, Error> {
        let mut names = BTreeSet::new();
        if let Some(tests) = when {
            variables::validate_tests(tests, self.declarations)?;
            self.used.variables = true;
            for test in tests {
                names.insert(test.variable().to_owned());
                if !self.sample && !test.holds(self.values.get(test.variable())) {
                    return Ok(Activity::Inactive(format!(
                        "when test on {} does not hold",
                        test.variable()
                    )));
                }
            }
        }
        let mut trees = trees;
        for tree in &mut trees {
            self.optional_checks(tree, domain, &mut names)?;
        }
        let mut unset = BTreeSet::new();
        for tree in trees {
            let mut referenced = BTreeSet::new();
            variables::references(tree, domain, &mut referenced);
            if referenced.is_empty() {
                continue;
            }
            self.used.variables = true;
            let lookup = |name: &str| -> Result<Option<Value>, Error> {
                let declaration = self
                    .declarations
                    .get(name)
                    .ok_or(Error::Contract("reference to an undeclared variable"))?;
                if declaration.kind == variables::Kind::Boolean {
                    return Err(Error::Contract("boolean variables cannot be selectors"));
                }
                Ok(self
                    .values
                    .get(name)
                    .and_then(|value| declaration.selector(value)))
            };
            variables::substitute(tree, domain, &lookup, &mut unset)?;
            names.extend(referenced);
        }
        if self.templates {
            for text in texts {
                if !templates::templated(text) {
                    continue;
                }
                self.used.templates = true;
                let mut lookup = |name: &str| -> Result<Option<String>, Error> {
                    if !self.declarations.contains_key(name) {
                        return Err(Error::Contract("template names an undeclared variable"));
                    }
                    names.insert(name.to_owned());
                    Ok(if self.sample {
                        Some("x".into())
                    } else {
                        self.values.get(name).map(variables::render)
                    })
                };
                match templates::render(text, &mut lookup)? {
                    Some(rendered) => *text = rendered,
                    None => {
                        unset.extend(
                            names
                                .iter()
                                .filter(|n| !self.values.contains_key(*n))
                                .cloned(),
                        );
                    }
                }
            }
        }
        if let Some(name) = unset.iter().next() {
            return Ok(Activity::Inactive(format!("{name} unset")));
        }
        Ok(Activity::Active(names))
    }

    /// Resolve the optional checks of every option grammar in a predicate tree:
    /// `{var, unset: any}` value matches and requirement items with `when`.
    fn optional_checks(
        &mut self,
        value: &mut Value,
        domain: &str,
        names: &mut BTreeSet<String>,
    ) -> Result<(), Error> {
        match value {
            Value::Object(fields) => {
                if fields.get("kind").and_then(Value::as_str) == Some("options") {
                    self.option_values(fields, names)?;
                    if let Some(Value::Object(leading)) = fields.get_mut("leading") {
                        self.option_values(leading, names)?;
                    }
                    for key in ["require_all", "require_any", "forbid"] {
                        self.requirement_items(fields, key, names)?;
                    }
                }
                for (key, child) in fields.iter_mut() {
                    // Environment selectors are text selectors, never option grammars.
                    if !(["exact", "input", "context", "environment"].contains(&key.as_str())
                        || domain == "mcp" && key == "arguments")
                    {
                        self.optional_checks(child, domain, names)?;
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.optional_checks(item, domain, names)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// An unset variable drops the value match; a set one becomes a plain reference.
    fn option_values(
        &mut self,
        grammar: &mut serde_json::Map<String, Value>,
        names: &mut BTreeSet<String>,
    ) -> Result<(), Error> {
        for key in ["values", "optional_values"] {
            let Some(Value::Array(items)) = grammar.get_mut(key) else {
                continue;
            };
            for item in items {
                let Some(declaration) = item.as_object_mut() else {
                    continue;
                };
                let Some(name) = declaration
                    .get("match")
                    .and_then(variables::optional_reference)
                    .map(str::to_owned)
                else {
                    continue;
                };
                if declaration["match"]["unset"] != "any" {
                    return Err(Error::Contract("unset accepts only any"));
                }
                let declared = self
                    .declarations
                    .get(&name)
                    .ok_or(Error::Contract("reference to an undeclared variable"))?;
                if declared.kind == variables::Kind::Boolean {
                    return Err(Error::Contract("boolean variables cannot be selectors"));
                }
                self.used.variables = true;
                if self.sample || self.values.contains_key(&name) {
                    declaration.insert("match".into(), serde_json::json!({ "var": name }));
                } else {
                    declaration.remove("match");
                }
                names.insert(name);
            }
        }
        Ok(())
    }

    /// Keep a `{name, when}` item as its name while every test holds; an empty
    /// list is removed, imposing no presence condition.
    fn requirement_items(
        &mut self,
        grammar: &mut serde_json::Map<String, Value>,
        key: &str,
        names: &mut BTreeSet<String>,
    ) -> Result<(), Error> {
        let items = match grammar.get_mut(key) {
            Some(Value::Array(items)) if items.iter().any(Value::is_object) => {
                std::mem::take(items)
            }
            _ => return Ok(()),
        };
        let mut kept = Vec::with_capacity(items.len());
        for item in items {
            let Value::Object(fields) = item else {
                kept.push(item);
                continue;
            };
            let (Some(Value::String(name)), Some(when), 2) =
                (fields.get("name"), fields.get("when"), fields.len())
            else {
                return Err(Error::Contract(
                    "a conditional option item has exactly name and when",
                ));
            };
            let tests: Vec<Test> =
                serde_json::from_value(when.clone()).map_err(|_| Error::Syntax)?;
            variables::validate_tests(&tests, self.declarations)?;
            self.used.variables = true;
            names.extend(tests.iter().map(|t| t.variable().to_owned()));
            if self.sample || tests.iter().all(|t| t.holds(self.values.get(t.variable()))) {
                kept.push(Value::String(name.clone()));
            }
        }
        if kept.is_empty() {
            grammar.remove(key);
        } else {
            grammar.insert(key.into(), Value::Array(kept));
        }
        Ok(())
    }
}

fn off(id: &str, kind: &'static str) -> Entry {
    Entry {
        id: id.into(),
        kind,
        state: "off",
        reason: Some("turned off by an override".into()),
        variables: BTreeMap::new(),
        overridden: Some(Override::Off),
    }
}

fn inactive(id: &str, kind: &'static str, reason: String) -> Entry {
    Entry {
        id: id.into(),
        kind,
        state: "inactive",
        reason: Some(reason),
        variables: BTreeMap::new(),
        overridden: None,
    }
}

fn apply_rule(rule: &mut Rule, change: &Override) {
    match change {
        Override::Off => {}
        Override::Decision(decision) => rule.decision = Decisions::Scalar(*decision),
        Override::Branches { decision, on_unmet } => {
            if let Some(decision) = decision {
                rule.decision = Decisions::Scalar(*decision);
            }
            if let (Some(decision), Some(unmet)) = (on_unmet, rule.on_unmet.as_mut()) {
                unmet.decision = Decisions::Scalar(*decision);
            }
        }
    }
}

fn validate_overrides(
    document: &PackDocument,
    overrides: &BTreeMap<String, Override>,
) -> Result<(), Error> {
    if overrides.len() > MAX_OVERRIDES {
        return Err(Error::Limit);
    }
    let members: BTreeSet<&str> = document
        .command_sets
        .values()
        .flat_map(|m| m.values().map(String::as_str))
        .collect();
    for (id, change) in overrides {
        let unmet = match change {
            Override::Branches { on_unmet, .. } => *on_unmet,
            _ => None,
        };
        if unmet == Some(Decision::Allow) {
            return Err(Error::Contract("on_unmet overrides are ask or deny"));
        }
        if let Some(rule) = document.rules.iter().find(|r| &r.id == id) {
            if unmet.is_some() && rule.on_unmet.is_none() {
                return Err(Error::Contract(
                    "on_unmet override for a rule without requirements",
                ));
            }
        } else if document.preconditions.iter().any(|p| &p.id == id) {
            if !matches!(
                change,
                Override::Off
                    | Override::Branches {
                        decision: None,
                        on_unmet: Some(_)
                    }
            ) {
                return Err(Error::Contract(
                    "preconditions accept off or on_unmet overrides",
                ));
            }
        } else if members.contains(id.as_str()) {
            if *change != Override::Off {
                return Err(Error::Contract("command members accept only off"));
            }
        } else {
            return Err(Error::Contract(
                "override names an ID the pack does not contain",
            ));
        }
    }
    Ok(())
}
