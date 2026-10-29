//! Ordered descriptor flow from literal syntax, independent of filesystem facts.

use super::structure::Redirection;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum Target {
    Inherited { descriptor: u32 },
    File { path: String, mode: &'static str },
    Heredoc { index: usize },
    HereString { index: usize },
    Closed,
    Unknown,
}

#[derive(Serialize)]
pub(super) struct Flow {
    pub(super) bindings: BTreeMap<u32, Target>,
    pub(super) opened: Vec<Open>,
    pub(super) syntax_complete: bool,
    pub(super) stdin: Target,
}

#[derive(Serialize)]
pub(super) struct Open {
    index: usize,
    descriptor: u32,
    path: String,
    mode: &'static str,
}

pub(super) fn analyze(redirections: &[Redirection]) -> Flow {
    let mut flow = Flow {
        bindings: BTreeMap::new(),
        opened: Vec::new(),
        syntax_complete: true,
        stdin: Target::Inherited { descriptor: 0 },
    };
    for (index, redirect) in redirections.iter().enumerate() {
        let default = u32::from(!redirect.operator.starts_with('<'));
        let fd = redirect
            .descriptor
            .as_ref()
            .map_or(Some(default), |value| value.parse().ok());
        let Some(fd) = fd else {
            // An invalid/unsupported descriptor cannot establish a known input.
            flow.syntax_complete = false;
            flow.bindings.insert(0, Target::Unknown);
            continue;
        };
        let literal = redirect.target.literal.as_deref();
        let target = match redirect.operator.as_str() {
            "<<" | "<<-" => Target::Heredoc { index },
            "<<<" => Target::HereString { index },
            "<&" | ">&" => match literal {
                Some("-") => Target::Closed,
                Some(value) => {
                    let (source, moving) = value
                        .strip_suffix('-')
                        .map_or((value, false), |value| (value, true));
                    if let Ok(source) = source.parse::<u32>() {
                        let target = flow
                            .bindings
                            .get(&source)
                            .cloned()
                            .unwrap_or(Target::Inherited { descriptor: source });
                        if moving && source != fd {
                            flow.bindings.insert(source, Target::Closed);
                        }
                        if moving && source == fd {
                            Target::Closed
                        } else {
                            target
                        }
                    } else {
                        flow.syntax_complete = false;
                        Target::Unknown
                    }
                }
                None => {
                    flow.syntax_complete = false;
                    Target::Unknown
                }
            },
            operator => {
                let mode = match operator {
                    "<" => "read",
                    "<>" => "read-write",
                    ">>" | "&>>" => "append",
                    _ => "truncate",
                };
                if let Some(path) = literal {
                    flow.opened.push(Open {
                        index,
                        descriptor: fd,
                        path: path.into(),
                        mode,
                    });
                    Target::File {
                        path: path.into(),
                        mode,
                    }
                } else {
                    flow.syntax_complete = false;
                    Target::Unknown
                }
            }
        };
        if matches!(redirect.operator.as_str(), "&>" | "&>>") {
            flow.bindings.insert(2, target.clone());
        }
        flow.bindings.insert(fd, target);
    }
    flow.stdin = flow
        .bindings
        .get(&0)
        .cloned()
        .unwrap_or(Target::Inherited { descriptor: 0 });
    flow
}
