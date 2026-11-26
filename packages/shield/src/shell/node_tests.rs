//! Admission for a literal Node test command explicitly trusted by a policy
//! that exports its Codex prefix. Test bodies and imports execute as code.

use super::structure::{Node, Span};
use crate::{
    configuration::CandidateEvaluation,
    policy::{Decision, Request, rules::agent_outputs::CodexPrefix},
};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn direct(tree: &Node, environment: Option<&BTreeMap<String, String>>) -> Option<Span> {
    let Node::Command {
        span,
        words,
        redirections,
    } = tree
    else {
        return None;
    };
    (redirections.is_empty()
        && words.first().and_then(|word| word.literal.as_deref()) == Some("node")
        && words.get(1).and_then(|word| word.literal.as_deref()) == Some("--test")
        && words.iter().all(|word| word.literal.is_some())
        && environment
            .and_then(|env| env.get("NODE_OPTIONS"))
            .is_none_or(String::is_empty))
    .then_some(*span)
}

pub(super) fn allowed(
    request: &Request,
    result: &CandidateEvaluation,
    prefixes: &[CodexPrefix],
) -> bool {
    if result.decision != Decision::Allow {
        return false;
    }
    let arguments: Vec<_> = request.arguments.iter().map(String::as_str).collect();
    let authorized = prefixes.iter().any(|prefix| {
        prefix.trust_project_node_tests
            && prefix.covers(&request.executable, &arguments)
            && result.evaluation().matches.iter().any(|matched| {
                matched.applied
                    && matched.decision == Decision::Allow
                    // Custom documents merge by rule ID before compilation.
                    && (matched.source == prefix.owner
                        || prefix.owner == "custom"
                            && matches!(matched.source.as_str(), "custom:project" | "custom:local"))
                    && matched.rule_id == prefix.rule
            })
    });
    authorized && project_inputs(request, &arguments)
}

fn project_inputs(request: &Request, arguments: &[&str]) -> bool {
    let (Ok(project), Ok(cwd)) = (
        fs::canonicalize(&request.project),
        fs::canonicalize(&request.cwd),
    ) else {
        return false;
    };
    if !cwd.starts_with(&project) || !cwd.is_dir() {
        return false;
    }
    let Some(mut arguments) = arguments.strip_prefix(&["--test"]) else {
        return false;
    };
    while let Some((&option, tail)) = arguments.split_first() {
        if option == "--" {
            arguments = tail;
            break;
        }
        if !option.starts_with('-') {
            break;
        }
        let (name, inline) = option
            .split_once('=')
            .map_or((option, None), |(name, value)| (name, Some(value)));
        if inline.is_none()
            && matches!(
                name,
                "--test-only" | "--test-force-exit" | "--experimental-test-coverage"
            )
        {
            arguments = tail;
            continue;
        }
        let (value, rest) = if let Some(value) = inline {
            (value, tail)
        } else if let Some((&value, rest)) = tail.split_first() {
            (value, rest)
        } else {
            return false;
        };
        let supported = match name {
            "--test-concurrency" => value.parse::<std::num::NonZeroU32>().is_ok(),
            "--test-name-pattern" | "--test-skip-pattern" => {
                !value.is_empty() && !value.starts_with('-')
            }
            "--test-isolation" => matches!(value, "process" | "none"),
            _ => false,
        };
        if !supported {
            return false;
        }
        arguments = rest;
    }
    arguments.iter().all(|file| {
        !file.is_empty()
            && !file.starts_with('-')
            && !file.starts_with('!')
            && !file.contains(['*', '?', '[', ']', '{', '}', '(', ')'])
            && fs::canonicalize(cwd.join(Path::new(file)))
                .is_ok_and(|path| path.starts_with(&project) && path.is_file())
    })
}
