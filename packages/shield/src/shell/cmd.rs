//! Closed cmd literal grammar; expansion, batches and nested syntax stay opaque.
use super::{
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
        if matches!(character, '&' | '|') {
            input.next();
            let doubled = input.peek().is_some_and(|(_, next)| *next == character);
            if doubled {
                input.next();
            }
            let token = match (character, doubled) {
                ('&', false) => Token::Separator,
                ('&', true) => Token::And,
                ('|', true) => Token::Or,
                _ => return Ok(None),
            };
            if matches!(token, Token::Separator)
                && (tokens.is_empty()
                    || matches!(
                        tokens.last(),
                        Some(Token::Separator | Token::And | Token::Or | Token::Call(_))
                    ))
            {
                return Ok(None);
            }
            native::push(&mut tokens, token)?;
            continue;
        }
        let quoted = character == '"';
        if quoted {
            input.next();
        }
        let mut literal = String::new();
        let mut end = source.len();
        let mut closed = !quoted;
        let mut scanned = 0_usize;
        while let Some(&(at, character)) = input.peek() {
            if scanned.is_multiple_of(128) {
                checkpoint()?;
            }
            scanned += 1;
            if quoted && character == '"' {
                input.next();
                end = at + 1;
                closed = true;
                break;
            }
            if !quoted && matches!(character, ' ' | '\t' | '&' | '|') {
                end = at;
                break;
            }
            if character.is_control()
                || matches!(character, '%' | '!' | '^')
                || !(quoted
                    || character.is_ascii_alphanumeric()
                    || "._:/\\=-+'".contains(character))
            {
                return Ok(None);
            }
            input.next();
            literal.push(character);
        }
        if !closed
            || literal.is_empty()
            || (quoted && literal.ends_with('\\'))
            || (quoted
                && input
                    .peek()
                    .is_some_and(|(_, next)| !matches!(next, ' ' | '\t' | '&' | '|')))
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
    if matches!(tokens.last(), Some(Token::Separator)) {
        return Ok(None);
    }
    native::parse(tokens, false)
}
