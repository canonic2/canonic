//! Bounded recognition of jq expressions that consume only supplied data.
//! This checks effects; jq still diagnoses malformed expressions.

pub(super) fn read_only(source: &str) -> bool {
    !source.trim().is_empty()
        && source.len() <= 65_536
        && Reader {
            bytes: source.as_bytes(),
            position: 0,
            tokens: 0,
        }
        .expression(None, 0)
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    tokens: usize,
}

impl Reader<'_> {
    fn expression(&mut self, end: Option<u8>, depth: usize) -> bool {
        if depth > 32 {
            return false;
        }
        let mut field = false;
        // Whether the last token ended a value, where `..` cannot follow.
        let mut value = false;
        let mut object_key = end == Some(b'}');
        while let Some(&byte) = self.bytes.get(self.position) {
            self.position += 1;
            self.tokens += 1;
            if self.tokens > 8192 {
                return false;
            }
            if byte.is_ascii_whitespace() {
                continue;
            }
            if Some(byte) == end {
                return true;
            }
            match byte {
                b'"' => {
                    if !self.string(depth + 1) {
                        return false;
                    }
                }
                b'(' | b'[' | b'{' => {
                    let close = match byte {
                        b'(' => b')',
                        b'[' => b']',
                        _ => b'}',
                    };
                    if !self.expression(Some(close), depth + 1) {
                        return false;
                    }
                }
                b')' | b']' | b'}' => return false,
                b'.' => {
                    // `..` recurses through the input like `recurse`, and starts
                    // a value; a name after it is not a field, so it must be a
                    // listed builtin.
                    if self.bytes.get(self.position) == Some(&b'.') {
                        if value {
                            return false;
                        }
                        self.position += 1;
                        field = false;
                    } else {
                        field = true;
                    }
                    value = true;
                    continue;
                }
                b'@' => {
                    if ![
                        "text", "json", "html", "uri", "urid", "csv", "tsv", "sh", "base64",
                        "base64d", "base32", "base32d",
                    ]
                    .contains(&self.identifier())
                    {
                        return false;
                    }
                }
                // Variables hold supplied values, `as` bindings or --arg
                // values; only $ENV reads the environment.
                b'$' => {
                    if matches!(self.identifier(), "" | "ENV") {
                        return false;
                    }
                }
                byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                    self.position -= 1;
                    let start = self.position;
                    self.identifier();
                    // Object shorthand ({name}) reads .name, even when the key
                    // is also a helper name such as input or env.
                    let key = object_key
                        && matches!(
                            self.bytes[self.position..]
                                .iter()
                                .find(|b| !b.is_ascii_whitespace()),
                            Some(b':' | b',' | b'}')
                        );
                    let name =
                        std::str::from_utf8(&self.bytes[start..self.position]).unwrap_or_default();
                    if !field && !key && !builtin(name) {
                        return false;
                    }
                }
                byte if byte.is_ascii_digit() => {
                    while self
                        .bytes
                        .get(self.position)
                        .is_some_and(u8::is_ascii_digit)
                    {
                        self.position += 1;
                    }
                }
                b'|' | b',' | b';' | b':' | b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>'
                | b'=' | b'!' | b'?' => {}
                _ => return false,
            }
            // Operators leave room for a new value; anything else ends one.
            value = !b"|,;:+-*/%<>=!".contains(&byte);
            field = false;
            object_key = end == Some(b'}') && byte == b',';
        }
        end.is_none()
    }

    fn identifier(&mut self) -> &str {
        let start = self.position;
        while self
            .bytes
            .get(self.position)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.position += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.position]).unwrap_or_default()
    }

    fn string(&mut self, depth: usize) -> bool {
        while let Some(&byte) = self.bytes.get(self.position) {
            self.position += 1;
            match byte {
                b'"' => return true,
                b'\\' => match self.bytes.get(self.position).copied() {
                    Some(b'(') => {
                        self.position += 1;
                        if !self.expression(Some(b')'), depth + 1) {
                            return false;
                        }
                    }
                    Some(b'u') => {
                        self.position += 1;
                        let Some(hex) = self.bytes.get(self.position..self.position + 4) else {
                            return false;
                        };
                        if !hex.iter().all(u8::is_ascii_hexdigit) {
                            return false;
                        }
                        self.position += 4;
                    }
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                        self.position += 1;
                    }
                    _ => return false,
                },
                0..=31 => return false,
                _ => {}
            }
        }
        false
    }
}

/// Keywords and builtins that compute only from their input and arguments.
const BUILTINS: &str = "keys keys_unsorted length type sort sort_by unique unique_by values \
    tostring tonumber tojson fromjson select map map_values test match capture contains inside \
    has startswith endswith split join ltrimstr rtrimstr ascii_downcase ascii_upcase add min \
    max min_by max_by flatten reverse group_by any all not empty true false null and or if then \
    elif else end try catch as reduce foreach label break to_entries from_entries with_entries \
    paths leaf_paths getpath setpath del delpaths pick range limit first last nth until while \
    repeat recurse walk transpose combinations indices index rindex splits sub gsub scan ascii \
    implode explode trim ltrim rtrim tostream fromstream floor ceil round sqrt abs toarray \
    utf8bytelength isempty error numbers strings arrays objects iterables booleans nulls scalars \
    now todate fromdate todateiso8601 fromdateiso8601 strftime strptime mktime gmtime IN INDEX";

fn builtin(name: &str) -> bool {
    BUILTINS.split_whitespace().any(|builtin| builtin == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jq_summaries_selections_and_formats_only_consume_input_data() {
        for source in [
            r#".data | "desc: \(.description)\nprofiles: \(.profiles)\nrules: \(.rules|length)\nrule keys: \(.rules[0]|keys)""#,
            r#".data.rules[] | select(.id|test("npm")) | [.id, (.profile_decisions|tostring), .reason[0:90]] | @tsv"#,
            r"{name: .name, count: (.items | length), env: .env}",
            r"{env: .env, input: .input, slice: .items[0:3]}",
            r".items | map(select(.enabled == true)) | sort_by(.name)",
            r#""literal env and \\(env) are data""#,
        ] {
            assert!(read_only(source), "{source}");
        }
    }

    #[test]
    fn jq_object_shorthand_reads_fields_without_calling_helpers() {
        for source in [
            r"{id}",
            r"{id, decision, conditions}",
            r"{env, input, inputs}",
            r"{id, nested: {name, value: .value}, conditions}",
            r#".data.rules[] | select((tostring|test("npm|script";"i"))) | {id, decision, pack: (.pack // .source // null), conditions}"#,
        ] {
            assert!(read_only(source), "{source}");
        }
        for source in [
            r"{id, value: env}",
            r"{id, value: input}",
            r"{id, value: inputs}",
            r"{id | env}",
            r"{id, nested: {name, value: env}}",
            r#"{id, value: "\(input)"}"#,
        ] {
            assert!(!read_only(source), "{source}");
        }
    }
    #[test]
    fn jq_variables_bindings_and_pure_builtins_only_consume_input_data() {
        for source in [
            r"$name",
            r". as $run | {sha: $run.head_sha, event}",
            r"reduce .[] as $item (0; . + $item.count)",
            r".jobs | map({name, conclusion}) | to_entries | limit(3; .[])",
            r#".items[] | select(.name | test("^api")) | .name | sub("^api-"; "")"#,
            r"{head_sha,event,run_attempt,status,conclusion}",
            r".created_at | fromdateiso8601 | todate",
            // `..` recurses like `recurse`; both read only the input.
            r"[.. | numbers] | add",
            r"[.. | objects | .name]",
            r"[recurse | strings]",
        ] {
            assert!(read_only(source), "{source}");
        }
        for source in ["..env", ".. | env", "..|input"] {
            assert!(!read_only(source), "{source}");
        }
        for source in [
            "$ENV.HOME",
            "$ENV",
            "$",
            ". as $x | $ENV",
            "{home: $ENV.HOME}",
        ] {
            assert!(!read_only(source), "{source}");
        }
    }

    #[test]
    fn jq_hidden_effects_are_rejected_even_inside_interpolation() {
        for source in [
            "env",
            "$ENV",
            ". | input",
            ". | inputs",
            "include \"helper\"; .",
            "import \"helper\" as h; h::read",
            r#""value: \(env.KEY)""#,
            r#""nested \("value: \(input)")""#,
            r".items | map(env)",
            r#""unterminated"#,
            ".foo..",
            ".[env.KEY]",
            ".[env:]",
            ".[input:]",
            ".[0:input]",
            "{value: env: .name}",
        ] {
            assert!(!read_only(source), "{source}");
        }
        assert!(!read_only(&format!(
            "{} . {}",
            "[".repeat(40),
            "]".repeat(40)
        )));
    }
}
