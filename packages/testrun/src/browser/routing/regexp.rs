//! Translates the regular part of ASCII ECMAScript regular expressions into the
//! syntax of Rust's linear matchers, refusing what they cannot run.
//!
//! The source follows `RegExp` with Annex B in non-`u` mode and the stricter
//! grammar with `u`. Non-regular features (lookaround, backreferences) and the
//! `v` flag are refused with `ROUTE_REGEX_UNSUPPORTED`; syntax JavaScript itself
//! rejects is refused with `ROUTE_REGEX_INVALID`. The pinned `urlMatches` resets
//! `lastIndex` before every test, so `g` has no effect and `y` anchors the match
//! at the start of the URL.

const INVALID: &str = "ROUTE_REGEX_INVALID";
const UNSUPPORTED: &str = "ROUTE_REGEX_UNSUPPORTED";

/// Validate `pattern`/`flags` and return the equivalent Rust regex source.
pub(crate) fn translate(pattern: &str, flags: &str) -> Result<String, &'static str> {
    translate_within(pattern, flags, 2048)
}
/// [`translate`] with a caller-chosen source bound, for generated patterns.
pub(crate) fn translate_within(
    pattern: &str,
    flags: &str,
    limit: usize,
) -> Result<String, &'static str> {
    if pattern.len() > limit
        || !pattern.is_ascii()
        || pattern.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(INVALID);
    }
    let (unicode, sticky) = flag_set(flags)?;
    let source = Translator {
        bytes: pattern.as_bytes(),
        at: 0,
        unicode,
        out: String::with_capacity(pattern.len() + 16),
        depth: 0,
        names: Vec::new(),
        captures: Captures::inspect(pattern),
    }
    .run()?;
    // `\A` stays the haystack start under `m`, unlike `^`.
    Ok(if sticky {
        format!(r"\A(?:{source})")
    } else {
        source
    })
}

/// Validate a flag string and report whether `u` and `y` are set.
fn flag_set(flags: &str) -> Result<(bool, bool), &'static str> {
    let mut seen = Vec::with_capacity(flags.len());
    for flag in flags.chars() {
        if seen.contains(&flag) {
            return Err(INVALID);
        }
        match flag {
            'd' | 'g' | 'i' | 'm' | 's' | 'u' | 'y' => seen.push(flag),
            'v' => return Err(UNSUPPORTED),
            _ => return Err(INVALID),
        }
    }
    Ok((seen.contains(&'u'), seen.contains(&'y')))
}

/// One class member: a code point or a class escape such as `\d`.
enum Member {
    Char(u8),
    Set(&'static str),
}

/// Capture authority is global, including forward references. This scan only
/// collects layout; the translator still validates every supported grammar term.
struct Captures {
    count: usize,
    names: Vec<String>,
}
impl Captures {
    fn inspect(pattern: &str) -> Self {
        let bytes = pattern.as_bytes();
        let mut result = Self {
            count: 0,
            names: Vec::new(),
        };
        let mut at = 0;
        let mut class = false;
        while let Some(&byte) = bytes.get(at) {
            match byte {
                b'\\' => at += 1,
                b'[' if !class => class = true,
                b']' if class => class = false,
                b'(' if !class => {
                    let named = bytes.get(at + 1) == Some(&b'?')
                        && bytes.get(at + 2) == Some(&b'<')
                        && !matches!(bytes.get(at + 3).copied(), Some(b'=' | b'!'));
                    if bytes.get(at + 1) != Some(&b'?') || named {
                        result.count += 1;
                    }
                    if named {
                        let tail = &pattern[at + 3..];
                        let end = tail
                            .bytes()
                            .position(|byte| {
                                !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'$')
                            })
                            .unwrap_or(tail.len());
                        let name = &tail[..end];
                        if tail.as_bytes().get(end) == Some(&b'>') && identifier(name) {
                            result.names.push(name.to_owned());
                        }
                    }
                }
                _ => {}
            }
            at += 1;
        }
        result
    }
}

struct Translator<'a> {
    bytes: &'a [u8],
    at: usize,
    unicode: bool,
    out: String,
    depth: usize,
    names: Vec<String>,
    captures: Captures,
}

impl Translator<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }
    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.at += 1;
        }
        found
    }
    fn literal(&mut self, byte: u8) {
        self.out.push_str(&format!("\\x{byte:02X}"));
    }

    fn run(mut self) -> Result<String, &'static str> {
        // Whether the previous term can take a quantifier.
        let mut quantifiable = false;
        while let Some(byte) = self.next() {
            match byte {
                b'(' => {
                    self.group()?;
                    quantifiable = false;
                }
                b')' => {
                    self.depth = self.depth.checked_sub(1).ok_or(INVALID)?;
                    self.out.push(')');
                    quantifiable = true;
                }
                b'|' => {
                    self.out.push('|');
                    quantifiable = false;
                }
                b'^' | b'$' => {
                    self.out.push(byte as char);
                    quantifiable = false;
                }
                b'.' => {
                    self.out.push('.');
                    quantifiable = true;
                }
                b'*' | b'+' | b'?' => {
                    if !quantifiable {
                        return Err(INVALID);
                    }
                    self.out.push(byte as char);
                    self.eat(b'?').then(|| self.out.push('?'));
                    quantifiable = false;
                }
                b'{' => match self.braces() {
                    Some((low, high)) => {
                        if !quantifiable || high.is_some_and(|high| high < low) {
                            return Err(INVALID);
                        }
                        let high = high.map_or_else(
                            || ",".into(),
                            |high| {
                                if high == low {
                                    String::new()
                                } else {
                                    format!(",{high}")
                                }
                            },
                        );
                        self.out.push_str(&format!("{{{low}{high}}}"));
                        self.eat(b'?').then(|| self.out.push('?'));
                        quantifiable = false;
                    }
                    None if self.unicode => return Err(INVALID),
                    None => {
                        self.literal(b'{');
                        quantifiable = true;
                    }
                },
                b'}' | b']' if self.unicode => return Err(INVALID),
                b'}' | b']' => {
                    self.literal(byte);
                    quantifiable = true;
                }
                b'[' => {
                    self.class()?;
                    quantifiable = true;
                }
                b'\\' => quantifiable = self.escape()?,
                _ => {
                    self.literal(byte);
                    quantifiable = true;
                }
            }
        }
        if self.depth != 0 {
            return Err(INVALID);
        }
        Ok(self.out)
    }

    /// After `(`: captures, named captures, noncapturing groups and modifiers.
    fn group(&mut self) -> Result<(), &'static str> {
        self.depth += 1;
        if !self.eat(b'?') {
            self.out.push('(');
            return Ok(());
        }
        match self.next() {
            Some(b':') => self.out.push_str("(?:"),
            Some(b'=' | b'!') => return Err(UNSUPPORTED),
            Some(b'<') if matches!(self.peek(), Some(b'=' | b'!')) => return Err(UNSUPPORTED),
            Some(b'<') => {
                let start = self.at;
                while self.peek().is_some_and(|byte| byte != b'>') {
                    self.at += 1;
                }
                let name = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| INVALID)?;
                if !self.eat(b'>') || !identifier(name) {
                    return Err(INVALID);
                }
                // Duplicate names are valid only in separate alternatives.
                if self.names.iter().any(|seen| seen == name) {
                    return Err(UNSUPPORTED);
                }
                self.names.push(name.to_owned());
                // Names do not affect matching; keep a plain capture.
                self.out.push('(');
            }
            Some(byte) => {
                self.at -= 1;
                self.modifiers(byte)?;
            }
            None => return Err(INVALID),
        }
        Ok(())
    }

    /// `(?ims-ims:` pattern modifiers, which Rust's syntax spells the same way.
    fn modifiers(&mut self, first: u8) -> Result<(), &'static str> {
        if !matches!(first, b'i' | b'm' | b's' | b'-') {
            return Err(INVALID);
        }
        let mut seen = Vec::new();
        let mut removing = false;
        let mut text = String::from("(?");
        loop {
            match self.next().ok_or(INVALID)? {
                b':' => break,
                b'-' if !removing => {
                    removing = true;
                    text.push('-');
                }
                flag @ (b'i' | b'm' | b's') if !seen.contains(&flag) => {
                    seen.push(flag);
                    text.push(flag as char);
                }
                _ => return Err(INVALID),
            }
        }
        if seen.is_empty() {
            return Err(INVALID);
        }
        if text.ends_with('-') {
            text.pop();
        }
        text.push(':');
        self.out.push_str(&text);
        Ok(())
    }

    /// After `{`: a `{n}`, `{n,}` or `{n,m}` quantifier, or `None` (and no input
    /// consumed) when the braces are not one.
    fn braces(&mut self) -> Option<(u32, Option<u32>)> {
        let start = self.at;
        let parsed = (|| {
            let low = self.number()?;
            if self.eat(b'}') {
                return Some((low, Some(low)));
            }
            if !self.eat(b',') {
                return None;
            }
            if self.eat(b'}') {
                return Some((low, None));
            }
            let high = self.number()?;
            self.eat(b'}').then_some((low, Some(high)))
        })();
        if parsed.is_none() {
            self.at = start;
        }
        parsed
    }
    fn number(&mut self) -> Option<u32> {
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        let digits = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
        // Oversized counts stay quantifiers; the matcher's size limit refuses them.
        (!digits.is_empty()).then(|| digits.parse().unwrap_or(u32::MAX))
    }

    /// After `\` outside a class; returns whether the escape is quantifiable.
    fn escape(&mut self) -> Result<bool, &'static str> {
        let byte = self.next().ok_or(INVALID)?;
        match byte {
            b'b' | b'B' => {
                self.out.push('\\');
                self.out.push(byte as char);
                Ok(false)
            }
            _ => {
                match self.atom_escape(byte, false)? {
                    Member::Char(byte) => self.literal(byte),
                    Member::Set(set) => self.out.push_str(set),
                }
                Ok(true)
            }
        }
    }

    /// An escape that denotes one character or class escape, inside or outside
    /// a class.
    fn atom_escape(&mut self, byte: u8, in_class: bool) -> Result<Member, &'static str> {
        Ok(match byte {
            b'd' => Member::Set(r"\d"),
            b'D' => Member::Set(r"\D"),
            b's' => Member::Set(r"\s"),
            b'S' => Member::Set(r"\S"),
            b'w' => Member::Set(r"\w"),
            b'W' => Member::Set(r"\W"),
            b'f' => Member::Char(0x0C),
            b'n' => Member::Char(b'\n'),
            b'r' => Member::Char(b'\r'),
            b't' => Member::Char(b'\t'),
            b'v' => Member::Char(0x0B),
            b'b' if in_class => Member::Char(0x08),
            b'0' if self.unicode && self.peek().is_some_and(|byte| byte.is_ascii_digit()) => {
                return Err(INVALID);
            }
            b'0' => self.octal(byte)?,
            b'1'..=b'7' if in_class && !self.unicode => self.octal(byte)?,
            b'8' | b'9' if in_class && !self.unicode => Member::Char(byte),
            b'1'..=b'9' if in_class => return Err(INVALID),
            b'1'..=b'9' => self.decimal_escape(byte)?,
            b'k' => self.named_escape(in_class)?,
            b'c' => match self.peek() {
                Some(letter)
                    if letter.is_ascii_alphabetic()
                        || in_class
                            && !self.unicode
                            && (letter.is_ascii_digit() || letter == b'_') =>
                {
                    self.at += 1;
                    Member::Char(letter % 32)
                }
                _ if self.unicode => return Err(INVALID),
                // Annex B: an incomplete control escape is a literal backslash.
                _ => {
                    self.at -= 1;
                    Member::Char(b'\\')
                }
            },
            b'x' => match self.hex(2) {
                Some(value) => ascii(value)?,
                None if self.unicode => return Err(INVALID),
                None => Member::Char(b'x'),
            },
            b'u' => {
                let value = if self.unicode && self.eat(b'{') {
                    let start = self.at;
                    while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                        self.at += 1;
                    }
                    let digits =
                        std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| INVALID)?;
                    let value = u32::from_str_radix(digits, 16).map_err(|_| INVALID)?;
                    if !self.eat(b'}') || value > 0x10_FFFF {
                        return Err(INVALID);
                    }
                    Some(value)
                } else {
                    self.hex(4)
                };
                match value {
                    Some(value) => ascii(value)?,
                    None if self.unicode => return Err(INVALID),
                    None => Member::Char(b'u'),
                }
            }
            b'p' | b'P' if self.unicode => return Err(UNSUPPORTED),
            b'-' if self.unicode && !in_class => return Err(INVALID),
            b'^' | b'$' | b'\\' | b'.' | b'*' | b'+' | b'?' | b'(' | b')' | b'[' | b']' | b'{'
            | b'}' | b'|' | b'/' | b'-' => Member::Char(byte),
            _ if self.unicode => return Err(INVALID),
            // Annex B identity escapes, including unrecognised letters.
            _ => Member::Char(byte),
        })
    }
    fn hex(&mut self, digits: usize) -> Option<u32> {
        let text = self.bytes.get(self.at..self.at + digits)?;
        if !text.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        self.at += digits;
        u32::from_str_radix(std::str::from_utf8(text).ok()?, 16).ok()
    }

    fn decimal_escape(&mut self, first: u8) -> Result<Member, &'static str> {
        let mut value = usize::from(first - b'0');
        for digit in self.bytes[self.at..]
            .iter()
            .take_while(|digit| digit.is_ascii_digit())
        {
            value = value
                .saturating_mul(10)
                .saturating_add(usize::from(digit - b'0'));
        }
        if value <= self.captures.count {
            return Err(UNSUPPORTED);
        }
        if self.unicode {
            return Err(INVALID);
        }
        // Do not consume the complete decimal span for an Annex B fallback:
        // octal has its own width, and 8/9 remain identity characters.
        if first <= b'7' {
            self.octal(first)
        } else {
            Ok(Member::Char(first))
        }
    }

    fn named_escape(&mut self, in_class: bool) -> Result<Member, &'static str> {
        if !self.unicode && self.captures.names.is_empty() {
            return Ok(Member::Char(b'k'));
        }
        if in_class || !self.eat(b'<') {
            return Err(INVALID);
        }
        let start = self.at;
        while self.peek().is_some_and(|byte| byte != b'>') {
            self.at += 1;
        }
        let name = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| INVALID)?;
        if !self.eat(b'>')
            || !identifier(name)
            || !self.captures.names.iter().any(|known| known == name)
        {
            return Err(INVALID);
        }
        Err(UNSUPPORTED)
    }

    /// Annex B octal is unambiguous after zero, inside a class, or after proving
    /// that a complete decimal escape is not a backreference.
    /// A leading 4–7 admits only one further digit; other digits stay literal.
    fn octal(&mut self, first: u8) -> Result<Member, &'static str> {
        let mut value = u32::from(first - b'0');
        for _ in 0..if first <= b'3' { 2 } else { 1 } {
            let Some(digit @ b'0'..=b'7') = self.peek() else {
                break;
            };
            self.at += 1;
            value = value * 8 + u32::from(digit - b'0');
        }
        ascii(value)
    }

    /// After `[`: one class, with every member emitted as an escaped byte.
    fn class(&mut self) -> Result<(), &'static str> {
        let negated = self.eat(b'^');
        let mut members = Vec::new();
        loop {
            let byte = self.next().ok_or(INVALID)?;
            if byte == b']' {
                break;
            }
            let member = self.class_member(byte)?;
            if self.peek() == Some(b'-') && self.bytes.get(self.at + 1) != Some(&b']') {
                self.at += 1;
                let end = self.next().ok_or(INVALID)?;
                let end = self.class_member(end)?;
                match (member, end) {
                    (Member::Char(low), Member::Char(high)) => {
                        if high < low {
                            return Err(INVALID);
                        }
                        members.push(format!("\\x{low:02X}-\\x{high:02X}"));
                    }
                    _ if self.unicode => return Err(INVALID),
                    // Annex B: a range with a class escape is a literal hyphen.
                    (start, end) => {
                        members.push(member_text(&start));
                        members.push("\\x2D".into());
                        members.push(member_text(&end));
                    }
                }
            } else {
                members.push(member_text(&member));
            }
        }
        match (negated, members.is_empty()) {
            // `[^]` matches any character.
            (true, true) => self.out.push_str(r"[\x00-\xFF]"),
            // The intersection is empty in Rust's class-set dialect.
            (false, true) => self.out.push_str(r"[\x00&&\x01]"),
            (negated, false) => {
                self.out.push('[');
                if negated {
                    self.out.push('^');
                }
                self.out.push_str(&members.concat());
                self.out.push(']');
            }
        }
        Ok(())
    }
    fn class_member(&mut self, byte: u8) -> Result<Member, &'static str> {
        if byte != b'\\' {
            return Ok(Member::Char(byte));
        }
        let escaped = self.next().ok_or(INVALID)?;
        match escaped {
            b'B' if self.unicode => Err(INVALID),
            b'B' => Ok(Member::Char(b'B')),
            _ => self.atom_escape(escaped, true),
        }
    }
}

fn member_text(member: &Member) -> String {
    match member {
        Member::Char(byte) => format!("\\x{byte:02X}"),
        Member::Set(set) => (*set).to_owned(),
    }
}

/// An escaped code point; matching is ASCII-only, so others are refused.
fn ascii(value: u32) -> Result<Member, &'static str> {
    u8::try_from(value)
        .ok()
        .filter(u8::is_ascii)
        .map(Member::Char)
        .ok_or(UNSUPPORTED)
}

/// An ASCII JavaScript identifier, as a group name.
fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '_' | '$'))
        && chars.all(|next| next.is_ascii_alphanumeric() || matches!(next, '_' | '$'))
}

#[cfg(test)]
mod tests;
