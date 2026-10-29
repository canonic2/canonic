//! Pack variable declarations, typed values, setup wording and `when` activation tests.
pub(crate) mod order;

use super::{Error, patterns, selectors};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const MAX_VARIABLES: usize = 64;
const MAX_TESTS: usize = 16;
const MAX_MEMBERS: usize = 256;

/// Value type of a pack variable.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Choice,
    String,
    List,
    Boolean,
    Path,
}

/// Layer setup suggests for a value.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Layer {
    Project,
    Local,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Within {
    Project,
    Anywhere,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Suggestion {
    pub(crate) file: String,
    pub(crate) value: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Items {
    #[serde(rename = "type")]
    kind: Kind,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    values: Option<Vec<String>>,
    #[serde(
        default,
        rename = "match",
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    matcher: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    max_length: Option<usize>,
}

/// A typed setting declared by a pack and bound by each project selection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Declaration {
    #[serde(rename = "type")]
    pub(crate) kind: Kind,
    pub(crate) description: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) default: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    required: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    layer: Option<Layer>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) suggest: Option<Vec<Suggestion>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) values: Option<Vec<String>>,
    #[serde(
        default,
        rename = "match",
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    matcher: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    max_length: Option<usize>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    items: Option<Items>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    min_items: Option<usize>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    max_items: Option<usize>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    within: Option<Within>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) setup: Option<Setup>,
}

/// Pack-authored wording for a variable's setup question.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) question: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) unset: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) value: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) placeholder: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) ask_when: Option<Vec<Test>>,
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Validate a pack's declarations, including defaults and suggestions.
pub(crate) fn validate(declarations: &BTreeMap<String, Declaration>) -> Result<(), Error> {
    if declarations.len() > MAX_VARIABLES {
        return Err(Error::Limit);
    }
    for (name, declaration) in declarations {
        if !valid_name(name) {
            return Err(Error::Contract("invalid variable name"));
        }
        declaration.validate()?;
        if let Some(setup) = &declaration.setup {
            setup.validate(name, declaration, declarations)?;
        }
    }
    Ok(())
}

/// Require `ask_when` tests to name only variables declared before their own,
/// in the pack's source `order`, so answers never depend on later questions.
pub(crate) fn validate_order(
    declarations: &BTreeMap<String, Declaration>,
    order: &[String],
) -> Result<(), Error> {
    for (position, name) in order.iter().enumerate() {
        let tests = declarations
            .get(name)
            .and_then(|d| d.setup.as_ref())
            .and_then(|s| s.ask_when.as_deref())
            .unwrap_or_default();
        if tests
            .iter()
            .any(|test| !order[..position].iter().any(|n| n == test.variable()))
        {
            return Err(Error::Contract(
                "ask_when tests only variables declared earlier",
            ));
        }
    }
    Ok(())
}

impl Setup {
    fn validate(
        &self,
        name: &str,
        declaration: &Declaration,
        declarations: &BTreeMap<String, Declaration>,
    ) -> Result<(), Error> {
        for (text, max) in [
            (&self.question, 256),
            (&self.unset, 64),
            (&self.value, 64),
            (&self.placeholder, 128),
        ] {
            if text
                .as_ref()
                .is_some_and(|t| t.trim().is_empty() || t.len() > max || t.contains('\0'))
            {
                return Err(Error::Contract("invalid variable setup text"));
            }
        }
        if (self.value.is_some() || self.placeholder.is_some()) && !declaration.typed() {
            return Err(Error::Contract(
                "setup value and placeholder apply to typed variables",
            ));
        }
        if self.unset.is_some() && declaration.required() {
            return Err(Error::Contract("required variables cannot be left unset"));
        }
        if let Some(placeholder) = &self.placeholder
            && crate::evidence::redact(&mut json!(placeholder))
        {
            return Err(Error::Contract(
                "variable placeholders cannot contain credentials",
            ));
        }
        if let Some(tests) = &self.ask_when {
            if declaration.required() {
                return Err(Error::Contract(
                    "ask_when needs an optional variable or a default",
                ));
            }
            if tests.iter().any(|test| test.variable() == name) {
                return Err(Error::Contract("ask_when cannot test its own variable"));
            }
            validate_tests(tests, declarations)?;
        }
        Ok(())
    }
}

impl Declaration {
    /// Whether activation refuses while the variable has no value.
    pub(crate) fn required(&self) -> bool {
        self.required == Some(true) && self.default.is_none()
    }

    /// Whether setup answers it by typing: strings, paths and free-text lists.
    pub(crate) fn typed(&self) -> bool {
        match self.kind {
            Kind::String | Kind::Path => true,
            Kind::List => self.items.as_ref().is_some_and(|i| i.kind == Kind::String),
            Kind::Choice | Kind::Boolean => false,
        }
    }

    /// The layer setup suggests, project unless declared local.
    pub(crate) fn layer(&self) -> Layer {
        self.layer.unwrap_or(Layer::Project)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.description.trim().is_empty() || self.description.len() > 512 {
            return Err(Error::Contract("invalid variable description"));
        }
        let only = |present: bool, kind: bool| {
            if present && !kind {
                Err(Error::Contract("variable field does not apply to its type"))
            } else {
                Ok(())
            }
        };
        only(self.values.is_some(), self.kind == Kind::Choice)?;
        only(
            self.matcher.is_some() || self.max_length.is_some(),
            self.kind == Kind::String,
        )?;
        only(
            self.items.is_some() || self.min_items.is_some() || self.max_items.is_some(),
            self.kind == Kind::List,
        )?;
        only(self.within.is_some(), self.kind == Kind::Path)?;
        match self.kind {
            Kind::Choice => members(self.values.as_ref())?,
            Kind::String => text_constraints(self.matcher.as_ref(), self.max_length)?,
            Kind::List => {
                let items = self
                    .items
                    .as_ref()
                    .ok_or(Error::Contract("list variables declare items"))?;
                match items.kind {
                    Kind::Choice if items.matcher.is_none() && items.max_length.is_none() => {
                        members(items.values.as_ref())?;
                    }
                    Kind::String if items.values.is_none() => {
                        text_constraints(items.matcher.as_ref(), items.max_length)?;
                    }
                    _ => return Err(Error::Contract("list items are choice or string")),
                }
                let (min, max) = self.bounds();
                if !(1..=MAX_MEMBERS).contains(&min)
                    || !(1..=MAX_MEMBERS).contains(&max)
                    || min > max
                {
                    return Err(Error::Contract("invalid list bounds"));
                }
            }
            Kind::Boolean | Kind::Path => {}
        }
        if let Some(default) = &self.default {
            self.check(default)?;
        }
        let suggestions = self.suggest.as_deref().unwrap_or_default();
        if suggestions.len() > 16 {
            return Err(Error::Limit);
        }
        for suggestion in suggestions {
            relative_path(&suggestion.file)?;
            self.check(&suggestion.value)?;
        }
        Ok(())
    }

    fn bounds(&self) -> (usize, usize) {
        (
            self.min_items.unwrap_or(1),
            self.max_items.unwrap_or(MAX_MEMBERS),
        )
    }

    /// Validate one bound value against the declared type and constraints.
    ///
    /// Values name things; recognized credentials are refused, never stored.
    pub(crate) fn check(&self, value: &Value) -> Result<(), Error> {
        match self.kind {
            Kind::Choice => choice(self.values.as_deref().unwrap_or_default(), value)?,
            Kind::String => string(value, self.matcher.as_ref(), self.max_length)?,
            Kind::Boolean => {
                if !value.is_boolean() {
                    return Err(Error::Contract("variable value has the wrong type"));
                }
            }
            Kind::Path => {
                let path = value
                    .as_str()
                    .ok_or(Error::Contract("variable value has the wrong type"))?;
                if path.len() > 4096 {
                    return Err(Error::Contract("variable value is too long"));
                }
                if std::path::Path::new(path).is_absolute() {
                    if self.within != Some(Within::Anywhere) {
                        return Err(Error::Contract("path variable must stay in the project"));
                    }
                    normalized(path.trim_start_matches('/'))?;
                } else {
                    normalized(path)?;
                }
            }
            Kind::List => {
                let items = value
                    .as_array()
                    .ok_or(Error::Contract("variable value has the wrong type"))?;
                let (min, max) = self.bounds();
                if items.len() < min || items.len() > max {
                    return Err(Error::Contract("list value is outside its bounds"));
                }
                let declared = self.items.as_ref().ok_or(Error::Syntax)?;
                let mut seen = BTreeSet::new();
                for item in items {
                    if declared.kind == Kind::Choice {
                        choice(declared.values.as_deref().unwrap_or_default(), item)?;
                    } else {
                        string(item, declared.matcher.as_ref(), declared.max_length)?;
                    }
                    if !seen.insert(item.as_str()) {
                        return Err(Error::Contract("list value has duplicate items"));
                    }
                }
            }
        }
        if crate::evidence::redact(&mut value.clone()) {
            return Err(Error::Contract(
                "variable values cannot contain credentials",
            ));
        }
        Ok(())
    }

    /// Selector denoted by a value, or None for booleans.
    pub(crate) fn selector(&self, value: &Value) -> Option<Value> {
        match self.kind {
            Kind::Choice | Kind::String => Some(value.clone()),
            Kind::List => Some(json!({"one_of": value})),
            Kind::Path => value.as_str().map(|path| {
                if std::path::Path::new(path).is_absolute() {
                    json!({"root":"absolute","path":path})
                } else {
                    json!({"root":"project","path":path})
                }
            }),
            Kind::Boolean => None,
        }
    }

    /// Parse operator text into a checked value: `true`/`false` for booleans; a
    /// JSON array, or comma-separated items, for lists; text otherwise.
    pub(crate) fn parse(&self, text: &str) -> Result<Value, Error> {
        let value = match self.kind {
            Kind::Boolean => match text {
                "true" => json!(true),
                "false" => json!(false),
                _ => return Err(Error::Contract("boolean values are true or false")),
            },
            Kind::List if text.trim_start().starts_with('[') => {
                serde_json::from_str(text).map_err(|_| Error::Contract("invalid JSON list"))?
            }
            Kind::List => json!(
                text.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>()
            ),
            Kind::Choice | Kind::String | Kind::Path => json!(text),
        };
        self.check(&value)?;
        Ok(value)
    }

    /// Choice values, or a list's choice items, for setup prompts.
    pub(crate) fn choices(&self) -> Option<&[String]> {
        match self.kind {
            Kind::Choice => self.values.as_deref(),
            Kind::List => self.items.as_ref().and_then(|i| i.values.as_deref()),
            _ => None,
        }
    }

    /// Pattern capabilities that the declared constraints use.
    pub(crate) fn capabilities(&self, result: &mut BTreeSet<String>) {
        let item = self.items.as_ref().and_then(|i| i.matcher.as_ref());
        for selector in self.matcher.iter().chain(item) {
            let inner = selector.get("not");
            if inner.is_some() {
                result.insert(super::rules::NEGATED_SELECTORS.into());
            }
            for (kind, capability) in [("regex", "regex-v1"), ("glob", "glob-v1")] {
                if selector.get(kind).or_else(|| inner?.get(kind)).is_some() {
                    result.insert(capability.into());
                }
            }
        }
    }

    /// A representative value used to validate references before any binding.
    pub(crate) fn sample(&self) -> Value {
        if let Some(default) = &self.default {
            return default.clone();
        }
        match self.kind {
            Kind::Choice => json!(self.values.as_ref().and_then(|v| v.first())),
            Kind::String | Kind::Path => json!("sample"),
            Kind::List => {
                let items = self.items.as_ref();
                let item = items
                    .and_then(|i| i.values.as_ref())
                    .and_then(|v| v.first())
                    .map_or_else(|| json!("sample"), |v| json!(v));
                json!([item])
            }
            Kind::Boolean => json!(true),
        }
    }
}

/// Text form of a value for templates: lists join with ", ".
pub(crate) fn render(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

fn members(values: Option<&Vec<String>>) -> Result<(), Error> {
    let values = values.ok_or(Error::Contract("choice variables declare values"))?;
    if values.is_empty() || values.len() > MAX_MEMBERS {
        return Err(Error::Contract("choice values must contain 1-256 entries"));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        selectors::literal(value, true)?;
        if value.len() > 1024 || !seen.insert(value) {
            return Err(Error::Contract(
                "choice values must be distinct and bounded",
            ));
        }
    }
    Ok(())
}

fn text_constraints(matcher: Option<&Value>, max_length: Option<usize>) -> Result<(), Error> {
    if max_length.is_some_and(|max| !(1..=1024).contains(&max)) {
        return Err(Error::Contract("invalid variable max_length"));
    }
    if let Some(selector) = matcher {
        selectors::text(selector, false)?;
        patterns::Store::default().collect(&json!({ "match": selector }), "argv")?;
    }
    Ok(())
}

fn choice(values: &[String], value: &Value) -> Result<(), Error> {
    if value
        .as_str()
        .is_some_and(|v| values.iter().any(|choice| choice == v))
    {
        Ok(())
    } else {
        Err(Error::Contract("variable value is not a declared choice"))
    }
}

fn string(value: &Value, matcher: Option<&Value>, max_length: Option<usize>) -> Result<(), Error> {
    let text = value
        .as_str()
        .ok_or(Error::Contract("variable value has the wrong type"))?;
    if text.is_empty() || text.contains('\0') || text.len() > max_length.unwrap_or(256) {
        return Err(Error::Contract("variable text is empty or too long"));
    }
    if let Some(selector) = matcher {
        let mut store = patterns::Store::default();
        store.collect(&json!({ "match": selector }), "argv")?;
        if !selectors::matches(selector, text, &store)? {
            return Err(Error::Contract("variable value does not satisfy its match"));
        }
    }
    Ok(())
}

fn normalized(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.contains('\0')
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(Error::Contract("path variables are normalized paths"));
    }
    Ok(())
}

fn relative_path(path: &str) -> Result<(), Error> {
    if path.len() > 4096 || std::path::Path::new(path).is_absolute() {
        return Err(Error::Contract("suggestion files are project-relative"));
    }
    normalized(path)
}

/// One `when` test on a variable of the owning pack.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Test {
    var: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    set: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    equals: Option<Value>,
    #[serde(
        default,
        rename = "in",
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    within: Option<Vec<Value>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    not_in: Option<Vec<Value>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    contains: Option<Value>,
}

/// Validate an entry's `when` tests against the pack's declarations.
pub(crate) fn validate_tests(
    tests: &[Test],
    declarations: &BTreeMap<String, Declaration>,
) -> Result<(), Error> {
    if tests.is_empty() || tests.len() > MAX_TESTS {
        return Err(Error::Contract("when requires 1-16 tests"));
    }
    for test in tests {
        let declaration = declarations
            .get(&test.var)
            .ok_or(Error::Contract("when names an undeclared variable"))?;
        let operators = [
            test.set.is_some(),
            test.equals.is_some(),
            test.within.is_some(),
            test.not_in.is_some(),
            test.contains.is_some(),
        ];
        if operators.iter().filter(|present| **present).count() != 1 {
            return Err(Error::Contract("each when test has exactly one operator"));
        }
        if let Some(value) = &test.equals {
            declaration.check(value)?;
        }
        for values in [&test.within, &test.not_in].into_iter().flatten() {
            if !matches!(declaration.kind, Kind::Choice | Kind::String) {
                return Err(Error::Contract(
                    "in and not_in test choice or string values",
                ));
            }
            if values.is_empty() || values.len() > MAX_MEMBERS {
                return Err(Error::Contract("in and not_in list 1-256 values"));
            }
            for value in values {
                declaration.check(value)?;
            }
        }
        if let Some(item) = &test.contains {
            if declaration.kind != Kind::List {
                return Err(Error::Contract("contains tests list variables"));
            }
            declaration.check(&json!([item]))?;
        }
    }
    Ok(())
}

impl Test {
    /// Whether this test holds for the resolved value; unset fails all but `set: false`.
    pub(crate) fn holds(&self, value: Option<&Value>) -> bool {
        if let Some(set) = self.set {
            return value.is_some() == set;
        }
        let Some(value) = value else {
            return false;
        };
        if let Some(expected) = &self.equals {
            return expected == value;
        }
        if let Some(values) = &self.within {
            return values.contains(value);
        }
        if let Some(values) = &self.not_in {
            return !values.contains(value);
        }
        if let Some(item) = &self.contains {
            return value.as_array().is_some_and(|items| items.contains(item));
        }
        false
    }

    pub(crate) fn variable(&self) -> &str {
        &self.var
    }
}

/// Replace every `{var: name}` selector in a predicate tree.
///
/// Returns the names of variables without a value; selectors for those names
/// remain in place, and the caller treats the entry as inactive.
pub(crate) fn substitute(
    value: &mut Value,
    domain: &str,
    resolve: &dyn Fn(&str) -> Result<Option<Value>, Error>,
    unset: &mut BTreeSet<String>,
) -> Result<(), Error> {
    if optional_reference(value).is_some() {
        return Err(Error::Contract(
            "unset: any applies only to an option value's match",
        ));
    }
    if let Some(name) = reference(value) {
        let name = name.to_owned();
        match resolve(&name)? {
            Some(selector) => *value = selector,
            None => {
                unset.insert(name);
            }
        }
        return Ok(());
    }
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                if key == "environment" {
                    for selector in selectors::environment_selectors_mut(child) {
                        substitute(selector, domain, resolve, unset)?;
                    }
                } else if !(["exact", "input", "context"].contains(&key.as_str())
                    || domain == "mcp" && key == "arguments")
                {
                    substitute(child, domain, resolve, unset)?;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                substitute(item, domain, resolve, unset)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Names referenced through `{var}` selectors, without changing the tree.
pub(crate) fn references(value: &Value, domain: &str, names: &mut BTreeSet<String>) {
    if let Some(name) = reference(value).or_else(|| optional_reference(value)) {
        names.insert(name.to_owned());
        return;
    }
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                if key == "environment" {
                    for selector in selectors::environment_selectors(child) {
                        references(selector, domain, names);
                    }
                } else if !(["exact", "input", "context"].contains(&key.as_str())
                    || domain == "mcp" && key == "arguments")
                {
                    references(child, domain, names);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                references(item, domain, names);
            }
        }
        _ => {}
    }
}

fn reference(value: &Value) -> Option<&str> {
    let fields = value.as_object()?;
    if fields.len() == 1 {
        fields.get("var")?.as_str()
    } else {
        None
    }
}

/// The variable named by `{var: name, unset: ...}`, an option value match that
/// states what an unset variable means. Only `unset: any` is valid.
pub(crate) fn optional_reference(value: &Value) -> Option<&str> {
    let fields = value.as_object()?;
    if fields.len() == 2 && fields.contains_key("unset") {
        fields.get("var")?.as_str()
    } else {
        None
    }
}
