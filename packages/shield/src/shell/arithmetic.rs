//! Bounded constant integer arithmetic; variable evaluation and side effects stay opaque.

pub(super) fn constant(source: &str) -> Option<String> {
    if source.len() > 4096 || source.contains("++") || source.contains("--") {
        return None;
    }
    let mut parser = Parser {
        source: source.as_bytes(),
        at: 0,
        depth: 0,
    };
    let value = parser.expression(0)?;
    parser.spaces();
    (parser.at == parser.source.len()).then(|| value.to_string())
}

struct Parser<'a> {
    source: &'a [u8],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn spaces(&mut self) {
        while self
            .source
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }

    fn expression(&mut self, minimum: u8) -> Option<i64> {
        self.depth += 1;
        if self.depth > 32 {
            return None;
        }
        let mut value = self.unary()?;
        loop {
            self.spaces();
            let Some(operator) = self.source.get(self.at).copied() else {
                break;
            };
            let precedence = match operator {
                b'+' | b'-' => 1,
                b'*' | b'/' | b'%' => 2,
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.at += 1;
            let right = self.expression(precedence + 1)?;
            value = match operator {
                b'+' => value.checked_add(right),
                b'-' => value.checked_sub(right),
                b'*' => value.checked_mul(right),
                b'/' => value.checked_div(right),
                b'%' => value.checked_rem(right),
                _ => None,
            }
            .filter(|value| i32::try_from(*value).is_ok())?;
        }
        self.depth -= 1;
        Some(value)
    }

    fn unary(&mut self) -> Option<i64> {
        self.spaces();
        match self.source.get(self.at)? {
            b'+' | b'-' => {
                let negative = self.source[self.at] == b'-';
                self.at += 1;
                self.depth += 1;
                if self.depth > 32 {
                    return None;
                }
                let value = self.unary()?;
                self.depth -= 1;
                if negative {
                    value.checked_neg()
                } else {
                    Some(value)
                }
            }
            b'(' => {
                self.at += 1;
                let value = self.expression(0)?;
                self.spaces();
                if self.source.get(self.at) != Some(&b')') {
                    return None;
                }
                self.at += 1;
                Some(value)
            }
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<i64> {
        let start = self.at;
        let radix = if self.source.get(self.at) == Some(&b'0') {
            if matches!(self.source.get(self.at + 1), Some(b'x' | b'X')) {
                self.at += 2;
                16
            } else {
                8
            }
        } else {
            10
        };
        let digits = self.at;
        let mut value = 0i64;
        while let Some(byte) = self.source.get(self.at) {
            let Some(digit) = char::from(*byte).to_digit(radix) else {
                break;
            };
            value = value
                .checked_mul(i64::from(radix))?
                .checked_add(i64::from(digit))?;
            if i32::try_from(value).is_err() {
                return None;
            }
            self.at += 1;
        }
        (self.at > digits && self.at > start).then_some(value)
    }
}
