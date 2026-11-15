//! Ordered descriptor flow from literal syntax, independent of filesystem facts.

use super::structure::Redirection;
use crate::policy::resolution::diagnostics::Private;
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
impl Flow {
    pub(super) fn private_input_view(&self) -> impl Serialize + '_ {
        PrivateFlow(self)
    }
}
struct PrivateFlow<'a>(&'a Flow);
impl Serialize for PrivateFlow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            bindings: Bindings<'a>,
            opened: Opened<'a>,
            syntax_complete: bool,
            stdin: PrivateTarget<'a>,
        }
        View {
            bindings: Bindings(&self.0.bindings),
            opened: Opened(&self.0.opened),
            syntax_complete: self.0.syntax_complete,
            stdin: PrivateTarget(&self.0.stdin),
        }
        .serialize(serializer)
    }
}
struct PrivateTarget<'a>(&'a Target);
impl Serialize for PrivateTarget<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Target::File { path, mode } = self.0 {
            #[derive(Serialize)]
            struct View<'a> {
                kind: &'static str,
                path: Private<&'a str>,
                mode: &'static str,
            }
            View {
                kind: "file",
                path: Private::new(path.as_str(), true),
                mode,
            }
            .serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}
struct Bindings<'a>(&'a BTreeMap<u32, Target>);
impl Serialize for Bindings<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (descriptor, target) in self.0 {
            map.serialize_entry(descriptor, &PrivateTarget(target))?;
        }
        map.end()
    }
}
struct Opened<'a>(&'a [Open]);
impl Serialize for Opened<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        #[derive(Serialize)]
        struct View<'a> {
            index: usize,
            descriptor: u32,
            path: Private<&'a str>,
            mode: &'static str,
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for open in self.0 {
            sequence.serialize_element(&View {
                index: open.index,
                descriptor: open.descriptor,
                path: Private::new(open.path.as_str(), true),
                mode: open.mode,
            })?;
        }
        sequence.end()
    }
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
