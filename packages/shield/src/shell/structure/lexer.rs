//! Token, quote, substitution and heredoc scanning with original byte mappings.
use super::{Control, Error, Fragment, HereDoc, SourceMapping, Span, Substitution, Token};
use std::cell::Cell;

// Expansions stay within a word, never becoming outer control flow. Supported
// substitution bodies and heredoc source are retained for separate inspection.
pub(super) fn lex(source: &str, control: &Control<'_>) -> Result<Vec<Token>, Error> {
    let mut result: Vec<Token> = Vec::new();
    let mut pending = Vec::new();
    let mut delimiter = None;
    let bytes = source.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        control.tick()?;
        if matches!(bytes[at], b' ' | b'\t') {
            at += 1;
            continue;
        }
        if bytes[at] == b'#' {
            while at < bytes.len() && bytes[at] != b'\n' {
                control.tick()?;
                at += 1;
            }
            continue;
        }
        let start = at;
        let word = !b";\n&|()<>".contains(&bytes[at])
            || (matches!(bytes[at], b'<' | b'>') && bytes.get(at + 1) == Some(&b'('));
        if word {
            scan_word_depth(source, &mut at, 0, control)?;
            if source[start..at].replace("\\\n", "").is_empty() {
                continue;
            }
        } else {
            at += [
                ";;&", "&>>", "<<<", "<<-", "&&", "||", "|&", ";;", ";&", "<&", ">&", "<>", ">|",
                ">>", "<<", "&>",
            ]
            .into_iter()
            .find(|operator| source[start..].starts_with(operator))
            .map_or(1, str::len);
        }
        if result.len() >= 2048 {
            return Err(Error::Limit);
        }
        result.push(Token {
            span: Span { start, end: at },
            raw: source[start..at].into(),
            word,
            heredoc: None,
        });
        if let Some(strip_tabs) = delimiter.take() {
            if !word {
                return Err(Error::Contract("heredoc requires a delimiter"));
            }
            let (name, expand) = heredoc_delimiter(&source[start..at], control)?;
            pending.push((result.len() - 1, name, expand, strip_tabs));
        }
        if matches!(&source[start..at], "<<" | "<<-") {
            delimiter = Some(&source[start..at] == "<<-");
        }
        if bytes[start] == b'\n' {
            for (index, name, expand, strip_tabs) in pending.drain(..) {
                result[index].heredoc = Some(read_heredoc(
                    source, &mut at, &name, expand, strip_tabs, control,
                )?);
            }
        }
    }
    if delimiter.is_some() || !pending.is_empty() {
        return Err(Error::Contract("unfinished heredoc"));
    }
    Ok(result)
}

fn heredoc_delimiter(raw: &str, control: &Control<'_>) -> Result<(String, bool), Error> {
    let mut name = String::new();
    let mut quote = None;
    let mut expand = true;
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        control.tick()?;
        if quote == Some(ch) {
            quote = None;
            expand = false;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            expand = false;
        } else if ch == '\\' && quote != Some('\'') {
            let next = chars
                .next()
                .ok_or(Error::Contract("unfinished heredoc delimiter"))?;
            if quote == Some('"') && !matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                name.push('\\');
            }
            if next != '\n' {
                name.push(next);
            }
            expand = false;
        } else {
            name.push(ch);
        }
    }
    Ok((name, expand))
}

fn read_heredoc(
    source: &str,
    at: &mut usize,
    delimiter: &str,
    expand: bool,
    strip_tabs: bool,
    control: &Control<'_>,
) -> Result<HereDoc, Error> {
    let start = *at;
    let mut body = String::new();
    let mut source_map = Vec::new();
    while *at < source.len() {
        control.tick()?;
        let line_start = *at;
        let mut value = String::new();
        let mut segments = Vec::new();
        let newline = loop {
            let end = source[*at..].find('\n').map_or(source.len(), |n| *at + n);
            let line = &source[*at..end];
            let trimmed = if strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            let tabs = line.len() - trimmed.len();
            let backslashes = trimmed.bytes().rev().take_while(|ch| *ch == b'\\').count();
            let continued = expand && end < source.len() && backslashes % 2 == 1;
            let length = trimmed.len() - usize::from(continued);
            let decoded_start = body.len() + value.len();
            value.push_str(&trimmed[..length]);
            let raw_start = *at + tabs;
            segments.push(SourceMapping {
                decoded: Span {
                    start: decoded_start,
                    end: body.len() + value.len(),
                },
                raw: Span {
                    start: raw_start,
                    end: raw_start + length,
                },
            });
            *at = (end + 1).min(source.len());
            if source_map.len() + segments.len() > 2048 {
                return Err(Error::Limit);
            }
            control.tick()?;
            if !continued || *at == source.len() {
                break end < source.len() && !continued;
            }
        };
        if value == delimiter {
            let result = HereDoc {
                span: Span {
                    start,
                    end: line_start,
                },
                source: body,
                raw_source: source[start..line_start].into(),
                expand,
                source_map,
            };
            return Ok(result);
        }
        body.push_str(&value);
        if newline {
            body.push('\n');
            if let Some(segment) = segments.last_mut() {
                segment.decoded.end += 1;
                segment.raw.end += 1;
            }
        }
        source_map.extend(segments);
    }
    Err(Error::Contract("unfinished heredoc body"))
}

fn scan_word_depth(
    source: &str,
    at: &mut usize,
    nesting: usize,
    control: &Control<'_>,
) -> Result<(), Error> {
    if nesting > 32 {
        return Err(Error::Limit);
    }
    let bytes = source.as_bytes();
    let mut quote = None;
    while *at < bytes.len() {
        control.tick()?;
        let ch = bytes[*at];
        if quote.is_none() && ch == b'$' && bytes.get(*at + 1) == Some(&b'\'') {
            *at += 2;
            scan_quote(source, at, b'\'', true, nesting, control)?;
            continue;
        }
        if ch == b'\\' && quote != Some(b'\'') {
            *at += 1;
            if *at == bytes.len() {
                return Err(Error::Contract("unfinished shell escape"));
            }
            *at += 1;
            continue;
        }
        if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, b'\'' | b'"' | b'`') {
            quote = Some(ch);
        } else if quote != Some(b'\'') && ch == b'$' && bytes.get(*at + 1) == Some(&b'(') {
            // A substitution is opaque. Consume its balanced body separately
            // from the surrounding double quotes, without parsing or executing it.
            *at += 2;
            scan_substitution_depth(bytes, at, nesting + 1, control)?;
            continue;
        } else if quote != Some(b'\'') && ch == b'$' && bytes.get(*at + 1) == Some(&b'{') {
            *at += 2;
            scan_parameter_depth(bytes, at, nesting + 1, control)?;
            continue;
        } else if quote.is_none() && matches!(ch, b'<' | b'>') && bytes.get(*at + 1) == Some(&b'(')
        {
            *at += 2;
            scan_substitution_depth(bytes, at, nesting + 1, control)?;
            continue;
        } else if quote.is_none() && b" \t\n;&|()<>".contains(&ch) {
            break;
        }
        *at += 1;
    }
    if quote.is_some() {
        return Err(Error::Contract("unfinished shell word"));
    }
    Ok(())
}

fn scan_parameter_depth(
    bytes: &[u8],
    at: &mut usize,
    nesting: usize,
    control: &Control<'_>,
) -> Result<(), Error> {
    if nesting > 32 {
        return Err(Error::Limit);
    }
    let mut depth = 1usize;
    let mut quote = None;
    while *at < bytes.len() {
        control.tick()?;
        let ch = bytes[*at];
        *at += 1;
        if ch == b'\\' && quote != Some(b'\'') {
            if *at == bytes.len() {
                return Err(Error::Contract("unfinished parameter escape"));
            }
            *at += 1;
        } else if quote != Some(b'\'') && ch == b'$' && bytes.get(*at) == Some(&b'(') {
            *at += 1;
            scan_substitution_depth(bytes, at, nesting + 1, control)?;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, b'\'' | b'"') {
            quote = Some(ch);
        } else if quote.is_none() && ch == b'{' {
            depth += 1;
        } else if quote.is_none() && ch == b'}' {
            depth -= 1;
            if depth == 0 {
                return Ok(());
            }
        }
        if depth > 32 {
            return Err(Error::Limit);
        }
    }
    Err(Error::Contract("unfinished shell parameter expansion"))
}

fn scan_substitution_depth(
    bytes: &[u8],
    at: &mut usize,
    nesting: usize,
    control: &Control<'_>,
) -> Result<(), Error> {
    if nesting > 32 {
        return Err(Error::Limit);
    }
    let source = substitution_source(bytes)?;
    let mut depth = 1usize;
    let mut quote = None;
    let mut boundary = SubstitutionBoundary::default();
    let mut pending = Vec::new();
    let mut arithmetic = Vec::new();
    if bytes.get(*at) == Some(&b'(') {
        arithmetic.push(depth);
    }
    while *at < bytes.len() {
        control.tick()?;
        // Nested substitution quoting belongs to its own parser environment,
        // including substitutions appearing inside an outer double quote.
        if quote != Some(b'\'')
            && bytes[*at] == b'$'
            && scan_dollar(source, at, nesting, quote.is_none(), control)?
        {
            continue;
        }
        if quote.is_none() && arithmetic.is_empty() && bytes[*at..].starts_with(b"<<") {
            if let Some(delimiter) = substitution_heredoc(source, at, nesting, control)? {
                pending.push(delimiter);
            }
            if pending.len() > 2048 {
                return Err(Error::Limit);
            }
            continue;
        }
        if quote.is_none() && boundary.token(bytes, at, depth, control)? {
            continue;
        }
        let ch = bytes[*at];
        *at += 1;
        if ch == b'\\' && quote != Some(b'\'') {
            if *at == bytes.len() {
                return Err(Error::Contract("unfinished substitution escape"));
            }
            *at += 1;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, b'\'' | b'"' | b'`') {
            quote = Some(ch);
            boundary.command_start = false;
        } else if quote.is_none() && ch == b'(' {
            if bytes.get(*at) == Some(&b'(') {
                arithmetic.push(depth);
            }
            if !boundary.pattern(depth) {
                depth += 1;
            }
            boundary.command_start = true;
        } else if quote.is_none() && ch == b')' {
            if boundary.pattern(depth) {
                boundary
                    .cases
                    .last_mut()
                    .ok_or(Error::Contract("missing case boundary"))?
                    .1 = CasePhase::Body;
                boundary.command_start = true;
                continue;
            }
            depth -= 1;
            if arithmetic.last().is_some_and(|level| depth <= *level) {
                arithmetic.pop();
            }
            if depth == 0 {
                if !pending.is_empty() {
                    return Err(Error::Contract("unfinished heredoc"));
                }
                return Ok(());
            }
        } else if quote.is_none() && b";\n&|".contains(&ch) {
            if ch == b';'
                && bytes
                    .get(*at)
                    .is_some_and(|next| matches!(next, b';' | b'&'))
                && let Some((level, phase)) = boundary.cases.last_mut()
                && *level == depth
            {
                *phase = CasePhase::Pattern;
            }
            boundary.command_start = true;
            if ch == b'\n' {
                for (name, expand, strip_tabs) in pending.drain(..) {
                    read_heredoc(source, at, &name, expand, strip_tabs, control)?;
                }
            }
        }
        if depth > 32 {
            return Err(Error::Limit);
        }
    }
    Err(Error::Contract("unfinished shell substitution"))
}

fn scan_dollar(
    source: &str,
    at: &mut usize,
    nesting: usize,
    ansi: bool,
    control: &Control<'_>,
) -> Result<bool, Error> {
    let bytes = source.as_bytes();
    match bytes.get(*at + 1) {
        Some(b'(') => {
            *at += 2;
            scan_substitution_depth(bytes, at, nesting + 1, control)?;
        }
        Some(b'{') => {
            *at += 2;
            scan_parameter_depth(bytes, at, nesting + 1, control)?;
        }
        Some(b'\'') if ansi => {
            *at += 2;
            scan_quote(source, at, b'\'', true, nesting, control)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn substitution_source(bytes: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(bytes).map_err(|_| Error::Contract("invalid substitution source"))
}

fn substitution_heredoc(
    source: &str,
    at: &mut usize,
    nesting: usize,
    control: &Control<'_>,
) -> Result<Option<(String, bool, bool)>, Error> {
    let bytes = source.as_bytes();
    if bytes.get(*at + 2) == Some(&b'<') {
        *at += 3;
        return Ok(None);
    }
    *at += 2;
    let strip_tabs = bytes.get(*at) == Some(&b'-');
    *at += usize::from(strip_tabs);
    while bytes.get(*at).is_some_and(|ch| matches!(ch, b' ' | b'\t')) {
        control.tick()?;
        *at += 1;
    }
    let start = *at;
    scan_word_depth(source, at, nesting + 1, control)?;
    if start == *at {
        return Err(Error::Contract("heredoc requires a delimiter"));
    }
    let (name, expand) = heredoc_delimiter(&source[start..*at], control)?;
    Ok(Some((name, expand, strip_tabs)))
}

#[derive(Clone, Copy, PartialEq)]
enum CasePhase {
    Subject,
    Pattern,
    Body,
}

struct SubstitutionBoundary {
    command_start: bool,
    cases: Vec<(usize, CasePhase)>,
}

impl Default for SubstitutionBoundary {
    fn default() -> Self {
        Self {
            command_start: true,
            cases: Vec::new(),
        }
    }
}

impl SubstitutionBoundary {
    fn pattern(&self, depth: usize) -> bool {
        self.cases
            .last()
            .is_some_and(|(level, phase)| *level == depth && *phase == CasePhase::Pattern)
    }

    fn token(
        &mut self,
        bytes: &[u8],
        at: &mut usize,
        depth: usize,
        control: &Control<'_>,
    ) -> Result<bool, Error> {
        let boundary = *at == 0 || b" \t\n;&|()".contains(&bytes[*at - 1]);
        if !boundary {
            return Ok(false);
        }
        if bytes[*at] == b'#' {
            while *at < bytes.len() && bytes[*at] != b'\n' {
                control.tick()?;
                *at += 1;
            }
            return Ok(true);
        }
        if !bytes[*at].is_ascii_alphabetic() {
            return Ok(false);
        }
        let start = *at;
        let mut end = start;
        while bytes
            .get(end)
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || b"_-".contains(ch))
        {
            control.tick()?;
            end += 1;
        }
        if bytes.get(end).is_some_and(|ch| !b" \t\n;&|()".contains(ch)) {
            self.command_start = false;
            return Ok(false);
        }
        let word = &bytes[start..end];
        if self.command_start && word == b"case" {
            if self.cases.len() >= 32 {
                return Err(Error::Limit);
            }
            self.cases.push((depth, CasePhase::Subject));
            self.command_start = false;
        } else if word == b"in"
            && self
                .cases
                .last()
                .is_some_and(|(level, phase)| *level == depth && *phase == CasePhase::Subject)
        {
            self.cases
                .last_mut()
                .ok_or(Error::Contract("missing case boundary"))?
                .1 = CasePhase::Pattern;
            self.command_start = true;
        } else if self.command_start
            && word == b"esac"
            && self.cases.last().is_some_and(|(level, _)| *level == depth)
        {
            self.cases.pop();
            self.command_start = false;
        } else {
            self.command_start = self.command_start
                && matches!(
                    word,
                    b"if" | b"then" | b"else" | b"elif" | b"while" | b"until" | b"do" | b"time"
                );
        }
        *at = end;
        Ok(true)
    }
}

// Preserve nested source without expanding it. Arithmetic expansion remains
// opaque: its parentheses do not denote a command source unit.
pub(super) fn substitutions(
    raw: &str,
    offset: usize,
    control: &Control<'_>,
) -> Result<Vec<Substitution>, Error> {
    substitutions_nested(raw, offset, false, 0, control)
}

pub(in crate::shell) fn heredoc_substitutions(
    raw: &str,
    offset: usize,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Vec<Substitution>, Error> {
    substitutions_nested(
        raw,
        offset,
        true,
        0,
        &Control {
            checkpoint,
            steps: Cell::new(0),
        },
    )
}

pub(in crate::shell) fn heredoc_span(body: &HereDoc, span: Span) -> Span {
    let offset = |position: usize, end: bool| {
        let position = position.saturating_sub(body.span.start);
        for segment in &body.source_map {
            if position >= segment.decoded.start
                && (position < segment.decoded.end || (end && position == segment.decoded.end))
            {
                return segment.raw.start + position - segment.decoded.start;
            }
        }
        body.span.end
    };
    Span {
        start: offset(span.start, false),
        end: offset(span.end, true),
    }
}

fn substitutions_nested(
    raw: &str,
    offset: usize,
    heredoc: bool,
    depth: usize,
    control: &Control<'_>,
) -> Result<Vec<Substitution>, Error> {
    if depth > 32 {
        return Err(Error::Limit);
    }
    let bytes = raw.as_bytes();
    let mut at = 0;
    let mut quote = None;
    let mut result = Vec::new();
    while at < bytes.len() {
        control.tick()?;
        let ch = bytes[at];
        if !heredoc && quote.is_none() && ch == b'$' && bytes.get(at + 1) == Some(&b'\'') {
            at += 2;
            scan_quote(raw, &mut at, b'\'', true, depth, control)?;
            continue;
        }
        if ch == b'\\' && quote != Some(b'\'') {
            at += 2;
            continue;
        }
        if !heredoc && quote == Some(ch) {
            quote = None;
        } else if !heredoc && quote.is_none() && matches!(ch, b'\'' | b'"') {
            quote = Some(ch);
        } else if quote != Some(b'\'') && ch == b'`' {
            result.push(backtick(raw, offset, &mut at, control)?);
            continue;
        } else if quote != Some(b'\'') && ch == b'$' && bytes.get(at + 1) == Some(&b'{') {
            let start = at;
            at += 2;
            let body_start = at;
            scan_parameter_depth(bytes, &mut at, depth, control)?;
            if bytes
                .get(body_start)
                .is_some_and(|ch| b" \t\n|".contains(ch))
            {
                let source_start = body_start + usize::from(bytes[body_start] == b'|');
                result.push(Substitution {
                    span: Span {
                        start: offset + start,
                        end: offset + at,
                    },
                    source: raw[source_start..at - 1].into(),
                    kind: "current-shell-substitution",
                });
            } else {
                result.extend(substitutions_nested(
                    &raw[body_start..at - 1],
                    offset + body_start,
                    true,
                    depth + 1,
                    control,
                )?);
            }
            continue;
        } else if quote != Some(b'\'') && ch == b'$' && bytes.get(at + 1) == Some(&b'(') {
            let start = at;
            at += 2;
            let body_start = at;
            scan_substitution_depth(bytes, &mut at, depth, control)?;
            if bytes.get(body_start) == Some(&b'(') {
                result.extend(substitutions_nested(
                    &raw[body_start..at - 1],
                    offset + body_start,
                    true,
                    depth + 1,
                    control,
                )?);
            } else {
                result.push(Substitution {
                    span: Span {
                        start: offset + start,
                        end: offset + at,
                    },
                    source: raw[body_start..at - 1].into(),
                    kind: "command-substitution",
                });
            }
            continue;
        } else if !heredoc
            && quote.is_none()
            && matches!(ch, b'<' | b'>')
            && bytes.get(at + 1) == Some(&b'(')
        {
            let start = at;
            at += 2;
            let body_start = at;
            scan_substitution_depth(bytes, &mut at, depth, control)?;
            result.push(Substitution {
                span: Span {
                    start: offset + start,
                    end: offset + at,
                },
                source: raw[body_start..at - 1].into(),
                kind: "process-substitution",
            });
            continue;
        }
        at += 1;
    }
    Ok(result)
}

fn backtick(
    raw: &str,
    offset: usize,
    at: &mut usize,
    control: &Control<'_>,
) -> Result<Substitution, Error> {
    let bytes = raw.as_bytes();
    let start = *at;
    *at += 1;
    let mut source = String::new();
    while *at < bytes.len() && bytes[*at] != b'`' {
        control.tick()?;
        if bytes[*at] == b'\\'
            && bytes
                .get(*at + 1)
                .is_some_and(|ch| matches!(ch, b'$' | b'`' | b'\\' | b'\n'))
        {
            *at += 1;
            if bytes[*at] == b'\n' {
                *at += 1;
                continue;
            }
        }
        let ch = raw[*at..]
            .chars()
            .next()
            .ok_or(Error::Contract("invalid backtick source"))?;
        source.push(ch);
        *at += ch.len_utf8();
    }
    if *at == bytes.len() {
        return Err(Error::Contract("unfinished backtick substitution"));
    }
    *at += 1;
    Ok(Substitution {
        span: Span {
            start: offset + start,
            end: offset + *at,
        },
        source,
        kind: "backtick-substitution",
    })
}

fn scan_quote(
    source: &str,
    at: &mut usize,
    quote: u8,
    ansi: bool,
    nesting: usize,
    control: &Control<'_>,
) -> Result<(), Error> {
    let bytes = source.as_bytes();
    while *at < bytes.len() {
        control.tick()?;
        let ch = bytes[*at];
        *at += 1;
        if ch == b'\\' && (quote == b'"' || ansi) {
            if *at == bytes.len() {
                return Err(Error::Contract("unfinished quoted escape"));
            }
            *at += 1;
        } else if ch == quote {
            return Ok(());
        } else if quote == b'"' && ch == b'$' && bytes.get(*at) == Some(&b'(') {
            *at += 1;
            scan_substitution_depth(bytes, at, nesting + 1, control)?;
        } else if quote == b'"' && ch == b'$' && bytes.get(*at) == Some(&b'{') {
            *at += 1;
            scan_parameter_depth(bytes, at, nesting + 1, control)?;
        } else if quote == b'"' && ch == b'`' {
            *at -= 1;
            backtick(source, 0, at, control)?;
        }
    }
    Err(Error::Contract("unfinished shell quote"))
}

pub(super) fn fragments(
    raw: &str,
    offset: usize,
    control: &Control<'_>,
) -> Result<Vec<Fragment>, Error> {
    let bytes = raw.as_bytes();
    let mut at = 0;
    let mut result = Vec::new();
    while at < bytes.len() {
        control.tick()?;
        let start = at;
        let kind = match bytes[at] {
            b'\'' | b'"' => {
                let quote = bytes[at];
                at += 1;
                scan_quote(raw, &mut at, quote, false, 0, control)?;
                if quote == b'\'' {
                    "single-quoted"
                } else {
                    "double-quoted"
                }
            }
            b'\\' => {
                at += 1;
                let ch = raw[at..]
                    .chars()
                    .next()
                    .ok_or(Error::Contract("unfinished shell escape"))?;
                at += ch.len_utf8();
                "escape"
            }
            b'$' if matches!(bytes.get(at + 1), Some(b'\'' | b'"')) => {
                let quote = bytes[at + 1];
                at += 2;
                scan_quote(raw, &mut at, quote, quote == b'\'', 0, control)?;
                if quote == b'\'' {
                    "ansi-c-quoted"
                } else {
                    "locale-quoted"
                }
            }
            b'$' if bytes.get(at + 1) == Some(&b'{') => {
                at += 2;
                scan_parameter_depth(bytes, &mut at, 0, control)?;
                "parameter-expansion"
            }
            b'$' | b'<' | b'>' if bytes.get(at + 1) == Some(&b'(') => {
                let command = bytes[at] == b'$';
                at += 2;
                let arithmetic = bytes.get(at) == Some(&b'(');
                scan_substitution_depth(bytes, &mut at, 0, control)?;
                if !command {
                    "process-substitution"
                } else if arithmetic {
                    "arithmetic-expansion"
                } else {
                    "command-substitution"
                }
            }
            b'`' => {
                backtick(raw, 0, &mut at, control)?;
                "backtick-substitution"
            }
            _ => {
                at += 1;
                while at < bytes.len()
                    && !b"\"'\\$`".contains(&bytes[at])
                    && !(matches!(bytes[at], b'<' | b'>') && bytes.get(at + 1) == Some(&b'('))
                {
                    control.tick()?;
                    at += 1;
                }
                "unquoted"
            }
        };
        if result.len() >= 2048 {
            return Err(Error::Limit);
        }
        result.push(Fragment {
            kind,
            span: Span {
                start: offset + start,
                end: offset + at,
            },
            raw: raw[start..at].into(),
        });
    }
    Ok(result)
}
