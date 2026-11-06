//! Bounded full-document validation and selected-operation traversal.
use super::{Failure, carrier::Carrier, visit};
use crate::policy::{Error, patterns::Provider, selectors};
use graphql_parser::query as q;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Ast = q::Document<'static, String>;
type Selection = q::SelectionSet<'static, String>;
type GValue = q::Value<'static, String>;
const MAX_DEPTH: usize = 64;

pub(super) struct Document {
    ast: Ast,
    selected: usize,
    variables: BTreeMap<String, Value>,
    fragments: BTreeMap<String, usize>,
    query: bool,
    paginate: bool,
}
fn selection<'a>(operation: &'a q::OperationDefinition<'static, String>) -> &'a Selection {
    match operation {
        q::OperationDefinition::SelectionSet(s) => s,
        q::OperationDefinition::Query(q) => &q.selection_set,
        q::OperationDefinition::Mutation(m) => &m.selection_set,
        q::OperationDefinition::Subscription(s) => &s.selection_set,
    }
}
fn operation_name<'a>(operation: &'a q::OperationDefinition<'static, String>) -> Option<&'a str> {
    match operation {
        q::OperationDefinition::SelectionSet(_) => None,
        q::OperationDefinition::Query(q) => q.name.as_deref(),
        q::OperationDefinition::Mutation(m) => m.name.as_deref(),
        q::OperationDefinition::Subscription(s) => s.name.as_deref(),
    }
}
fn definitions<'a>(
    operation: &'a q::OperationDefinition<'static, String>,
) -> &'a [q::VariableDefinition<'static, String>] {
    match operation {
        q::OperationDefinition::SelectionSet(_) => &[],
        q::OperationDefinition::Query(q) => &q.variable_definitions,
        q::OperationDefinition::Mutation(m) => &m.variable_definitions,
        q::OperationDefinition::Subscription(s) => &s.variable_definitions,
    }
}
fn scalar(value: &GValue) -> Option<Value> {
    match value {
        q::Value::String(v) | q::Value::Enum(v) => Some(Value::String(v.clone())),
        q::Value::Int(n) => n.as_i64().map(Value::from),
        q::Value::Float(n) => serde_json::Number::from_f64(*n).map(Value::Number),
        q::Value::Boolean(v) => Some(Value::Bool(*v)),
        q::Value::Null => Some(Value::Null),
        _ => None,
    }
}
fn kind<'a>(ty: &'a q::Type<'static, String>) -> Option<(&'a str, bool)> {
    match ty {
        q::Type::NamedType(n) => Some((n, false)),
        q::Type::NonNullType(inner) => kind(inner).map(|(n, _)| (n, true)),
        q::Type::ListType(_) => None,
    }
}
fn compatible(ty: &str, value: &Value) -> bool {
    value.is_null()
        || match ty {
            "Boolean" => value.is_boolean(),
            "Int" => value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
            "Float" => value.is_number(),
            "ID" => value.is_string() || value.is_i64(),
            _ => value.is_string(),
        }
}

#[expect(
    clippy::too_many_lines,
    reason = "one bounded document admission pass keeps definition, graph and variable validation ordered"
)]
pub(super) fn parse(
    carrier: &Carrier,
    visits: &mut usize,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Result<Document, Failure>, Error> {
    checkpoint()?;
    super::record("graphql.parse");
    let ast = match q::parse_query::<String>(&carrier.query) {
        Ok(ast) => ast.into_static(),
        Err(error) => {
            // The dependency reports its nesting limit as a parse error; it is a
            // blocking resource failure, never the ordinary unknown fallback.
            if error.to_string().contains("Recursion limit exceeded") {
                return Err(Error::Limit);
            }
            return Ok(Err(Failure::new("document", "malformed GraphQL document")));
        }
    };
    checkpoint()?;
    if ast.definitions.len() > 64 {
        return Err(Error::Limit);
    }
    let fail = || Failure::new("document", "invalid or unsupported GraphQL document");
    let mut fragments = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut operations = vec![];
    let mut anonymous = false;
    for (index, definition) in ast.definitions.iter().enumerate() {
        visit(visits, checkpoint)?;
        match definition {
            q::Definition::Fragment(fragment) => {
                if fragments.insert(fragment.name.clone(), index).is_some()
                    || !fragment.directives.is_empty()
                {
                    return Ok(Err(fail()));
                }
            }
            q::Definition::Operation(op) => {
                if let Some(name) = operation_name(op) {
                    if !names.insert(name) {
                        return Ok(Err(fail()));
                    }
                } else {
                    anonymous = true;
                }
                if definitions(op).len() > 128 {
                    return Err(Error::Limit);
                }
                let mut vars = BTreeSet::new();
                for variable in definitions(op) {
                    if !vars.insert(&variable.name)
                        || kind(&variable.var_type).is_none()
                        || variable
                            .default_value
                            .as_ref()
                            .is_some_and(|v| scalar(v).is_none())
                    {
                        return Ok(Err(fail()));
                    }
                }
                let directives = match op {
                    q::OperationDefinition::SelectionSet(_) => &[][..],
                    q::OperationDefinition::Query(q) => &q.directives,
                    q::OperationDefinition::Mutation(q) => &q.directives,
                    q::OperationDefinition::Subscription(q) => &q.directives,
                };
                if !directives.is_empty() {
                    return Ok(Err(fail()));
                }
                operations.push(index);
            }
        }
    }
    if operations.is_empty() || anonymous && operations.len() != 1 {
        return Ok(Err(fail()));
    }
    let selected = if let Some(name) = &carrier.operation {
        operations.iter().copied().find(|i| matches!(&ast.definitions[*i], q::Definition::Operation(op) if operation_name(op)==Some(name))).ok_or_else(fail)
    } else if operations.len() == 1 {
        Ok(operations[0])
    } else {
        Err(fail())
    };
    let selected = match selected {
        Ok(s) => s,
        Err(f) => return Ok(Err(f)),
    };
    let mut document = Document {
        ast,
        selected,
        fragments,
        variables: BTreeMap::new(),
        query: false,
        paginate: carrier.paginate,
    };
    // Validate fragment graphs and structural uniqueness once, with memoized
    // fragment summaries. Variable sets are checked per operation below.
    let mut summaries = BTreeMap::new();
    let mut active = BTreeSet::new();
    let mut nodes = 0;
    for name in document.fragments.keys() {
        if document
            .fragment_refs(
                name,
                0,
                &mut summaries,
                &mut active,
                &mut nodes,
                visits,
                checkpoint,
            )?
            .is_err()
        {
            return Ok(Err(fail()));
        }
    }
    for definition in &document.ast.definitions {
        if let q::Definition::Operation(op) = definition {
            let refs = document.structural(
                selection(op),
                0,
                &mut summaries,
                &mut active,
                &mut nodes,
                visits,
                checkpoint,
            )?;
            let refs = match refs {
                Ok(refs) => refs,
                Err(f) => return Ok(Err(f)),
            };
            if refs
                .iter()
                .any(|name| !definitions(op).iter().any(|v| &v.name == name))
            {
                return Ok(Err(fail()));
            }
        }
    }
    let q::Definition::Operation(operation) = &document.ast.definitions[selected] else {
        return Err(Error::Syntax);
    };
    document.query = matches!(
        operation,
        q::OperationDefinition::Query(_) | q::OperationDefinition::SelectionSet(_)
    );
    for variable in definitions(operation) {
        let Some((ty, required)) = kind(&variable.var_type) else {
            return Ok(Err(fail()));
        };
        let value = carrier
            .variables
            .get(&variable.name)
            .cloned()
            .or_else(|| variable.default_value.as_ref().and_then(scalar));
        if required && value.as_ref().is_none_or(Value::is_null) {
            return Ok(Err(fail()));
        }
        if let Some(value) = value {
            if !compatible(ty, &value) {
                return Ok(Err(fail()));
            }
            document.variables.insert(variable.name.clone(), value);
        }
        if document.paginate && variable.name == "endCursor" && (ty != "String" || required) {
            return Ok(Err(Failure::new(
                "pagination",
                "cursor must be nullable String",
            )));
        }
    }
    if document.paginate && !definitions(operation).iter().any(|v| v.name == "endCursor") {
        return Ok(Err(Failure::new(
            "pagination",
            "missing nullable endCursor variable",
        )));
    }
    Ok(Ok(document))
}

impl Document {
    #[expect(
        clippy::too_many_arguments,
        reason = "one shared structural budget includes fragment expansion depth"
    )]
    fn fragment_refs(
        &self,
        name: &str,
        depth: usize,
        memo: &mut BTreeMap<String, BTreeSet<String>>,
        active: &mut BTreeSet<String>,
        nodes: &mut usize,
        visits: &mut usize,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Result<BTreeSet<String>, Failure>, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Limit);
        }
        visit(visits, checkpoint)?;
        if let Some(refs) = memo.get(name) {
            return Ok(Ok(refs.clone()));
        }
        if !active.insert(name.into()) {
            return Ok(Err(Failure::new("document", "cyclic fragment")));
        }
        let Some(index) = self.fragments.get(name) else {
            return Ok(Err(Failure::new("document", "undefined fragment")));
        };
        let q::Definition::Fragment(fragment) = &self.ast.definitions[*index] else {
            return Err(Error::Syntax);
        };
        let result = self.structural(
            &fragment.selection_set,
            depth,
            memo,
            active,
            nodes,
            visits,
            checkpoint,
        )?;
        active.remove(name);
        if let Ok(refs) = &result {
            memo.insert(name.into(), refs.clone());
        }
        Ok(result)
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "one shared structural budget includes fragment expansion depth"
    )]
    fn structural(
        &self,
        set: &Selection,
        depth: usize,
        memo: &mut BTreeMap<String, BTreeSet<String>>,
        active: &mut BTreeSet<String>,
        nodes: &mut usize,
        visits: &mut usize,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Result<BTreeSet<String>, Failure>, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Limit);
        }
        let mut refs = BTreeSet::new();
        for item in &set.items {
            visit(visits, checkpoint)?;
            *nodes += 1;
            if *nodes > 4096 {
                return Err(Error::Limit);
            }
            let (arguments, directives, child) = match item {
                q::Selection::Field(f) => {
                    (&f.arguments[..], &f.directives[..], Some(&f.selection_set))
                }
                q::Selection::InlineFragment(f) => {
                    (&[][..], &f.directives[..], Some(&f.selection_set))
                }
                q::Selection::FragmentSpread(f) => {
                    match self.fragment_refs(
                        &f.fragment_name,
                        depth + 1,
                        memo,
                        active,
                        nodes,
                        visits,
                        checkpoint,
                    )? {
                        Ok(r) => refs.extend(r),
                        Err(f) => return Ok(Err(f)),
                    }
                    (&[][..], &f.directives[..], None)
                }
            };
            let mut names = BTreeSet::new();
            for (name, value) in arguments {
                visit(visits, checkpoint)?;
                if !names.insert(name) || !scalar_or_variable(value, &mut refs) {
                    return Ok(Err(Failure::new(
                        "document",
                        "duplicate argument or unsupported value",
                    )));
                }
            }
            let mut names = BTreeSet::new();
            for directive in directives {
                visit(visits, checkpoint)?;
                if !matches!(directive.name.as_str(), "skip" | "include")
                    || !names.insert(&directive.name)
                    || directive.arguments.len() != 1
                    || directive.arguments[0].0 != "if"
                    || !scalar_or_variable(&directive.arguments[0].1, &mut refs)
                {
                    return Ok(Err(Failure::new("document", "unsupported directive")));
                }
            }
            if let Some(child) = child {
                match self.structural(child, depth + 1, memo, active, nodes, visits, checkpoint)? {
                    Ok(r) => refs.extend(r),
                    Err(f) => return Ok(Err(f)),
                }
            }
        }
        Ok(Ok(refs))
    }
    fn value(&self, value: &GValue) -> Option<Value> {
        if let q::Value::Variable(name) = value {
            self.variables.get(name).cloned()
        } else {
            scalar(value)
        }
    }
    fn is_enum(&self, value: &GValue) -> bool {
        match value {
            q::Value::Enum(_) => true,
            q::Value::Variable(name) => {
                let q::Definition::Operation(op) = &self.ast.definitions[self.selected] else {
                    return false;
                };
                definitions(op)
                    .iter()
                    .find(|v| &v.name == name)
                    .and_then(|v| kind(&v.var_type))
                    .is_some_and(|(ty, _)| {
                        !matches!(ty, "String" | "ID" | "Int" | "Float" | "Boolean")
                    })
            }
            _ => false,
        }
    }
    pub(super) fn check(
        &self,
        model: Option<&Value>,
        patterns: &dyn Provider,
        visits: &mut usize,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Option<Failure>, Error> {
        if !self.query {
            return Ok(Some(Failure::new(
                "operation",
                "selected operation is not a query",
            )));
        }
        let q::Definition::Operation(operation) = &self.ast.definitions[self.selected] else {
            return Err(Error::Syntax);
        };
        let mut seen = BTreeSet::new();
        super::record("graphql.selection");
        self.walk(
            selection(operation),
            0,
            model,
            model.and_then(|m| m["root"].as_str()),
            patterns,
            &mut seen,
            &mut vec![],
            visits,
            checkpoint,
        )
    }
    fn check_directives(
        &self,
        directives: &[q::Directive<'static, String>],
        visits: &mut usize,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Option<Failure>, Error> {
        for directive in directives {
            visit(visits, checkpoint)?;
            let value = &directive.arguments[0].1;
            if self.paginate && matches!(value,q::Value::Variable(n) if n=="endCursor")
                || !self.value(value).is_some_and(|v| v.is_boolean())
            {
                return Ok(Some(Failure::new(
                    "directive",
                    "unknown or invalid directive value",
                )));
            }
        }
        Ok(None)
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "bounded traversal shares one request budget and fragment/state memo"
    )]
    fn walk(
        &self,
        set: &Selection,
        depth: usize,
        model: Option<&Value>,
        state: Option<&str>,
        patterns: &dyn Provider,
        seen: &mut BTreeSet<(String, String)>,
        path: &mut Vec<String>,
        visits: &mut usize,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Option<Failure>, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Limit);
        }
        let current = model.zip(state).map(|(m, s)| &m["states"][s]);
        for item in &set.items {
            visit(visits, checkpoint)?;
            let directives = match item {
                q::Selection::Field(f) => &f.directives,
                q::Selection::InlineFragment(f) => &f.directives,
                q::Selection::FragmentSpread(f) => &f.directives,
            };
            if let Some(failure) = self.check_directives(directives, visits, checkpoint)? {
                return Ok(Some(failure));
            }
            match item {
                q::Selection::Field(field) => {
                    path.push(field.name.clone());
                    let declaration = current.map(|s| &s["fields"][&field.name]);
                    if let Some(mut failure) =
                        self.check_field(field, declaration, patterns, visits, checkpoint)?
                    {
                        failure.path.clone_from(path);
                        return Ok(Some(failure));
                    }
                    let next = declaration.and_then(|d| d["select"].as_str());
                    if let Some(failure) = self.walk(
                        &field.selection_set,
                        depth + 1,
                        model,
                        next,
                        patterns,
                        seen,
                        path,
                        visits,
                        checkpoint,
                    )? {
                        return Ok(Some(failure));
                    }
                    path.pop();
                }
                q::Selection::FragmentSpread(spread) => {
                    let index = *self
                        .fragments
                        .get(&spread.fragment_name)
                        .ok_or(Error::Syntax)?;
                    let q::Definition::Fragment(fragment) = &self.ast.definitions[index] else {
                        return Err(Error::Syntax);
                    };
                    if !type_matches(current, Some(&fragment.type_condition)) {
                        return Ok(Some(Failure::new(
                            "selection",
                            "type condition is not admitted",
                        )));
                    }
                    if seen.insert((spread.fragment_name.clone(), state.unwrap_or("").into()))
                        && let Some(f) = self.walk(
                            &fragment.selection_set,
                            depth + 1,
                            model,
                            state,
                            patterns,
                            seen,
                            path,
                            visits,
                            checkpoint,
                        )?
                    {
                        return Ok(Some(f));
                    }
                }
                q::Selection::InlineFragment(fragment) => {
                    if !type_matches(current, fragment.type_condition.as_ref()) {
                        return Ok(Some(Failure::new(
                            "selection",
                            "type condition is not admitted",
                        )));
                    }
                    if let Some(f) = self.walk(
                        &fragment.selection_set,
                        depth + 1,
                        model,
                        state,
                        patterns,
                        seen,
                        path,
                        visits,
                        checkpoint,
                    )? {
                        return Ok(Some(f));
                    }
                }
            }
        }
        Ok(None)
    }
    fn check_field(
        &self,
        field: &q::Field<'static, String>,
        declaration: Option<&Value>,
        patterns: &dyn Provider,
        visits: &mut usize,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<Option<Failure>, Error> {
        if declaration.is_some_and(Value::is_null) {
            return Ok(Some(Failure::new("selection", "field is not admitted")));
        }
        if let Some(rule) = declaration {
            if rule
                .get("arguments")
                .and_then(Value::as_object)
                .is_some_and(|args| {
                    args.iter().any(|(n, a)| {
                        a["required"] == true && !field.arguments.iter().any(|(name, _)| name == n)
                    })
                })
            {
                return Ok(Some(Failure::new("selection", "required argument missing")));
            }
            if rule["leaf"] == true && !field.selection_set.items.is_empty()
                || rule.get("select").is_some() && field.selection_set.items.is_empty()
            {
                return Ok(Some(Failure::new(
                    "selection",
                    "unexpected child selection",
                )));
            }
        }
        for (name, value) in &field.arguments {
            visit(visits, checkpoint)?;
            let rule = declaration.and_then(|d| d["arguments"].get(name));
            if self.paginate && matches!(value,q::Value::Variable(n) if n=="endCursor") {
                let allowed = if declaration.is_some() {
                    rule.is_some_and(|r| r["cursor"] == true)
                } else {
                    matches!(name.as_str(), "after" | "before")
                };
                if !allowed {
                    return Ok(Some(Failure::new(
                        "pagination",
                        "cursor changes a non-cursor argument",
                    )));
                }
            } else if declaration.is_some() {
                let Some(rule) = rule else {
                    return Ok(Some(Failure::new("selection", "argument is not admitted")));
                };
                match self.value(value) {
                    None if rule["required"] == true => {
                        return Ok(Some(Failure::new(
                            "selection",
                            "scope value is unavailable",
                        )));
                    }
                    Some(actual) if !argument(rule, &actual, self.is_enum(value), patterns)? => {
                        return Ok(Some(Failure::new(
                            "selection",
                            "argument value does not match",
                        )));
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }
}
fn scalar_or_variable(value: &GValue, refs: &mut BTreeSet<String>) -> bool {
    if let q::Value::Variable(name) = value {
        refs.insert(name.clone());
        true
    } else {
        scalar(value).is_some()
    }
}
fn type_matches(
    state: Option<&Value>,
    condition: Option<&q::TypeCondition<'static, String>>,
) -> bool {
    let Some(q::TypeCondition::On(name)) = condition else {
        return true;
    };
    state.is_none_or(|s| {
        s["types"]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| t == name))
    })
}
fn argument(
    rule: &Value,
    value: &Value,
    is_enum: bool,
    patterns: &dyn Provider,
) -> Result<bool, Error> {
    if value.is_null() {
        return Ok(rule["nullable"] == true);
    }
    let matcher = &rule["match"];
    if let Some(selector) = matcher.get("text") {
        return value.as_str().map_or(Ok(false), |v| {
            if is_enum {
                Ok(false)
            } else {
                selectors::matches(selector, v, patterns)
            }
        });
    }
    if let Some(expected) = matcher.get("exact") {
        return Ok(!is_enum && expected == value);
    }
    Ok(match matcher["scalar"].as_str() {
        Some("string") => !is_enum && value.is_string(),
        Some("enum") => is_enum && value.is_string(),
        Some("integer") => value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        _ => false,
    })
}
