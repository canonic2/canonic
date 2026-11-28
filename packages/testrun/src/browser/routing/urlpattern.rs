//! WHATWG URLPattern component patterns compiled to Rust regexes and matched
//! as the pinned client matches a serialized `URLPattern`: each of the eight
//! components separately, case-insensitively, with omitted components `*`.
//!
//! Component patterns are ASCII. Their fixed text is matched as written, not
//! re-canonicalized through the URL parser, and custom regexp groups follow the
//! route regex translator's limits.

use serde::{Deserialize, Serialize};

const INVALID: &str = "ROUTE_URL_PATTERN_INVALID";
const MAX_COMPONENT: usize = 1024;

/// The pinned `serializeURLPattern` shape: one optional pattern per component.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlPattern {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pathname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// One regex per component, in [`UrlPattern`] field order.
pub(crate) struct Compiled([regex::bytes::Regex; 8]);

impl Compiled {
    /// Whether every component of the parsed `url` matches.
    pub(crate) fn is_match(&self, url: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else {
            return false;
        };
        let port = url.port().map(|port| port.to_string()).unwrap_or_default();
        let values = [
            url.scheme(),
            url.username(),
            url.password().unwrap_or(""),
            url.host_str().unwrap_or(""),
            &port,
            url.path(),
            url.query().unwrap_or(""),
            url.fragment().unwrap_or(""),
        ];
        self.0
            .iter()
            .zip(values)
            .all(|(component, value)| component.is_match(value.as_bytes()))
    }
}

impl UrlPattern {
    pub(crate) fn schema() -> serde_json::Value {
        let component = serde_json::json!({"type":"string","maxLength":MAX_COMPONENT});
        serde_json::json!({"type":"object","properties":{"kind":{"const":"url_pattern"},"protocol":component,"username":component,"password":component,"hostname":component,"port":component,"pathname":component,"search":component,"hash":component},"required":["kind"],"additionalProperties":false,"description":"URLPattern component patterns (omitted components match anything), matched case-insensitively"})
    }
    pub(crate) fn compile(&self) -> Result<Compiled, &'static str> {
        let sources = self.sources()?;
        let mut compiled = Vec::with_capacity(8);
        for source in sources {
            compiled.push(regex(source.as_deref().unwrap_or("^(?s:.*)$"))?);
        }
        Ok(Compiled(
            compiled.try_into().unwrap_or_else(|_| unreachable!()),
        ))
    }
    /// Each component's case-insensitive Rust regex source, in field order, or
    /// `None` for a component that is exactly `*` and so matches anything.
    pub(crate) fn sources(&self) -> Result<[Option<String>; 8], &'static str> {
        fn value(component: &Option<String>) -> &str {
            component.as_deref().unwrap_or("*")
        }
        // Pinned "process … for init": a protocol's `:`, a search's `?` and a hash's `#`.
        let protocol = value(&self.protocol);
        let protocol = protocol.strip_suffix(':').unwrap_or(protocol);
        let search = value(&self.search);
        let search = search.strip_prefix('?').unwrap_or(search);
        let hash = value(&self.hash);
        let hash = hash.strip_prefix('#').unwrap_or(hash);
        let source = |pattern: &str, options| -> Result<Option<String>, &'static str> {
            let source = component(pattern, options)?;
            Ok((pattern != "*").then_some(source))
        };
        let protocol_source = component(protocol, Options::DEFAULT)?;
        // Pathnames of special schemes use `/` segments.
        let protocol_regex = regex(&protocol_source)?;
        let special = ["ftp", "file", "http", "https", "ws", "wss"]
            .iter()
            .any(|scheme| protocol_regex.is_match(scheme.as_bytes()));
        Ok([
            (protocol != "*").then_some(protocol_source),
            source(value(&self.username), Options::DEFAULT)?,
            source(value(&self.password), Options::DEFAULT)?,
            source(value(&self.hostname), Options::HOSTNAME)?,
            source(value(&self.port), Options::DEFAULT)?,
            source(
                value(&self.pathname),
                if special {
                    Options::PATHNAME
                } else {
                    Options::DEFAULT
                },
            )?,
            source(search, Options::DEFAULT)?,
            source(hash, Options::DEFAULT)?,
        ])
    }
}

/// The pinned compile options: a segment delimiter and an implicit prefix.
#[derive(Clone, Copy)]
struct Options {
    delimiter: Option<u8>,
    prefix: Option<u8>,
}
impl Options {
    const DEFAULT: Self = Self {
        delimiter: None,
        prefix: None,
    };
    const HOSTNAME: Self = Self {
        delimiter: Some(b'.'),
        prefix: None,
    };
    const PATHNAME: Self = Self {
        delimiter: Some(b'/'),
        prefix: Some(b'/'),
    };
    fn segment_wildcard(self) -> String {
        match self.delimiter {
            Some(delimiter) => format!("[^{}]+?", escape(&(delimiter as char).to_string())),
            None => "[^]+?".into(),
        }
    }
}

/// A component pattern as Rust regex source (case-insensitive, ASCII).
fn component(pattern: &str, options: Options) -> Result<String, &'static str> {
    if pattern.len() > MAX_COMPONENT
        || !pattern.is_ascii()
        || pattern.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(INVALID);
    }
    let parts = Parser::new(tokenize(pattern)?, options).parse()?;
    super::regexp::translate_within(&generate(&parts, options), "iu", 8 * MAX_COMPONENT)
}
fn regex(source: &str) -> Result<regex::bytes::Regex, &'static str> {
    regex::bytes::RegexBuilder::new(source)
        .unicode(false)
        .case_insensitive(true)
        .size_limit(65536)
        .build()
        .map_err(|_| "ROUTE_PATTERN_BOUNDS")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Open,
    Close,
    Regexp,
    Name,
    Char,
    EscapedChar,
    OtherModifier,
    Asterisk,
    End,
}
struct Token {
    kind: Kind,
    value: String,
}

/// The pinned strict tokenizer, restricted to ASCII input.
fn tokenize(input: &str) -> Result<Vec<Token>, &'static str> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    let token = |kind, value: &[u8]| Token {
        kind,
        value: String::from_utf8_lossy(value).into_owned(),
    };
    while at < bytes.len() {
        let byte = bytes[at];
        match byte {
            b'*' => tokens.push(token(Kind::Asterisk, &bytes[at..=at])),
            b'+' | b'?' => tokens.push(token(Kind::OtherModifier, &bytes[at..=at])),
            b'\\' => {
                at += 1;
                let escaped = bytes.get(at).ok_or(INVALID)?;
                tokens.push(token(Kind::EscapedChar, std::slice::from_ref(escaped)));
            }
            b'{' => tokens.push(token(Kind::Open, &bytes[at..=at])),
            b'}' => tokens.push(token(Kind::Close, &bytes[at..=at])),
            b':' => {
                let start = at + 1;
                let mut end = start;
                while bytes.get(end).is_some_and(|byte| {
                    byte.is_ascii_alphabetic()
                        || matches!(byte, b'_' | b'$')
                        || end > start && byte.is_ascii_digit()
                }) {
                    end += 1;
                }
                if end == start {
                    return Err(INVALID);
                }
                tokens.push(token(Kind::Name, &bytes[start..end]));
                at = end;
                continue;
            }
            b'(' => {
                let start = at + 1;
                let mut end = start;
                let mut depth = 1;
                while end < bytes.len() {
                    let current = bytes[end];
                    if end == start && current == b'?' {
                        return Err(INVALID);
                    }
                    if current == b'\\' {
                        end += 2;
                        if end > bytes.len() {
                            return Err(INVALID);
                        }
                        continue;
                    }
                    if current == b')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if current == b'(' {
                        depth += 1;
                        // Only non-capturing groups may nest.
                        if bytes.get(end + 1) != Some(&b'?') {
                            return Err(INVALID);
                        }
                    }
                    end += 1;
                }
                if depth != 0 || end == start {
                    return Err(INVALID);
                }
                tokens.push(token(Kind::Regexp, &bytes[start..end]));
                at = end + 1;
                continue;
            }
            _ => tokens.push(token(Kind::Char, &bytes[at..=at])),
        }
        at += 1;
    }
    tokens.push(Token {
        kind: Kind::End,
        value: String::new(),
    });
    Ok(tokens)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modifier {
    None,
    Optional,
    ZeroOrMore,
    OneOrMore,
}
impl Modifier {
    fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Optional => "?",
            Self::ZeroOrMore => "*",
            Self::OneOrMore => "+",
        }
    }
}
enum Part {
    Fixed {
        value: String,
        modifier: Modifier,
    },
    Group {
        /// A custom regexp, or `None` for a segment wildcard; `.*` is a full wildcard.
        regexp: Option<String>,
        prefix: String,
        suffix: String,
        modifier: Modifier,
    },
}

/// The pinned "parse a pattern string" over the token list.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
    options: Options,
    parts: Vec<Part>,
    pending: String,
    names: Vec<String>,
    next_name: usize,
}
impl Parser {
    fn new(tokens: Vec<Token>, options: Options) -> Self {
        Self {
            tokens,
            at: 0,
            options,
            parts: Vec::new(),
            pending: String::new(),
            names: Vec::new(),
            next_name: 0,
        }
    }
    fn consume(&mut self, kind: Kind) -> Option<String> {
        let token = &self.tokens[self.at];
        (token.kind == kind).then(|| {
            self.at += 1;
            token.value.clone()
        })
    }
    fn consume_modifier(&mut self) -> Modifier {
        match self
            .consume(Kind::OtherModifier)
            .or_else(|| self.consume(Kind::Asterisk))
            .as_deref()
        {
            Some("?") => Modifier::Optional,
            Some("*") => Modifier::ZeroOrMore,
            Some("+") => Modifier::OneOrMore,
            _ => Modifier::None,
        }
    }
    /// A custom regexp, a wildcard (`.*`), or nothing. After a name, `*` is
    /// that name's modifier rather than a wildcard.
    fn consume_regexp_or_wildcard(&mut self, named: bool) -> Option<String> {
        self.consume(Kind::Regexp).or_else(|| {
            (!named)
                .then(|| self.consume(Kind::Asterisk).map(|_| ".*".into()))
                .flatten()
        })
    }
    fn consume_text(&mut self) -> String {
        let mut text = String::new();
        while let Some(value) = self
            .consume(Kind::Char)
            .or_else(|| self.consume(Kind::EscapedChar))
        {
            text.push_str(&value);
        }
        text
    }
    fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            self.parts.push(Part::Fixed {
                value: std::mem::take(&mut self.pending),
                modifier: Modifier::None,
            });
        }
    }
    fn add_part(
        &mut self,
        prefix: String,
        name: Option<String>,
        regexp: Option<String>,
        suffix: String,
        modifier: Modifier,
    ) -> Result<(), &'static str> {
        if name.is_none() && regexp.is_none() && modifier == Modifier::None {
            self.pending.push_str(&prefix);
            return Ok(());
        }
        self.flush_pending();
        if name.is_none() && regexp.is_none() {
            if !prefix.is_empty() {
                self.parts.push(Part::Fixed {
                    value: prefix,
                    modifier,
                });
            }
            return Ok(());
        }
        let name = name.unwrap_or_else(|| {
            self.next_name += 1;
            (self.next_name - 1).to_string()
        });
        if self.names.contains(&name) {
            return Err(INVALID);
        }
        self.names.push(name);
        let regexp = regexp.filter(|regexp| *regexp != self.options.segment_wildcard());
        self.parts.push(Part::Group {
            regexp,
            prefix,
            suffix,
            modifier,
        });
        Ok(())
    }
    fn parse(mut self) -> Result<Vec<Part>, &'static str> {
        let implicit_prefix = self
            .options
            .prefix
            .map(|prefix| (prefix as char).to_string())
            .unwrap_or_default();
        loop {
            let character = self.consume(Kind::Char);
            let name = self.consume(Kind::Name);
            let regexp = self.consume_regexp_or_wildcard(name.is_some());
            if name.is_some() || regexp.is_some() {
                let mut prefix = character.unwrap_or_default();
                if !prefix.is_empty() && prefix != implicit_prefix {
                    self.pending.push_str(&prefix);
                    prefix.clear();
                }
                self.flush_pending();
                let modifier = self.consume_modifier();
                self.add_part(prefix, name, regexp, String::new(), modifier)?;
                continue;
            }
            if let Some(fixed) = character.or_else(|| self.consume(Kind::EscapedChar)) {
                self.pending.push_str(&fixed);
                continue;
            }
            if self.consume(Kind::Open).is_some() {
                let prefix = self.consume_text();
                let name = self.consume(Kind::Name);
                let regexp = self.consume_regexp_or_wildcard(name.is_some());
                let suffix = self.consume_text();
                self.consume(Kind::Close).ok_or(INVALID)?;
                let modifier = self.consume_modifier();
                self.add_part(prefix, name, regexp, suffix, modifier)?;
                continue;
            }
            self.flush_pending();
            self.consume(Kind::End).ok_or(INVALID)?;
            return Ok(self.parts);
        }
    }
}

/// The pinned "escape a regexp string".
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if ".+*?^${}()[]|/\\".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The pinned "generate a regular expression and name list", as JavaScript source.
fn generate(parts: &[Part], options: Options) -> String {
    let mut result = String::from("^");
    for part in parts {
        match part {
            Part::Fixed { value, modifier } => {
                if *modifier == Modifier::None {
                    result.push_str(&escape(value));
                } else {
                    result.push_str(&format!("(?:{}){}", escape(value), modifier.text()));
                }
            }
            Part::Group {
                regexp,
                prefix,
                suffix,
                modifier,
            } => {
                let regexp = regexp.clone().unwrap_or_else(|| options.segment_wildcard());
                let text = modifier.text();
                let (prefix, suffix) = (escape(prefix), escape(suffix));
                let single = matches!(modifier, Modifier::None | Modifier::Optional);
                if prefix.is_empty() && suffix.is_empty() {
                    if single {
                        result.push_str(&format!("({regexp}){text}"));
                    } else {
                        result.push_str(&format!("((?:{regexp}){text})"));
                    }
                } else if single {
                    result.push_str(&format!("(?:{prefix}({regexp}){suffix}){text}"));
                } else {
                    result.push_str(&format!(
                        "(?:{prefix}((?:{regexp})(?:{suffix}{prefix}(?:{regexp}))*){suffix})"
                    ));
                    if *modifier == Modifier::ZeroOrMore {
                        result.push('?');
                    }
                }
            }
        }
    }
    result.push('$');
    result
}

#[cfg(test)]
mod tests;
