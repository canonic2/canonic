//! Bounded shell control-flow parsing; words retain raw source and byte boundaries.

use super::Dialect;
use crate::policy::Error;
use serde::Serialize;
use std::cell::Cell;
mod lexer;
mod parser;
use lexer::lex;
pub(super) use lexer::{heredoc_span, heredoc_substitutions};

struct Control<'a> {
    checkpoint: &'a dyn Fn() -> Result<(), Error>,
    steps: Cell<usize>,
}

impl Control<'_> {
    fn tick(&self) -> Result<(), Error> {
        let steps = self.steps.get();
        self.steps.set(steps + 1);
        if steps.is_multiple_of(128) {
            (self.checkpoint)()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum Node {
    Opaque {
        span: Span,
    },
    Command {
        span: Span,
        words: Vec<Word>,
        redirections: Vec<Redirection>,
    },
    Sequence {
        children: Vec<Self>,
    },
    Binary {
        operator: String,
        left: Box<Self>,
        right: Box<Self>,
    },
    Group {
        subshell: bool,
        body: Box<Self>,
    },
    Background {
        body: Box<Self>,
    },
    Decorated {
        operator: String,
        body: Box<Self>,
    },
    Redirected {
        body: Box<Self>,
        redirections: Vec<Redirection>,
    },
    Conditional {
        condition: Box<Self>,
        yes: Box<Self>,
        no: Option<Box<Self>>,
    },
    Loop {
        keyword: String,
        condition: Box<Self>,
        body: Box<Self>,
    },
    For {
        keyword: String,
        variable: String,
        values: Vec<Word>,
        implicit_positionals: bool,
        body: Box<Self>,
    },
    ArithmeticLoop {
        header: Vec<Word>,
        body: Box<Self>,
    },
    Expression {
        syntax: String,
        words: Vec<Word>,
    },
    Function {
        name: String,
        body: Box<Self>,
    },
    Case {
        subject: Word,
        arms: Vec<CaseArm>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct CaseArm {
    pub(super) patterns: Vec<Word>,
    pub(super) body: Node,
    pub(super) terminator: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Word {
    pub(super) span: Span,
    pub(super) raw: String,
    pub(super) literal: Option<String>,
    pub(super) substitutions: Vec<Substitution>,
    pub(super) fragments: Vec<Fragment>,
    #[serde(skip)]
    pub(super) resolved: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Fragment {
    pub(super) kind: &'static str,
    span: Span,
    pub(super) raw: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Redirection {
    pub(super) span: Span,
    pub(super) descriptor: Option<String>,
    pub(super) operator: String,
    pub(super) target: Word,
    pub(super) heredoc: Option<HereDoc>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct HereDoc {
    pub(super) span: Span,
    pub(super) source: String,
    pub(super) raw_source: String,
    pub(super) expand: bool,
    pub(super) source_map: Vec<SourceMapping>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceMapping {
    decoded: Span,
    raw: Span,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Substitution {
    pub(super) span: Span,
    pub(super) source: String,
    pub(super) kind: &'static str,
}

#[derive(Debug)]
struct Token {
    span: Span,
    raw: String,
    word: bool,
    heredoc: Option<HereDoc>,
}

pub(super) fn parse(
    source: &str,
    dialect: Dialect,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Node, Error> {
    let control = Control {
        checkpoint,
        steps: Cell::new(0),
    };
    let tokens = lex(source, &control)?;
    if tokens.iter().all(|token| token.raw == "\n") {
        return Ok(Node::Sequence {
            children: Vec::new(),
        });
    }
    parser::parse(tokens, dialect, control)
}

// Bound AST depth before recursive state inspection or JSON conversion, including
// left-associated lists/pipelines that do not recurse while being parsed.
fn validate_depth(root: &Node, control: &Control<'_>) -> Result<(), Error> {
    let mut pending = vec![(root, 0)];
    while let Some((node, depth)) = pending.pop() {
        control.tick()?;
        if depth > 32 {
            return Err(Error::Limit);
        }
        match node {
            Node::Command { .. } | Node::Opaque { .. } | Node::Expression { .. } => {}
            Node::Sequence { children } => {
                pending.extend(children.iter().map(|child| (child, depth + 1)));
            }
            Node::Group { body, .. }
            | Node::Background { body }
            | Node::Decorated { body, .. }
            | Node::Redirected { body, .. }
            | Node::For { body, .. }
            | Node::ArithmeticLoop { body, .. }
            | Node::Function { body, .. } => pending.push((body, depth + 1)),
            Node::Binary { left, right, .. } => {
                pending.extend([(left.as_ref(), depth + 1), (right.as_ref(), depth + 1)]);
            }
            Node::Conditional { condition, yes, no } => {
                pending.extend([(condition.as_ref(), depth + 1), (yes.as_ref(), depth + 1)]);
                if let Some(no) = no {
                    pending.push((no, depth + 1));
                }
            }
            Node::Loop {
                condition, body, ..
            } => {
                pending.extend([(condition.as_ref(), depth + 1), (body.as_ref(), depth + 1)]);
            }
            Node::Case { arms, .. } => {
                pending.extend(arms.iter().map(|arm| (&arm.body, depth + 1)));
            }
        }
    }
    Ok(())
}
