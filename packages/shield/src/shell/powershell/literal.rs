//! Version-invariant PowerShell native literals, sequences and call operators.
use super::super::{
    native::{self, Token},
    structure::{Node, Span, Word},
};
use crate::policy::Error;

pub(super) fn parse(
    source: &str,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Option<Node>, Error> {
    let mut input = source.char_indices().peekable();
    let mut tokens = Vec::new();
    while let Some(&(start, character)) = input.peek() {
        checkpoint()?;
        if matches!(character, ' ' | '\t') {
            input.next();
            continue;
        }
        if matches!(character, ';' | '\r' | '\n') {
            input.next();
            if character == '\r' && input.peek().is_some_and(|(_, next)| *next == '\n') {
                input.next();
            }
            native::push(&mut tokens, Token::Separator)?;
            continue;
        }
        if character == '&' {
            input.next();
            if input.peek().is_some_and(|(_, next)| *next == '&') {
                return Ok(None);
            }
            native::push(
                &mut tokens,
                Token::Call(Span {
                    start,
                    end: start + 1,
                }),
            )?;
            continue;
        }
        let quote = matches!(character, '\'' | '"').then_some(character);
        if quote.is_some() {
            input.next();
        }
        let mut literal = String::new();
        let mut end = source.len();
        let mut closed = quote.is_none();
        let mut scanned = 0_usize;
        while let Some(&(at, character)) = input.peek() {
            if scanned.is_multiple_of(128) {
                checkpoint()?;
            }
            scanned += 1;
            if Some(character) == quote {
                input.next();
                if quote == Some('\'') && input.peek().is_some_and(|(_, next)| *next == '\'') {
                    input.next();
                    literal.push('\'');
                    continue;
                }
                end = at + character.len_utf8();
                closed = true;
                break;
            }
            if quote.is_none() && matches!(character, ' ' | '\t' | ';' | '\r' | '\n' | '&') {
                end = at;
                break;
            }
            if !character.is_ascii()
                || character.is_control()
                || character == '"'
                || (quote == Some('"') && matches!(character, '$' | '`'))
                || (quote.is_none()
                    && !(character.is_ascii_alphanumeric() || "._:/\\=-+".contains(character)))
            {
                return Ok(None);
            }
            input.next();
            literal.push(character);
        }
        if !closed {
            return Err(Error::Contract("unterminated PowerShell literal quote"));
        }
        if literal.is_empty()
            || literal == "--%"
            || (literal.contains(' ') && literal.ends_with('\\'))
            || (quote.is_some()
                && input
                    .peek()
                    .is_some_and(|(_, next)| !matches!(next, ' ' | '\t' | ';' | '\r' | '\n' | '&')))
        {
            return Ok(None);
        }
        native::push(
            &mut tokens,
            Token::Word(Word {
                span: Span { start, end },
                raw: source[start..end].into(),
                literal: Some(literal),
                substitutions: Vec::new(),
                fragments: Vec::new(),
                resolved: true,
                private_evidence: false,
            }),
        )?;
    }
    native::parse(tokens, true)
}
