//! Shell grammar and control-flow construction over scanned tokens.
use super::lexer::{fragments, substitutions};
use super::{
    CaseArm, Control, Dialect, Error, Node, Redirection, Span, Token, Word, validate_depth,
};
use crate::shell::words::{identifier as shell_identifier, literal_words};

pub(super) fn parse(
    tokens: Vec<Token>,
    dialect: Dialect,
    control: Control<'_>,
) -> Result<Node, Error> {
    let mut parser = Parser {
        tokens,
        position: 0,
        dialect,
        depth: 0,
        control,
    };
    let result = parser.list(&[])?;
    if parser.position != parser.tokens.len() {
        return Err(Error::Contract("unexpected shell delimiter"));
    }
    validate_depth(&result, &parser.control)?;
    Ok(result)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    dialect: Dialect,
    depth: usize,
    control: Control<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> &str {
        self.tokens
            .get(self.position)
            .map_or("", |token| token.raw.as_str())
    }

    fn take(&mut self, value: &str) -> bool {
        if self.peek() == value {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn require(&mut self, value: &str) -> Result<(), Error> {
        if self.take(value) {
            Ok(())
        } else {
            Err(Error::Contract("missing shell delimiter"))
        }
    }

    fn list(&mut self, stops: &[&str]) -> Result<Node, Error> {
        (self.control.checkpoint)()?;
        self.depth += 1;
        if self.depth > 32 {
            return Err(Error::Limit);
        }
        let mut children = Vec::new();
        while self.take("\n") {}
        while !self.peek().is_empty() && !stops.contains(&self.peek()) {
            let mut child = self.and_or()?;
            if self.take("&") {
                child = Node::Background {
                    body: Box::new(child),
                };
            } else if !self.take(";") && !self.take("\n") {
                if !self.peek().is_empty() && !matches!(self.peek(), ")" | ";;" | ";&" | ";;&") {
                    return Err(Error::Contract("missing shell list separator"));
                }
                children.push(child);
                break;
            }
            children.push(child);
            while self.take("\n") {}
        }
        self.depth -= 1;
        if children.is_empty() {
            return Err(Error::Contract("empty shell list"));
        }
        Ok(if children.len() == 1 {
            children.remove(0)
        } else {
            Node::Sequence { children }
        })
    }

    fn and_or(&mut self) -> Result<Node, Error> {
        let mut left = self.pipeline()?;
        while matches!(self.peek(), "&&" | "||") {
            let operator = self.peek().to_owned();
            self.position += 1;
            while self.take("\n") {}
            left = Node::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(self.pipeline()?),
            };
        }
        Ok(left)
    }

    fn pipeline(&mut self) -> Result<Node, Error> {
        let mut left = self.command()?;
        while matches!(self.peek(), "|" | "|&") {
            let operator = self.peek().to_owned();
            self.position += 1;
            while self.take("\n") {}
            left = Node::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(self.command()?),
            };
        }
        Ok(left)
    }

    fn command(&mut self) -> Result<Node, Error> {
        let body = self.command_body()?;
        let mut redirections = Vec::new();
        loop {
            let descriptor = self
                .tokens
                .get(self.position)
                .filter(|token| {
                    !token.raw.is_empty()
                        && token.raw.bytes().all(|byte| byte.is_ascii_digit())
                        && self.tokens.get(self.position + 1).is_some_and(|next| {
                            token.span.end == next.span.start && redirection_operator(&next.raw)
                        })
                })
                .map(|token| token.raw.clone());
            if descriptor.is_some() {
                self.position += 1;
            }
            if !redirection_operator(self.peek()) {
                break;
            }
            redirections.push(self.redirection(descriptor)?);
        }
        Ok(if redirections.is_empty() {
            body
        } else {
            Node::Redirected {
                body: Box::new(body),
                redirections,
            }
        })
    }

    fn command_body(&mut self) -> Result<Node, Error> {
        if self.peek() == "[[" {
            self.position += 1;
            return Ok(Node::Expression {
                syntax: "bash-test".into(),
                words: self.expression_words("[[", "]]")?,
            });
        }
        if self.dialect == Dialect::Bash
            && self.peek() == "("
            && self.tokens.get(self.position + 1).is_some_and(|token| {
                token.raw == "(" && self.tokens[self.position].span.end == token.span.start
            })
        {
            self.position += 2;
            return Ok(Node::Expression {
                syntax: "arithmetic-command".into(),
                words: self.expression_words("(", ")")?,
            });
        }
        if self.take("!") || self.take("time") {
            let mut operator = self.tokens[self.position - 1].raw.clone();
            if operator == "time" && self.take("-p") {
                operator.push_str(" -p");
            }
            self.depth += 1;
            if self.depth > 32 {
                return Err(Error::Limit);
            }
            let body = self.pipeline()?;
            self.depth -= 1;
            return Ok(Node::Decorated {
                operator,
                body: Box::new(body),
            });
        }
        if self.take("coproc") {
            return self.coprocess();
        }
        if matches!(self.peek(), "for" | "select") {
            let keyword = self.peek().to_owned();
            self.position += 1;
            return self.for_loop(keyword);
        }
        if self.take("case") {
            return self.case();
        }
        if self.take("function") {
            return self.function();
        }
        if self
            .tokens
            .get(self.position + 1)
            .is_some_and(|token| token.raw == "(")
            && self
                .tokens
                .get(self.position + 2)
                .is_some_and(|token| token.raw == ")")
        {
            return self.function();
        }
        if self.take("(") || self.take("{") {
            let subshell = self.tokens[self.position - 1].raw == "(";
            let body = self.list(&[if subshell { ")" } else { "}" }])?;
            self.require(if subshell { ")" } else { "}" })?;
            return Ok(Node::Group {
                subshell,
                body: Box::new(body),
            });
        }
        if self.take("if") {
            return self.conditional();
        }
        if self.take("while") || self.take("until") {
            let keyword = self.tokens[self.position - 1].raw.clone();
            let condition = Box::new(self.list(&["do"])?);
            self.require("do")?;
            let body = Box::new(self.list(&["done"])?);
            self.require("done")?;
            return Ok(Node::Loop {
                keyword,
                condition,
                body,
            });
        }
        self.simple_command()
    }

    /// Bash's `coproc [NAME] COMMAND`, after the `coproc` keyword.
    fn coprocess(&mut self) -> Result<Node, Error> {
        // `coproc [NAME] COMMAND` runs COMMAND asynchronously; NAME is present
        // only before a compound command.
        let mut operator = "coproc".to_owned();
        if shell_identifier(self.peek())
            && self.tokens.get(self.position + 1).is_some_and(|token| {
                matches!(
                    token.raw.as_str(),
                    "{" | "(" | "[[" | "if" | "while" | "until" | "for" | "select" | "case"
                )
            })
        {
            operator.push(' ');
            operator.push_str(self.peek());
            self.position += 1;
        }
        self.depth += 1;
        if self.depth > 32 {
            return Err(Error::Limit);
        }
        let body = self.command()?;
        self.depth -= 1;
        Ok(Node::Decorated {
            operator,
            body: Box::new(Node::Background {
                body: Box::new(body),
            }),
        })
    }

    fn simple_command(&mut self) -> Result<Node, Error> {
        let start = self
            .tokens
            .get(self.position)
            .ok_or(Error::Contract("missing shell command"))?
            .span
            .start;
        let mut words: Vec<Word> = Vec::new();
        let mut redirections = Vec::new();
        while let Some(token) = self.tokens.get(self.position) {
            if redirection_operator(&token.raw) {
                let descriptor = if words.last().is_some_and(|word| {
                    word.span.end == token.span.start
                        && !word.raw.is_empty()
                        && word.raw.bytes().all(|byte| byte.is_ascii_digit())
                }) {
                    words.pop().map(|word| word.raw)
                } else {
                    None
                };
                redirections.push(self.redirection(descriptor)?);
                continue;
            }
            if !token.word
                || (words.is_empty()
                    && matches!(
                        token.raw.as_str(),
                        "}" | "then" | "else" | "elif" | "fi" | "do" | "done" | "esac"
                    ))
            {
                break;
            }
            let literal = literal_words(&token.raw, self.dialect)?.and_then(|mut words| {
                if words.len() == 1 { words.pop() } else { None }
            });
            words.push(Word {
                span: token.span,
                raw: token.raw.clone(),
                literal,
                substitutions: substitutions(&token.raw, token.span.start, &self.control)?,
                fragments: fragments(&token.raw, token.span.start, &self.control)?,
                resolved: false,
            });
            if words.len() > 257 {
                return Err(Error::Limit);
            }
            self.position += 1;
        }
        let end = self
            .tokens
            .get(self.position.saturating_sub(1))
            .ok_or(Error::Contract("missing shell command"))?
            .span
            .end;
        if words.is_empty() && redirections.is_empty() {
            return Err(Error::Contract("missing shell command"));
        }
        Ok(Node::Command {
            span: Span { start, end },
            words,
            redirections,
        })
    }

    fn redirection(&mut self, descriptor: Option<String>) -> Result<Redirection, Error> {
        let token = &self.tokens[self.position];
        let operator = token.raw.clone();
        let start = token.span.start;
        self.position += 1;
        let heredoc = self
            .tokens
            .get(self.position)
            .and_then(|token| token.heredoc.clone());
        let target = self.word()?;
        Ok(Redirection {
            span: Span {
                start,
                end: target.span.end,
            },
            descriptor,
            operator,
            target,
            heredoc,
        })
    }

    fn function(&mut self) -> Result<Node, Error> {
        let name = self
            .word()?
            .literal
            .ok_or(Error::Contract("literal function name required"))?;
        if !shell_identifier(&name) {
            return Err(Error::Contract("invalid function name"));
        }
        if self.take("(") {
            self.require(")")?;
        }
        while self.take("\n") {}
        // Use the list depth budget for recursive function-definition parsing.
        self.depth += 1;
        if self.depth > 32 {
            return Err(Error::Limit);
        }
        let body = self.command()?;
        self.depth -= 1;
        if !compound(&body) {
            return Err(Error::Contract("function requires a compound body"));
        }
        Ok(Node::Function {
            name,
            body: Box::new(body),
        })
    }

    fn word(&mut self) -> Result<Word, Error> {
        let token = self
            .tokens
            .get(self.position)
            .ok_or(Error::Contract("missing shell word"))?;
        if !token.word {
            return Err(Error::Contract("expected shell word"));
        }
        let literal = literal_words(&token.raw, self.dialect)?
            .and_then(|mut words| if words.len() == 1 { words.pop() } else { None });
        let word = Word {
            span: token.span,
            raw: token.raw.clone(),
            literal,
            substitutions: substitutions(&token.raw, token.span.start, &self.control)?,
            fragments: fragments(&token.raw, token.span.start, &self.control)?,
            resolved: false,
        };
        self.position += 1;
        Ok(word)
    }

    fn for_loop(&mut self, keyword: String) -> Result<Node, Error> {
        if keyword == "for" && self.take("(") {
            self.require("(")?;
            let header = self.expression_words("(", ")")?;
            if !self.take(";") && !self.take("\n") {
                return Err(Error::Contract("arithmetic loop separator required"));
            }
            while self.take("\n") {}
            self.require("do")?;
            let body = self.list(&["done"])?;
            self.require("done")?;
            return Ok(Node::ArithmeticLoop {
                header,
                body: Box::new(body),
            });
        }
        let variable = self
            .word()?
            .literal
            .ok_or(Error::Contract("literal loop variable required"))?;
        if !shell_identifier(&variable) {
            return Err(Error::Contract("invalid loop variable"));
        }
        let mut values = Vec::new();
        let implicit_positionals = !self.take("in");
        if !implicit_positionals {
            while !matches!(self.peek(), "" | ";" | "\n") {
                values.push(self.word()?);
            }
        }
        if !self.take(";") && !self.take("\n") {
            return Err(Error::Contract("loop header separator required"));
        }
        while self.take("\n") {}
        self.require("do")?;
        let body = self.list(&["done"])?;
        self.require("done")?;
        Ok(Node::For {
            keyword,
            variable,
            values,
            implicit_positionals,
            body: Box::new(body),
        })
    }

    fn expression_words(&mut self, open: &str, close: &str) -> Result<Vec<Word>, Error> {
        let mut depth = 1usize;
        let mut words = Vec::new();
        loop {
            let token = self
                .tokens
                .get(self.position)
                .ok_or(Error::Contract("unfinished shell expression"))?;
            if token.raw == open {
                depth += 1;
            }
            if token.raw == close {
                depth -= 1;
                if depth == 0 {
                    self.position += 1;
                    if close == ")" {
                        self.require(")")?;
                    }
                    return Ok(words);
                }
            }
            if depth > 32 || words.len() >= 257 {
                return Err(Error::Limit);
            }
            words.push(Word {
                span: token.span,
                raw: token.raw.clone(),
                literal: None,
                substitutions: substitutions(&token.raw, token.span.start, &self.control)?,
                fragments: fragments(&token.raw, token.span.start, &self.control)?,
                resolved: false,
            });
            self.position += 1;
        }
    }

    fn case(&mut self) -> Result<Node, Error> {
        let subject = self.word()?;
        while self.take("\n") {}
        self.require("in")?;
        let mut arms = Vec::new();
        while self.take("\n") {}
        while !self.take("esac") {
            self.take("(");
            let mut patterns = Vec::new();
            // Pattern matching is not used to prune reachable arms.
            while !self.take(")") {
                if self.peek().is_empty() {
                    return Err(Error::Contract("unfinished case pattern"));
                }
                if self.take("|") {
                    continue;
                }
                patterns.push(self.word()?);
            }
            let body = if matches!(self.peek(), ";;" | ";&" | ";;&" | "esac") {
                Node::Sequence {
                    children: Vec::new(),
                }
            } else {
                self.list(&[";;", ";&", ";;&", "esac"])?
            };
            let terminator = self.peek().to_owned();
            arms.push(CaseArm {
                patterns,
                body,
                terminator,
            });
            if matches!(self.peek(), ";;" | ";&" | ";;&") {
                self.position += 1;
            } else {
                self.require("esac")?;
                break;
            }
            while self.take("\n") {}
        }
        Ok(Node::Case { subject, arms })
    }

    fn conditional(&mut self) -> Result<Node, Error> {
        let condition = Box::new(self.list(&["then"])?);
        self.require("then")?;
        let yes = Box::new(self.list(&["else", "elif", "fi"])?);
        let no = if self.take("else") {
            Some(Box::new(self.list(&["fi"])?))
        } else if self.take("elif") {
            return Ok(Node::Conditional {
                condition,
                yes,
                no: Some(Box::new(self.conditional()?)),
            });
        } else {
            None
        };
        self.require("fi")?;
        Ok(Node::Conditional { condition, yes, no })
    }
}

fn compound(node: &Node) -> bool {
    match node {
        Node::Redirected { body, .. } => compound(body),
        Node::Group { .. }
        | Node::Conditional { .. }
        | Node::Case { .. }
        | Node::For { .. }
        | Node::Loop { .. }
        | Node::ArithmeticLoop { .. }
        | Node::Expression { .. } => true,
        _ => false,
    }
}

fn redirection_operator(value: &str) -> bool {
    matches!(
        value,
        "<" | ">" | ">>" | "<<" | "<<-" | "<&" | ">&" | "<>" | ">|" | "<<<" | "&>" | "&>>"
    )
}
