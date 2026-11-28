//! ECMAScript JSON values preserve UTF-16 strings and IEEE-754 numbers without a JS runtime.
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Syntax,
    Bounds,
    NonJson,
}
#[derive(Debug)]
enum Node {
    Null,
    Boolean(bool),
    Number(f64),
    String(Vec<u16>),
    Array(Vec<usize>),
    Object(Vec<(Vec<u16>, usize)>),
}
pub(super) struct Graph {
    nodes: Vec<Node>,
    root: usize,
}
#[derive(Clone, Copy)]
enum State {
    ArrayFirst,
    ArrayValue,
    ArrayComma,
    ObjectFirst,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectComma,
}
struct Frame {
    id: usize,
    state: State,
    key: Option<Vec<u16>>,
    keys: BTreeMap<Vec<u16>, usize>,
}
struct Parser<'a> {
    input: &'a str,
    position: usize,
    nodes: Vec<Node>,
    frames: Vec<Frame>,
}
pub(super) fn parse(bytes: &[u8]) -> Result<Graph, Error> {
    if bytes.len() > super::MAX_BODY {
        return Err(Error::Bounds);
    }
    // Buffer.toString("utf8") replaces invalid sequences before JSON.parse.
    let decoded = String::from_utf8_lossy(bytes);
    let mut parser = Parser {
        input: &decoded,
        position: 0,
        nodes: vec![],
        frames: vec![],
    };
    let root = parser.atom()?;
    while let Some(frame) = parser.frames.last() {
        let parent = parser.frames.len() - 1;
        let id = frame.id;
        let state = frame.state;
        parser.whitespace();
        match state {
            State::ArrayFirst if parser.take(b']') => parser.close(),
            State::ArrayFirst | State::ArrayValue => {
                parser.frames[parent].state = State::ArrayComma;
                let child = parser.atom()?;
                let Node::Array(items) = &mut parser.nodes[id] else {
                    unreachable!()
                };
                items.push(child);
            }
            State::ArrayComma => {
                if parser.take(b']') {
                    parser.close();
                } else if parser.take(b',') {
                    parser.frames[parent].state = State::ArrayValue;
                } else {
                    return Err(Error::Syntax);
                }
            }
            State::ObjectFirst if parser.take(b'}') => parser.close(),
            State::ObjectFirst | State::ObjectKey => {
                let key = parser.string()?;
                parser.frames[parent].key = Some(key);
                parser.frames[parent].state = State::ObjectColon;
            }
            State::ObjectColon => {
                if !parser.take(b':') {
                    return Err(Error::Syntax);
                }
                parser.frames[parent].state = State::ObjectValue;
            }
            State::ObjectValue => {
                let key = parser.frames[parent].key.take().expect("Object key");
                parser.frames[parent].state = State::ObjectComma;
                let child = parser.atom()?;
                let frame = &mut parser.frames[parent];
                let Node::Object(entries) = &mut parser.nodes[id] else {
                    unreachable!()
                };
                if let Some(slot) = frame.keys.get(&key) {
                    entries[*slot].1 = child;
                } else {
                    frame.keys.insert(key.clone(), entries.len());
                    entries.push((key, child));
                }
            }
            State::ObjectComma => {
                if parser.take(b'}') {
                    parser.close();
                } else if parser.take(b',') {
                    parser.frames[parent].state = State::ObjectKey;
                } else {
                    return Err(Error::Syntax);
                }
            }
        }
    }
    parser.whitespace();
    if parser.position != parser.input.len() {
        return Err(Error::Syntax);
    }
    Ok(Graph {
        nodes: parser.nodes,
        root,
    })
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .input
            .as_bytes()
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.position += 1;
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        if self.input.as_bytes().get(self.position) == Some(&byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn literal(&mut self, literal: &str) -> Result<(), Error> {
        if !self.input[self.position..].starts_with(literal) {
            return Err(Error::Syntax);
        }
        self.position += literal.len();
        Ok(())
    }
    fn atom(&mut self) -> Result<usize, Error> {
        self.whitespace();
        let mut state = None;
        let node = match self
            .input
            .as_bytes()
            .get(self.position)
            .copied()
            .ok_or(Error::Syntax)?
        {
            b'n' => {
                self.literal("null")?;
                Node::Null
            }
            b't' => {
                self.literal("true")?;
                Node::Boolean(true)
            }
            b'f' => {
                self.literal("false")?;
                Node::Boolean(false)
            }
            b'"' => Node::String(self.string()?),
            b'[' => {
                self.position += 1;
                state = Some(State::ArrayFirst);
                Node::Array(vec![])
            }
            b'{' => {
                self.position += 1;
                state = Some(State::ObjectFirst);
                Node::Object(vec![])
            }
            b'-' | b'0'..=b'9' => Node::Number(self.number()?),
            _ => return Err(Error::Syntax),
        };
        let id = self.nodes.len();
        self.nodes.push(node);
        if let Some(state) = state {
            self.frames.push(Frame {
                id,
                state,
                key: None,
                keys: BTreeMap::new(),
            });
        }
        Ok(id)
    }
    fn string(&mut self) -> Result<Vec<u16>, Error> {
        if !self.take(b'"') {
            return Err(Error::Syntax);
        }
        let mut units = vec![];
        loop {
            let byte = *self
                .input
                .as_bytes()
                .get(self.position)
                .ok_or(Error::Syntax)?;
            if byte == b'"' {
                self.position += 1;
                return Ok(units);
            }
            if byte < 32 {
                return Err(Error::Syntax);
            }
            if byte == b'\\' {
                self.position += 1;
                let escape = *self
                    .input
                    .as_bytes()
                    .get(self.position)
                    .ok_or(Error::Syntax)?;
                self.position += 1;
                let unit = match escape {
                    b'"' => 34,
                    b'\\' => 92,
                    b'/' => 47,
                    b'b' => 8,
                    b'f' => 12,
                    b'n' => 10,
                    b'r' => 13,
                    b't' => 9,
                    b'u' => {
                        let mut unit = 0u16;
                        for _ in 0..4 {
                            let byte = *self
                                .input
                                .as_bytes()
                                .get(self.position)
                                .ok_or(Error::Syntax)?;
                            let value = match byte {
                                b'0'..=b'9' => byte - b'0',
                                b'a'..=b'f' => byte - b'a' + 10,
                                b'A'..=b'F' => byte - b'A' + 10,
                                _ => return Err(Error::Syntax),
                            };
                            self.position += 1;
                            unit = unit * 16 + u16::from(value);
                        }
                        unit
                    }
                    _ => return Err(Error::Syntax),
                };
                units.push(unit);
            } else {
                let character = self.input[self.position..]
                    .chars()
                    .next()
                    .ok_or(Error::Syntax)?;
                let mut encoded = [0; 2];
                units.extend_from_slice(character.encode_utf16(&mut encoded));
                self.position += character.len_utf8();
            }
        }
    }
    fn number(&mut self) -> Result<f64, Error> {
        let start = self.position;
        self.take(b'-');
        if !self.take(b'0') {
            if !self
                .input
                .as_bytes()
                .get(self.position)
                .is_some_and(|byte| matches!(byte, b'1'..=b'9'))
            {
                return Err(Error::Syntax);
            }
            self.digits();
        }
        if self.take(b'.') {
            let before = self.position;
            self.digits();
            if self.position == before {
                return Err(Error::Syntax);
            }
        }
        if self.take(b'e') || self.take(b'E') {
            if !self.take(b'+') {
                self.take(b'-');
            }
            let before = self.position;
            self.digits();
            if self.position == before {
                return Err(Error::Syntax);
            }
        }
        self.input[start..self.position]
            .parse()
            .map_err(|_| Error::Syntax)
    }
    fn digits(&mut self) {
        while self
            .input
            .as_bytes()
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
    }
    fn close(&mut self) {
        let frame = self.frames.pop().expect("JSON container");
        if let Node::Object(entries) = &mut self.nodes[frame.id] {
            entries.sort_by(|(a, _), (b, _)| match (array_index(a), array_index(b)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });
        }
    }
}
pub(super) fn array_index(units: &[u16]) -> Option<u32> {
    if units.is_empty() || units.len() > 10 || (units.len() > 1 && units[0] == 48) {
        return None;
    }
    let mut value = 0u32;
    for &unit in units {
        if !(48..=57).contains(&unit) {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u32::from(unit - 48))?;
    }
    (value < u32::MAX).then_some(value)
}
impl Graph {
    /// Compact reachable values in public property order. Replaced duplicate
    /// properties never expose an obsolete value through orphan graph nodes.
    pub(super) fn project(&self, max_bytes: usize) -> Result<Value, Error> {
        let mut order = vec![];
        let mut pending = vec![self.root];
        let mut remaining = max_bytes;
        while let Some(id) = pending.pop() {
            // Every encoded graph node takes at least fifteen bytes. UTF-16
            // units take at least two each (a digit and delimiter). Refuse
            // before allocating the expanded JSON object representation. The
            // exact complete-result bound remains authoritative afterward.
            let units = match &self.nodes[id] {
                Node::String(units) => units.len(),
                Node::Object(entries) => entries.iter().map(|(key, _)| key.len()).sum(),
                _ => 0,
            };
            remaining = remaining
                .checked_sub(15usize.saturating_add(units.saturating_mul(2)))
                .ok_or(Error::Bounds)?;
            order.push(id);
            match &self.nodes[id] {
                Node::Array(items) => {
                    if items.len().saturating_mul(15) > remaining {
                        return Err(Error::Bounds);
                    }
                    pending.extend(items.iter().rev().copied());
                }
                Node::Object(entries) => {
                    if entries.len().saturating_mul(15) > remaining {
                        return Err(Error::Bounds);
                    }
                    pending.extend(entries.iter().rev().map(|(_, id)| *id));
                }
                _ => {}
            }
        }
        let mut remap = vec![usize::MAX; self.nodes.len()];
        for (id, old) in order.iter().enumerate() {
            remap[*old] = id;
        }
        let nodes=order.iter().map(|&id|match &self.nodes[id] {
            Node::Null=>json!({"kind":"null"}),
            Node::Boolean(value)=>json!({"kind":"boolean","value":value}),
            Node::Number(value)=>{
                let value=if value.is_infinite() {json!(if value.is_sign_negative() {"negative_infinity"}else{"positive_infinity"})}
                    else if *value==0.&&value.is_sign_negative() {json!("negative_zero")}else{plain_number(*value).expect("Finite JSON number")};
                json!({"kind":"number","value":value})
            },
            Node::String(units)=>json!({"kind":"string","units":units}),
            Node::Array(items)=>json!({"kind":"array","items":items.iter().map(|id|remap[*id]).collect::<Vec<_>>()}),
            Node::Object(entries)=>json!({"kind":"object","entries":entries.iter().map(|(key,id)|json!({"key":key,"value":remap[*id]})).collect::<Vec<_>>()}),
        }).collect::<Vec<_>>();
        Ok(json!({"root":0,"nodes":nodes}))
    }
    pub(super) fn plain(&self) -> Result<Value, Error> {
        self.plain_at(self.root, 0)
    }
    fn plain_at(&self, id: usize, depth: usize) -> Result<Value, Error> {
        if depth > 64 {
            return Err(Error::Bounds);
        }
        Ok(match &self.nodes[id] {
            Node::Null => Value::Null,
            Node::Boolean(value) => json!(value),
            Node::Number(value) => plain_number(*value)?,
            Node::String(units) => json!(String::from_utf16(units).map_err(|_| Error::NonJson)?),
            Node::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|id| self.plain_at(*id, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            Node::Object(entries) => {
                let mut object = serde_json::Map::new();
                for (key, id) in entries {
                    object.insert(
                        String::from_utf16(key).map_err(|_| Error::NonJson)?,
                        self.plain_at(*id, depth + 1)?,
                    );
                }
                Value::Object(object)
            }
        })
    }
}
fn plain_number(value: f64) -> Result<Value, Error> {
    if value.is_finite() && value.fract() == 0. && !(value == 0. && value.is_sign_negative()) {
        if (0. ..18446744073709551616.).contains(&value) {
            return Ok(json!(value as u64));
        }
        if value < 0. && value >= i64::MIN as f64 {
            return Ok(json!(value as i64));
        }
    }
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or(Error::NonJson)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_json_ecmascript_number_rounding_and_nonfinite_values() {
        let graph = parse(b"[9007199254740993,1e400,-1e400,-1e-400,-0,1.5]").unwrap();
        assert_eq!(graph.plain(), Err(Error::NonJson));
        let value = graph.project(65536).unwrap();
        let nodes = value["nodes"].as_array().unwrap();
        assert_eq!(nodes[1]["value"], json!(9007199254740992u64));
        assert_eq!(nodes[2]["value"], "positive_infinity");
        assert_eq!(nodes[3]["value"], "negative_infinity");
        assert_eq!(nodes[4]["value"], "negative_zero");
        assert_eq!(nodes[5]["value"], "negative_zero");
        assert_eq!(nodes[6]["value"], 1.5);
    }
    #[test]
    fn http_json_utf16_and_node_utf8_replacement_are_preserved() {
        let escaped = format!(
            "[\"{}uD800\",\"{}uD83D{}uDCA9\"]",
            char::from(92),
            char::from(92),
            char::from(92)
        );
        let graph = parse(escaped.as_bytes()).unwrap();
        assert_eq!(graph.plain(), Err(Error::NonJson));
        assert_eq!(
            graph.project(65536).unwrap()["nodes"][1]["units"],
            json!([0xd800])
        );
        assert_eq!(
            graph.project(65536).unwrap()["nodes"][2]["units"],
            json!([0xd83d, 0xdca9])
        );
        let invalid = [34, 0xe0, 0x80, 0x80, 0xed, 0xa0, 0x80, 0xf0, 0x9f, 34];
        assert_eq!(
            parse(&invalid).unwrap().project(65536).unwrap()["nodes"][0]["units"],
            json!([65533, 65533, 65533, 65533, 65533, 65533, 65533])
        );
    }
    #[test]
    fn http_json_duplicate_properties_remove_obsolete_values_and_keep_property_order() {
        let graph =
            parse(br#"{"b":"private-old","2":2,"1":1,"b":"current","00":0,"__proto__":true}"#)
                .unwrap();
        let value = graph.project(65536).unwrap();
        let entries = value["nodes"][0]["entries"].as_array().unwrap();
        let keys = entries
            .iter()
            .map(|entry| entry["key"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            vec![
                json!([49]),
                json!([50]),
                json!([98]),
                json!([48, 48]),
                json!([95, 95, 112, 114, 111, 116, 111, 95, 95])
            ]
        );
        assert_eq!(value["nodes"].as_array().unwrap().len(), 6);
        assert!(!value.to_string().contains("private-old"));
        assert_eq!(graph.plain().unwrap()["b"], "current");
    }
    #[test]
    fn http_json_strict_grammar_and_deep_iterative_value() {
        for bytes in [
            b"01".as_slice(),
            b"1.",
            b"1e+",
            b"[1,]",
            b"{\"a\":}",
            b"{\"a\":1,}",
            b"{} true",
            b"Infinity",
            b"NaN",
            b"\"\n\"",
            b"\xef\xbb\xbf{}",
        ] {
            assert!(matches!(parse(bytes), Err(Error::Syntax)), "{bytes:?}");
        }
        let deep = format!("{}0{}", "[".repeat(10000), "]".repeat(10000));
        let graph = parse(deep.as_bytes()).unwrap();
        assert_eq!(graph.plain(), Err(Error::Bounds));
        assert_eq!(graph.project(65536), Err(Error::Bounds));
        assert_eq!(
            graph.project(usize::MAX).unwrap()["nodes"]
                .as_array()
                .unwrap()
                .len(),
            10001
        );
        assert!(matches!(
            parse(&vec![32; super::super::MAX_BODY + 1]),
            Err(Error::Bounds)
        ));
    }
    #[test]
    fn http_json_projection_budget_counts_only_reachable_final_properties() {
        let repeated = format!("{{{}\"Acme\":true}}", "\"Acme\":null,".repeat(20000));
        let graph = parse(repeated.as_bytes()).unwrap();
        let value = graph.project(1000).unwrap();
        assert_eq!(value["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(value["nodes"][1], json!({"kind":"boolean","value":true}));
        let wide = format!("[{}null]", "null,".repeat(20000));
        assert_eq!(
            parse(wide.as_bytes()).unwrap().project(65536),
            Err(Error::Bounds)
        );
    }
}
