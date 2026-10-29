//! Branch state, function binding identity and conservative mutation invalidation.
use super::{Dialect, assignment, bindings, structure::Node};
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about the analyzed shell, each with its own lifetime"
)]
pub(super) struct State {
    pub(super) stopped: bool,
    pub(super) brace_expansion: Option<bool>,
    pub(super) cwd_unknown: bool,
    pub(super) environment_unknown: bool,
    pub(super) functions: BTreeMap<String, Vec<FunctionBinding>>,
    pub(super) defined_functions: std::collections::BTreeSet<String>,
    // None is unknown, not an empty argument list. Individual unknown operands
    // preserve the indices of known operands without guessing field splitting.
    pub(super) positionals: Option<Vec<Option<String>>>,
    pub(super) arg0: Option<String>,
    pub(super) variables: BTreeMap<String, super::provenance::Binding>,
    pub(super) exported: std::collections::BTreeSet<String>,
    pub(super) locals: Vec<BTreeMap<String, bindings::Saved>>,
    /// The next simple command reads a pipe as its standard input. It is taken
    /// by that command, so nested launches and later commands never inherit it.
    pub(super) stdin_pipe: bool,
}

#[derive(Clone)]
pub(super) struct FunctionBinding {
    pub(super) body: Rc<Node>,
    pub(super) source_unit: usize,
    pub(super) dialect: Dialect,
}

impl State {
    pub(super) fn join(&mut self, other: &Self) {
        if self.stopped && !other.stopped {
            self.clone_from(other);
            return;
        }
        if other.stopped && !self.stopped {
            return;
        }
        self.cwd_unknown |= other.cwd_unknown;
        self.environment_unknown |= other.environment_unknown;
        if self.brace_expansion != other.brace_expansion {
            self.brace_expansion = None;
        }
        self.defined_functions
            .retain(|name| other.defined_functions.contains(name));
        if self.positionals != other.positionals {
            self.positionals = None;
        }
        if self.arg0 != other.arg0 {
            self.arg0 = None;
        }
        self.variables
            .retain(|name, value| other.variables.get(name) == Some(value));
        self.exported.retain(|name| other.exported.contains(name));
        if self.locals.len() == other.locals.len() {
            for (frame, other) in self.locals.iter_mut().zip(&other.locals) {
                for name in frame.keys().chain(other.keys()) {
                    if frame.get(name) != other.get(name) {
                        self.variables.remove(name);
                        self.exported.remove(name);
                    }
                }
                frame.retain(|name, saved| other.get(name) == Some(saved));
            }
        } else {
            self.locals.clear();
        }
        for (name, bindings) in &other.functions {
            let current = self.functions.entry(name.clone()).or_default();
            for binding in bindings {
                if !current
                    .iter()
                    .any(|old| Rc::ptr_eq(&old.body, &binding.body))
                {
                    current.push(binding.clone());
                }
            }
        }
    }
}

pub(super) fn state_effect(executable: &str, arguments: Option<&[String]>, state: &mut State) {
    let brace_expansion = if executable == "set" {
        arguments.and_then(|arguments| match arguments {
            [value] if value == "-B" => Some(true),
            [value] if value == "+B" => Some(false),
            [option, value] if value == "braceexpand" && option == "-o" => Some(true),
            [option, value] if value == "braceexpand" && option == "+o" => Some(false),
            _ => None,
        })
    } else {
        None
    };
    apply_state_effect(executable, arguments, state);
    if brace_expansion.is_some() {
        state.brace_expansion = brace_expansion;
    }
}

fn apply_state_effect(executable: &str, arguments: Option<&[String]>, state: &mut State) {
    if executable == "export"
        && let Some(arguments) = arguments
    {
        let arguments = arguments
            .strip_prefix(&["--".to_owned()])
            .unwrap_or(arguments);
        if arguments
            .iter()
            .all(|arg| assignment(arg) || assignment(&format!("{arg}=")))
        {
            for arg in arguments {
                let (name, value) = arg
                    .split_once('=')
                    .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
                if let Some(value) = value {
                    if bindings::dynamic_variable(name) {
                        state.variables.remove(name);
                    } else {
                        state.variables.insert(name.into(), value.into());
                    }
                }
                state.exported.insert(name.into());
            }
            state.environment_unknown = true;
            return;
        }
    }
    if executable == "set"
        && arguments.is_some_and(|args| args.first().is_some_and(|arg| arg == "--"))
    {
        state.positionals = arguments.map(|args| args[1..].iter().cloned().map(Some).collect());
        return;
    }
    if executable == "shift" {
        let count = arguments.and_then(|args| match args {
            [] => Some(1),
            [count] if count.bytes().all(|byte| byte.is_ascii_digit()) => count.parse().ok(),
            _ => None,
        });
        if let (Some(count), Some(positionals)) = (count, state.positionals.as_mut()) {
            if count <= positionals.len() {
                positionals.drain(..count);
            }
        } else {
            state.positionals = None;
        }
        return;
    }
    if matches!(executable, "shift" | "set" | "eval" | "source" | ".") {
        state.positionals = None;
    }
    match executable {
        "cd" | "pushd" | "popd" => {
            state.cwd_unknown = true;
            state.variables.remove("PWD");
            state.variables.remove("OLDPWD");
        }
        "eval" | "source" | "." => {
            state.brace_expansion = None;
            state.cwd_unknown = true;
            state.environment_unknown = true;
            state.variables.clear();
            state.exported.clear();
            state.locals.iter_mut().for_each(BTreeMap::clear);
            state.defined_functions.clear();
        }
        name if name.contains('=')
            || bindings::unknown_builtin(name)
            || name == "printf"
            || matches!(
                name,
                "export"
                    | "unset"
                    | "set"
                    | "alias"
                    | "unalias"
                    | "read"
                    | "readonly"
                    | "declare"
                    | "local"
                    | "trap"
            ) =>
        {
            state.cwd_unknown |= bindings::unknown_builtin(name);
            state.brace_expansion = None;
            state.environment_unknown = true;
            state.variables.clear();
            state.exported.clear();
            state.locals.iter_mut().for_each(BTreeMap::clear);
            state.defined_functions.clear();
        }
        _ => {}
    }
}

pub(super) fn taint(node: &Node, state: &mut State) {
    match node {
        Node::Command { words, .. } => {
            if words.is_empty() {
                return;
            }
            for word in words.iter().take_while(|word| assignment(&word.raw)) {
                if let Some((name, _)) = word.raw.split_once('=') {
                    state.variables.remove(name);
                }
            }
            if words[0].literal.is_none() {
                state.brace_expansion = None;
                state.cwd_unknown = true;
                state.environment_unknown = true;
                state.variables.clear();
            } else if let Some(name) = words[0].literal.as_deref() {
                state_effect(name, None, state);
            } else {
                state.cwd_unknown = true;
                state.environment_unknown = true;
            }
        }
        Node::Sequence { children } => {
            for child in children {
                taint(child, state);
            }
        }
        Node::Binary { left, right, .. } => {
            taint(left, state);
            taint(right, state);
        }
        Node::Group {
            subshell: false,
            body,
        }
        | Node::Decorated { body, .. }
        | Node::Redirected { body, .. } => taint(body, state),
        Node::Conditional { condition, yes, no } => {
            taint(condition, state);
            taint(yes, state);
            if let Some(no) = no {
                taint(no, state);
            }
        }
        Node::Loop {
            condition, body, ..
        } => {
            taint(condition, state);
            taint(body, state);
        }
        Node::For { body, .. } | Node::ArithmeticLoop { body, .. } => {
            state.environment_unknown = true;
            taint(body, state);
        }
        Node::Case { arms, .. } => {
            for arm in arms {
                taint(&arm.body, state);
            }
        }
        Node::Opaque { .. } => {
            state.cwd_unknown = true;
            state.environment_unknown = true;
        }
        Node::Expression { .. } => state.environment_unknown = true,
        Node::Function { .. } | Node::Group { subshell: true, .. } | Node::Background { .. } => {}
    }
}
