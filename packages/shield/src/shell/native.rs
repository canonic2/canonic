//! Compose dedicated Windows literal tokens without POSIX reinterpretation.
use super::structure::{Node, Span, Word};
use crate::policy::Error;

#[cfg(test)]
mod tests;

pub(super) enum Token {
    Word(Word),
    Separator,
    And,
    Or,
    Call(Span),
}

pub(super) fn push(tokens: &mut Vec<Token>, token: Token) -> Result<(), Error> {
    if tokens.len() >= 2048 {
        return Err(Error::Limit);
    }
    tokens.push(token);
    Ok(())
}

fn executable(word: &Word, called: bool) -> bool {
    (called || word.raw == word.literal.as_deref().unwrap_or_default())
        && matches!(
            word.literal.as_deref(),
            Some("git" | "rg" | "node" | "rustc" | "cargo" | "make" | "just" | "shield")
        )
}

pub(super) fn parse(tokens: Vec<Token>, calls: bool) -> Result<Option<Node>, Error> {
    let mut tokens = tokens.into_iter().peekable();
    let mut children = Vec::new();
    while tokens.peek().is_some() {
        if matches!(tokens.peek(), Some(Token::Separator)) {
            tokens.next();
            continue;
        }
        let Some(mut node) = command(&mut tokens, calls)? else {
            return Ok(None);
        };
        let mut depth = 0;
        while matches!(tokens.peek(), Some(Token::And | Token::Or)) {
            depth += 1;
            if depth > 32 {
                return Err(Error::Limit);
            }
            let operator = if matches!(tokens.next(), Some(Token::And)) {
                "&&"
            } else {
                "||"
            };
            let Some(right) = command(&mut tokens, calls)? else {
                return Ok(None);
            };
            node = Node::Binary {
                operator: operator.into(),
                left: Box::new(node),
                right: Box::new(right),
            };
        }
        children.push(node);
        if children.len() > 256 {
            return Err(Error::Limit);
        }
        if !matches!(tokens.peek(), None | Some(Token::Separator)) {
            return Ok(None);
        }
    }
    Ok(match children.len() {
        0 => None,
        1 => children.pop(),
        _ => Some(Node::Sequence { children }),
    })
}

fn command(
    tokens: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    calls: bool,
) -> Result<Option<Node>, Error> {
    let call = if matches!(tokens.peek(), Some(Token::Call(_))) {
        let Some(Token::Call(span)) = tokens.next() else {
            unreachable!()
        };
        if !calls {
            return Ok(None);
        }
        Some(span)
    } else {
        None
    };
    let mut words = Vec::new();
    while matches!(tokens.peek(), Some(Token::Word(_))) {
        let Some(Token::Word(word)) = tokens.next() else {
            unreachable!()
        };
        words.push(word);
        if words.len() > 257 {
            return Err(Error::Limit);
        }
    }
    let Some(first) = words
        .first()
        .filter(|first| executable(first, call.is_some()))
    else {
        return Ok(None);
    };
    let span = Span {
        start: call.map_or(first.span.start, |span| span.start),
        end: words.last().expect("first word").span.end,
    };
    let command = Node::Command {
        span,
        words,
        redirections: Vec::new(),
    };
    Ok(Some(if call.is_some() {
        Node::Decorated {
            operator: "powershell-call".into(),
            body: Box::new(command),
        }
    } else {
        command
    }))
}
