//! Declared argv grammar, preserving token boundaries, option arity and phases.
use super::{Error, patterns::Provider, selectors};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate(value: &Value) -> Result<(), Error> {
    let allowed: &[&str] = match value["kind"].as_str() {
        Some("any") => &["kind"],
        Some("exact") => &["kind", "values"],
        Some("prefix") => &["kind", "values", "leading"],
        Some("sequence") => &["kind", "prefix", "rest"],
        Some("risk_tokens") => &["kind", "prefix", "any_of"],
        Some("options") => &[
            "kind",
            "prefix",
            "flags",
            "values",
            "operands",
            "leading",
            "optional_values",
            "require_all",
            "require_any",
            "forbid",
            "parse",
            "short_clusters",
            "long_equals",
            "short_attached",
            "long_abbreviations",
            "unknown_options",
        ],
        _ => return Err(Error::Contract("unsupported argument kind")),
    };
    selectors::fields(value, allowed)?;
    match value["kind"].as_str() {
        Some("any") => {}
        Some("exact") => {
            let values = array(&value["values"])?;
            for v in values {
                selectors::literal(v.as_str().ok_or(Error::Syntax)?, false)?;
            }
        }
        Some("prefix") => {
            selector_array(&value["values"], true)?;
            leading(value)?;
        }
        Some("sequence") => {
            selector_array(&value["prefix"], false)?;
            let rest = &value["rest"];
            match rest["kind"].as_str() {
                Some("none" | "any") => selectors::fields(rest, &["kind"])?,
                Some("each") => {
                    selectors::fields(rest, &["kind", "match", "min", "max"])?;
                    selectors::text(&rest["match"], false)?;
                    counts(rest)?;
                }
                _ => return Err(Error::Contract("invalid sequence rest")),
            }
        }
        Some("risk_tokens") => {
            selector_array(&value["prefix"], false)?;
            selector_array(&value["any_of"], true)?;
        }
        Some("options") => validate_options(value)?,
        _ => return Err(Error::Syntax),
    }
    Ok(())
}

fn validate_options(value: &Value) -> Result<(), Error> {
    selector_array(&value["prefix"], false)?;
    let main = grammar(value)?;
    let leading = leading(value)?.unwrap_or_default();
    for (name, spec) in &leading {
        if main.get(name).is_some_and(|s| s != spec) {
            return Err(Error::Contract("conflicting option phases"));
        }
    }
    let declared: BTreeSet<_> = main.keys().chain(leading.keys()).collect();
    for key in ["require_all", "require_any", "forbid"] {
        if let Some(names) = value.get(key) {
            selectors::strings(names, false, true)?;
            for name in array(names)? {
                if !declared.contains(&name.as_str().ok_or(Error::Syntax)?.to_owned()) {
                    return Err(Error::Contract("required option must be declared"));
                }
            }
        }
    }
    for name in names(value, "forbid") {
        if names(value, "require_all")
            .chain(names(value, "require_any"))
            .any(|n| n == name)
        {
            return Err(Error::Contract("required and forbidden option"));
        }
    }
    if value
        .get("parse")
        .is_some_and(|v| !matches!(v.as_str(), Some("intermixed" | "stop_at_operand")))
    {
        return Err(Error::Syntax);
    }
    match value.get("unknown_options").map(Value::as_str) {
        None | Some(Some("refuse")) => {}
        // Skipped options may take values the grammar cannot see, so those
        // values stay ordinary operands and parsing must continue past them.
        Some(Some("skip")) => {
            if value["operands"] != true || value["parse"] == "stop_at_operand" {
                return Err(Error::Contract(
                    "unknown_options: skip needs operands: true and intermixed parsing",
                ));
            }
        }
        Some(_) => return Err(Error::Syntax),
    }
    match &value["operands"] {
        Value::Bool(_) => {}
        v => {
            selectors::fields(v, &["match", "min", "max"])?;
            selectors::text(&v["match"], false)?;
            counts(v)?;
        }
    }
    Ok(())
}

fn leading(value: &Value) -> Result<Option<BTreeMap<String, Spec>>, Error> {
    value
        .get("leading")
        .map(|v| {
            selectors::fields(
                v,
                &[
                    "flags",
                    "values",
                    "optional_values",
                    "short_clusters",
                    "long_equals",
                    "short_attached",
                    "long_abbreviations",
                ],
            )?;
            grammar(v)
        })
        .transpose()
}

fn array(value: &Value) -> Result<&[Value], Error> {
    let items = value.as_array().ok_or(Error::Syntax)?;
    if items.len() > 256 {
        return Err(Error::Limit);
    }
    Ok(items)
}
fn selector_array(value: &Value, nonempty: bool) -> Result<(), Error> {
    let items = array(value)?;
    if nonempty && items.is_empty() {
        return Err(Error::Syntax);
    }
    for item in items {
        selectors::text(item, false)?;
    }
    Ok(())
}
fn counts(value: &Value) -> Result<(usize, usize), Error> {
    let number = |key, default| {
        value.get(key).map_or(Ok(default), |v| {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or(Error::Syntax)
        })
    };
    let min = number("min", 0)?;
    let max = number("max", 256)?;
    if min > max || max > 256 {
        return Err(Error::Limit);
    }
    Ok((min, max))
}
fn setting(value: &Value, key: &str) -> Result<bool, Error> {
    value
        .get(key)
        .map_or(Ok(true), |v| v.as_bool().ok_or(Error::Syntax))
}
/// Whether a phase accepts unique prefixes of its long options; default off.
fn abbreviations(value: &Value) -> Result<bool, Error> {
    value
        .get("long_abbreviations")
        .map_or(Ok(false), |v| v.as_bool().ok_or(Error::Syntax))
}

/// How a long option name written in argv maps onto a phase's declarations.
enum Long<'a> {
    Declared(&'a str),
    Ambiguous,
    Undeclared,
}

/// Resolves `name` exactly, or, when the phase allows abbreviations, as the
/// one declared long option it begins, as argparse-style parsers do.
fn long<'a>(
    settings: &Value,
    grammar: &'a BTreeMap<String, Spec>,
    name: &str,
) -> Result<Long<'a>, Error> {
    if let Some((declared, _)) = grammar.get_key_value(name) {
        return Ok(Long::Declared(declared));
    }
    if name.len() <= 2 || !abbreviations(settings)? {
        return Ok(Long::Undeclared);
    }
    let mut candidates = grammar
        .keys()
        .filter(|declared| declared.starts_with("--") && declared.starts_with(name));
    Ok(match (candidates.next(), candidates.next()) {
        (Some(only), None) => Long::Declared(only),
        (Some(_), Some(_)) => Long::Ambiguous,
        (None, _) => Long::Undeclared,
    })
}

/// Whether the main phase must inspect `arg`; skip mode must not hide a
/// possible abbreviation or a short cluster containing a declared option.
fn recognized(
    settings: &Value,
    grammar: &BTreeMap<String, Spec>,
    arg: &str,
) -> Result<bool, Error> {
    if arg.starts_with("--") {
        let name = arg.split_once('=').map_or(arg, |(name, _)| name);
        return Ok(!matches!(long(settings, grammar, name)?, Long::Undeclared)
            || (name.len() > 2 && grammar.keys().any(|declared| declared.starts_with(name))));
    }
    Ok(arg
        .chars()
        .skip(1)
        .any(|c| grammar.contains_key(&format!("-{c}"))))
}

fn names<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a str> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

#[derive(Clone, PartialEq)]
struct Spec {
    arity: u8,
    predicate: Option<Value>,
    repeat: String,
}
fn grammar(value: &Value) -> Result<BTreeMap<String, Spec>, Error> {
    for key in ["short_clusters", "long_equals", "short_attached"] {
        setting(value, key)?;
    }
    abbreviations(value)?;
    let mut result = BTreeMap::new();
    for (key, arity) in [("flags", 0), ("values", 1), ("optional_values", 2)] {
        let items = if key == "optional_values" && value.get(key).is_none() {
            &[][..]
        } else {
            array(&value[key])?
        };
        for declaration in items {
            let (name, predicate, repeat) = if let Some(name) = declaration.as_str() {
                (name, None, "all")
            } else {
                if arity == 0 {
                    return Err(Error::Syntax);
                }
                selectors::fields(declaration, &["name", "match", "repeat"])?;
                let predicate = declaration.get("match");
                if let Some(v) = predicate {
                    selectors::text(v, false)?;
                }
                (
                    declaration["name"].as_str().ok_or(Error::Syntax)?,
                    predicate.cloned(),
                    declaration
                        .get("repeat")
                        .map_or(Some("all"), Value::as_str)
                        .ok_or(Error::Syntax)?,
                )
            };
            let valid = name.strip_prefix("--").map_or_else(
                || {
                    name.len() == 2
                        && name.starts_with('-')
                        && name.as_bytes()[1].is_ascii_alphanumeric()
                },
                |s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            );
            if !valid
                || name.len() > 128
                || (arity == 2 && !name.starts_with("--"))
                || !["all", "forbid", "last"].contains(&repeat)
                || result
                    .insert(
                        name.into(),
                        Spec {
                            arity,
                            predicate,
                            repeat: repeat.into(),
                        },
                    )
                    .is_some()
            {
                return Err(Error::Contract("invalid option declaration"));
            }
        }
    }
    Ok(result)
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Record {
    pub(super) name: String,
    #[serde(skip)]
    pub(super) value: Option<String>,
    pub(super) index: usize,
    pub(super) phase: u8,
}
#[derive(Default)]
pub(super) struct ResultSet {
    pub(super) failures: Vec<&'static str>,
    pub(super) records: Vec<Record>,
    /// Positional operands after the declared prefix and option parsing.
    pub(super) operands: Vec<String>,
    /// Options whose value failed its match, in grammar order.
    mismatched: Vec<(String, Value)>,
}
impl ResultSet {
    fn fail(&mut self, code: &'static str) {
        if !self.failures.contains(&code) {
            self.failures.push(code);
        }
    }

    pub(super) fn evidence(&self, grammar: &Value) -> Value {
        let missing: Vec<_> = names(grammar, "require_all")
            .filter(|name| !self.records.iter().any(|r| r.name == *name))
            .collect();
        let any: Vec<_> = names(grammar, "require_any").collect();
        let any_missing =
            !any.is_empty() && !self.records.iter().any(|r| any.contains(&r.name.as_str()));
        let mut evidence = serde_json::json!({"options":self.records.iter().take(16).collect::<Vec<_>>(),
            "missing_options":missing.iter().take(4).collect::<Vec<_>>(),
            "required_any":if any_missing {any.iter().take(4).collect::<Vec<_>>()}else{vec![]},
            "truncated":self.records.len()>16 || missing.len()>4 || any_missing && any.len()>4
                || self.mismatched.len()>4});
        if !self.mismatched.is_empty() {
            evidence["mismatched_options"] = self
                .mismatched
                .iter()
                .take(4)
                .map(|(name, predicate)| {
                    let mut row = serde_json::json!({ "name": name });
                    if let Some(expected) = expected_values(predicate) {
                        row["expected"] = expected;
                    } else if let Some(excluded) = predicate.get("not").and_then(expected_values) {
                        row["excluded"] = excluded;
                    }
                    row
                })
                .collect();
        }
        evidence
    }
}

/// Policy literals a value must equal: one literal, or a short `one_of`.
fn expected_values(predicate: &Value) -> Option<Value> {
    if let Some(literal) = selectors::exact(predicate) {
        return Some(serde_json::json!([literal]));
    }
    let members = predicate.get("one_of")?.as_array()?;
    (members.len() <= 8).then(|| Value::Array(members.clone()))
}

pub(super) fn matches(
    value: &Value,
    args: &[String],
    patterns: &dyn Provider,
) -> Result<ResultSet, Error> {
    let mut result = ResultSet::default();
    match value["kind"].as_str() {
        Some("any") => {}
        Some("exact") => {
            if value["values"] != serde_json::json!(args) {
                result.fail("argument_mismatch");
            }
        }
        Some("prefix" | "sequence" | "risk_tokens") => {
            let key = if value["kind"] == "prefix" {
                "values"
            } else {
                "prefix"
            };
            let prefix = array(&value[key])?;
            let mut position = 0;
            if value["kind"] == "prefix" {
                let declarations = parse_leading(value, args, &mut position, &mut result)?;
                check_values(declarations, patterns, &mut result)?;
            }
            if !prefix_matches(prefix, &args[position..], patterns)? {
                result.fail("argument_mismatch");
                return Ok(result);
            }
            let suffix = &args[position + prefix.len()..];
            if value["kind"] == "sequence" {
                match value["rest"]["kind"].as_str() {
                    Some("none") if !suffix.is_empty() => result.fail("argument_mismatch"),
                    Some("each") => operands(&value["rest"], suffix, patterns, &mut result)?,
                    _ => {}
                }
            } else if value["kind"] == "risk_tokens"
                && !risk_matches(&value["any_of"], suffix, patterns)?
            {
                result.fail("argument_mismatch");
            }
        }
        Some("options") => parse_options(value, args, patterns, &mut result)?,
        _ => return Err(Error::Syntax),
    }
    Ok(result)
}

fn prefix_matches(
    prefix: &[Value],
    args: &[String],
    patterns: &dyn Provider,
) -> Result<bool, Error> {
    if args.len() < prefix.len() {
        return Ok(false);
    }
    for (predicate, value) in prefix.iter().zip(args) {
        if !selectors::matches(predicate, value, patterns)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn operands(
    value: &Value,
    args: &[String],
    patterns: &dyn Provider,
    result: &mut ResultSet,
) -> Result<(), Error> {
    if let Some(allowed) = value.as_bool() {
        if !allowed && !args.is_empty() {
            result.fail("operand_count_mismatch");
        }
        return Ok(());
    }
    let (min, max) = counts(value)?;
    if args.len() < min || args.len() > max {
        result.fail("operand_count_mismatch");
    }
    for arg in args {
        if !selectors::matches(&value["match"], arg, patterns)? {
            result.fail("argument_mismatch");
        }
    }
    Ok(())
}

fn parse_options(
    value: &Value,
    args: &[String],
    patterns: &dyn Provider,
    result: &mut ResultSet,
) -> Result<(), Error> {
    let mut position = 0;
    let mut declarations = parse_leading(value, args, &mut position, result)?;
    declarations.extend(grammar(value)?);
    let prefix = array(&value["prefix"])?;
    if !prefix_matches(prefix, &args[position..], patterns)? {
        result.fail("argument_mismatch");
        return Ok(());
    }
    position += prefix.len();
    let main = grammar(value)?;
    let skip = value["unknown_options"] == "skip";
    let mut options = true;
    let mut remaining = Vec::new();
    while position < args.len() {
        let arg = &args[position];
        if options && arg == "--" {
            options = false;
            position += 1;
        } else if options && arg.starts_with('-') && arg != "-" {
            if skip && !recognized(value, &main, arg)? {
                position += 1;
                continue;
            }
            option(value, &main, args, &mut position, 1, result)?;
        } else {
            remaining.push(arg.clone());
            position += 1;
            if value["parse"] == "stop_at_operand" {
                options = false;
            }
        }
    }
    operands(&value["operands"], &remaining, patterns, result)?;
    result.operands = remaining;
    for name in names(value, "require_all") {
        if !result.records.iter().any(|r| r.name == name) {
            result.fail("required_option_missing");
        }
    }
    let any: Vec<_> = names(value, "require_any").collect();
    if !any.is_empty()
        && !result
            .records
            .iter()
            .any(|r| any.contains(&r.name.as_str()))
    {
        result.fail("required_option_missing");
    }
    if result
        .records
        .iter()
        .any(|r| names(value, "forbid").any(|n| n == r.name))
    {
        result.fail("forbidden_option_present");
    }
    check_values(declarations, patterns, result)
}

fn parse_leading(
    value: &Value,
    args: &[String],
    position: &mut usize,
    result: &mut ResultSet,
) -> Result<BTreeMap<String, Spec>, Error> {
    if let Some(leading) = value.get("leading") {
        let global = grammar(leading)?;
        while *position < args.len() && args[*position].starts_with('-') && args[*position] != "-" {
            if args[*position] == "--" {
                *position += 1;
                break;
            }
            option(leading, &global, args, position, 0, result)?;
        }
        return Ok(global);
    }
    Ok(BTreeMap::new())
}

fn check_values(
    declarations: BTreeMap<String, Spec>,
    patterns: &dyn Provider,
    result: &mut ResultSet,
) -> Result<(), Error> {
    for (name, spec) in declarations {
        let records: Vec<_> = result.records.iter().filter(|r| r.name == name).collect();
        let repeated = spec.repeat == "forbid" && records.len() > 1;
        let checked = if spec.repeat == "last" {
            records.last().into_iter().copied().collect::<Vec<_>>()
        } else {
            records
        };
        if let Some(predicate) = &spec.predicate {
            let mut mismatch = false;
            for record in checked {
                mismatch |= match record.value.as_deref() {
                    Some(v) => !selectors::matches(predicate, v, patterns)?,
                    None => true,
                };
            }
            if mismatch {
                result.fail("option_value_mismatch");
                result.mismatched.push((name.clone(), predicate.clone()));
            }
        }
        if repeated {
            result.fail("repeated_option");
        }
    }
    Ok(())
}

fn option(
    settings: &Value,
    grammar: &BTreeMap<String, Spec>,
    args: &[String],
    position: &mut usize,
    phase: u8,
    result: &mut ResultSet,
) -> Result<(), Error> {
    let index = *position;
    let arg = &args[index];
    *position += 1;
    if arg.starts_with("--") {
        let (name, attached) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(n, v)| (n, Some(v)));
        if attached.is_some() && !setting(settings, "long_equals")? {
            result.fail("unknown_option");
            return Ok(());
        }
        let name = match long(settings, grammar, name)? {
            Long::Declared(declared) => declared,
            Long::Ambiguous => {
                result.fail("ambiguous_option");
                return Ok(());
            }
            Long::Undeclared => name,
        };
        consume(
            name, attached, grammar, args, position, index, phase, result,
        );
    } else if arg.len() == 2 {
        consume(arg, None, grammar, args, position, index, phase, result);
    } else if arg.is_ascii() {
        let mut offset = 1;
        while offset < arg.len() {
            let name = format!("-{}", char::from(arg.as_bytes()[offset]));
            offset += 1;
            let Some(spec) = grammar.get(&name) else {
                result.fail("unknown_option");
                break;
            };
            if spec.arity == 0 && !setting(settings, "short_clusters")? {
                result.fail("unknown_option");
                break;
            }
            let attached = (spec.arity != 0 && offset < arg.len()).then(|| &arg[offset..]);
            if attached.is_some() && !setting(settings, "short_attached")? {
                result.fail("unknown_option");
                break;
            }
            consume(
                &name, attached, grammar, args, position, index, phase, result,
            );
            if spec.arity != 0 {
                break;
            }
        }
    } else {
        result.fail("unknown_option");
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "one parsed occurrence retains token index and phase with its owning result"
)]
fn consume(
    name: &str,
    attached: Option<&str>,
    grammar: &BTreeMap<String, Spec>,
    args: &[String],
    position: &mut usize,
    index: usize,
    phase: u8,
    result: &mut ResultSet,
) {
    let Some(spec) = grammar.get(name) else {
        result.fail("unknown_option");
        return;
    };
    if spec.arity == 0 && attached.is_some() {
        result.fail("unknown_option");
        return;
    }
    let value = if let Some(value) = attached {
        Some(value.to_owned())
    } else if spec.arity == 1 {
        let Some(value) = args.get(*position) else {
            result.fail("option_value_missing");
            return;
        };
        *position += 1;
        Some(value.clone())
    } else {
        None
    };
    result.records.push(Record {
        name: name.into(),
        value,
        index,
        phase,
    });
}

fn risk_matches(
    predicates: &Value,
    args: &[String],
    patterns: &dyn Provider,
) -> Result<bool, Error> {
    let mut options = true;
    for arg in args {
        if arg == "--" {
            options = false;
        }
        for selector in array(predicates)? {
            if selectors::matches(selector, arg, patterns)? {
                return Ok(true);
            }
            if options && let Some(literal) = selectors::exact(selector) {
                if literal.starts_with("--")
                    && arg.split_once('=').is_some_and(|(name, _)| name == literal)
                {
                    return Ok(true);
                }
                if literal.len() == 2
                    && literal.starts_with('-')
                    && arg.starts_with('-')
                    && !arg.starts_with("--")
                    && arg.is_ascii()
                    && arg[1..].contains(&literal[1..])
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
