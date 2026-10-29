//! Agent-specific outputs a rule asks for, such as Codex command prefixes,
//! each checked against the rule's own conditions.
use super::{Decision, Error, Profile, rules::Rule};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most prefixes one rule can declare.
const MAX_PREFIXES: usize = 16;
/// Most positions in one prefix, and most tokens in one position.
const MAX_TOKENS: usize = 32;

/// A rule's `agents` field.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Agents {
    codex: Codex,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Codex {
    prefixes: Vec<Vec<Token>>,
}

/// One prefix position: a literal token, or a list of accepted tokens.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum Token {
    One(String),
    Any(Vec<String>),
}

impl Token {
    fn tokens(&self) -> Vec<String> {
        match self {
            Self::One(token) => vec![token.clone()],
            Self::Any(tokens) => tokens.clone(),
        }
    }
}

/// One Codex `prefix_rule` pattern: each position lists its accepted tokens.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexPrefix {
    /// Pack identity, or `custom` for project rules.
    pub(crate) owner: String,
    pub(crate) rule: String,
    pub(crate) pattern: Vec<Vec<String>>,
}

impl CodexPrefix {
    /// Whether this pattern covers an argv the way Codex matches it.
    pub(crate) fn covers(&self, executable: &str, arguments: &[&str]) -> bool {
        let mut argv = std::iter::once(executable).chain(arguments.iter().copied());
        self.pattern.iter().all(|position| {
            argv.next()
                .is_some_and(|token| position.iter().any(|t| t == token))
        })
    }
}

impl Agents {
    /// Refuse prefixes this rule cannot vouch for. Each prefix starts within
    /// the rule's literal executable and argument prefix, so it never names a
    /// command family wider than the rule; later positions narrow it further.
    pub(super) fn validate(&self, rule: &Rule) -> Result<(), Error> {
        // A written prefix would outlive the rule's expiry until the next activation.
        if rule.expires_at.is_some() {
            return Err(Error::Contract("agents cannot be set on an expiring rule"));
        }
        let prefixes = &self.codex.prefixes;
        if prefixes.is_empty() || prefixes.len() > MAX_PREFIXES {
            return Err(Error::Contract(
                "agents.codex.prefixes lists between 1 and 16 prefixes",
            ));
        }
        let bound = bound(rule)?;
        for prefix in prefixes {
            let pattern = pattern(prefix)?;
            let within = pattern.len() >= bound.len()
                && pattern
                    .iter()
                    .zip(&bound)
                    .all(|(tokens, allowed)| tokens.iter().all(|t| allowed.contains(t)));
            if !within {
                return Err(Error::Contract(
                    "each agents.codex prefix starts with the rule's executable and argument prefix",
                ));
            }
        }
        Ok(())
    }
}

/// The Codex prefixes of an active rule that allows under `profile`.
pub(super) fn codex_prefixes(rule: &Rule, owner: &str, profile: Profile) -> Vec<CodexPrefix> {
    let Some(agents) = &rule.agents else {
        return Vec::new();
    };
    if rule.decision.selected(profile) != Decision::Allow {
        return Vec::new();
    }
    agents
        .codex
        .prefixes
        .iter()
        .map(|prefix| CodexPrefix {
            owner: owner.into(),
            rule: rule.id.clone(),
            pattern: prefix.iter().map(Token::tokens).collect(),
        })
        .collect()
}

/// A declared prefix as Codex positions: an executable and at least one
/// argument, each position holding printable, nonempty tokens.
fn pattern(prefix: &[Token]) -> Result<Vec<Vec<String>>, Error> {
    if !(2..=MAX_TOKENS).contains(&prefix.len()) {
        return Err(Error::Contract(
            "an agents.codex prefix names an executable and at least one argument",
        ));
    }
    prefix
        .iter()
        .map(|position| {
            let tokens = position.tokens();
            let printable = |t: &String| !t.is_empty() && !t.chars().any(char::is_control);
            (!tokens.is_empty() && tokens.len() <= MAX_TOKENS && tokens.iter().all(printable))
                .then_some(tokens)
                .ok_or(Error::Contract(
                    "agents.codex prefix tokens are nonempty printable text",
                ))
        })
        .collect()
}

/// The rule's literal executable and argument prefix, which bounds its prefixes.
fn bound(rule: &Rule) -> Result<Vec<Vec<String>>, Error> {
    let refuse = Error::Contract(
        "agents.codex needs an argv rule with a literal executable and argument prefix",
    );
    if rule.invocation.is_some() || rule.conditions["domain"] != "argv" {
        return Err(refuse);
    }
    let arguments = &rule.conditions["arguments"];
    let key = match arguments["kind"].as_str() {
        Some("prefix" | "exact") => "values",
        Some("sequence" | "risk_tokens" | "options") => "prefix",
        _ => return Err(refuse),
    };
    let tokens = arguments[key].as_array().ok_or(Error::Syntax)?;
    if tokens.is_empty() {
        return Err(refuse);
    }
    std::iter::once(&rule.conditions["executable"])
        .chain(tokens)
        .map(|selector| {
            literals(selector).ok_or(Error::Contract(
                "agents.codex needs a rule whose prefix tokens are literal",
            ))
        })
        .collect()
}

fn literals(selector: &Value) -> Option<Vec<String>> {
    let values: Vec<&Value> = match selector {
        Value::String(_) => vec![selector],
        Value::Object(fields) if fields.len() == 1 => match fields.iter().next()? {
            (kind, value) if kind == "exact" => vec![value],
            (kind, Value::Array(items)) if kind == "one_of" => items.iter().collect(),
            _ => return None,
        },
        _ => return None,
    };
    values
        .into_iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(prefixes: &Value) -> Rule {
        serde_json::from_value(
            json!({"id":"logs","decision":"allow","agents":{"codex":{"prefixes":prefixes}},
            "conditions":{"domain":"argv","executable":"acme",
            "arguments":{"kind":"options","prefix":["logs",{"one_of":["tail","filter"]}],"flags":[],"values":[],"operands":false}}}),
        )
        .expect("rule")
    }

    #[test]
    fn declared_prefixes_stay_within_the_rule_and_follow_its_decision() {
        let allowed = rule(&json!([
            ["acme", "logs", "filter"],
            ["acme", "logs", ["tail", "filter"], "--follow"]
        ]));
        allowed.agents.as_ref().unwrap().validate(&allowed).unwrap();
        let prefixes = codex_prefixes(&allowed, "acme/tools", Profile::Recommended);
        assert_eq!(
            prefixes
                .iter()
                .map(|p| p.pattern.clone())
                .collect::<Vec<_>>(),
            vec![
                vec![vec!["acme"], vec!["logs"], vec!["filter"]],
                vec![
                    vec!["acme"],
                    vec!["logs"],
                    vec!["tail", "filter"],
                    vec!["--follow"]
                ],
            ]
        );
        assert!(prefixes[0].covers("acme", &["logs", "filter", "--since", "1h"]));
        assert!(!prefixes[0].covers("acme", &["--profile", "x", "logs", "filter"]));
        assert!(!prefixes[0].covers("acme", &["logs"]));

        let mut asking = allowed;
        asking.decision = serde_json::from_value(json!("ask")).unwrap();
        assert_eq!(
            codex_prefixes(&asking, "acme/tools", Profile::Recommended),
            []
        );

        for prefixes in [
            // Wider than the rule: it would cover every `acme logs` command.
            json!([["acme", "logs"]]),
            json!([["acme", "logs", ["filter", "delete"]]]),
            json!([["other", "logs", "filter"]]),
            json!([["acme"]]),
            json!([]),
            json!([["acme", "logs", ""]]),
            json!([["acme", "logs", []]]),
        ] {
            let refused = rule(&prefixes);
            assert!(
                refused.agents.as_ref().unwrap().validate(&refused).is_err(),
                "{prefixes}"
            );
        }
        for arguments in [
            json!({"kind":"any"}),
            json!({"kind":"prefix","values":[{"regex":"^l"}]}),
            json!({"kind":"sequence","prefix":[],"rest":{"kind":"any"}}),
        ] {
            let mut open = rule(&json!([["acme", "logs", "filter"]]));
            open.conditions["arguments"] = arguments;
            assert!(bound(&open).is_err());
        }
    }
}
