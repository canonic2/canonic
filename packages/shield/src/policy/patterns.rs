//! Bounded persisted Thompson programs, evaluated by regex-automata's Pike VM.
use super::{Error, selectors};
use regex_automata::{
    nfa::thompson::{self, NFA, State, Transition, pikevm::PikeVM},
    util::{look::Look, primitives::StateID, syntax},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

pub(super) const MAX_PATTERN_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_PREPARED_BYTES: usize = 256 * 1024 * 1024;

pub(crate) trait Provider {
    fn matches(&self, key: &str, value: &str) -> Result<bool, Error>;
    fn work(&self, key: &str, value: &str) -> Result<usize, Error>;
}

// Sharing lasts for this evaluation only. Source keys include selector dialect;
// each rule still retains its own owner, branch and diagnostics.
pub(super) struct Memo<'a> {
    provider: &'a dyn Provider,
    checkpoint: &'a dyn Fn() -> Result<(), Error>,
    results: RefCell<BTreeMap<(String, String), bool>>,
    work: Cell<usize>,
    bytes: Cell<usize>,
}
impl<'a> Memo<'a> {
    pub(super) fn new(
        provider: &'a dyn Provider,
        checkpoint: &'a dyn Fn() -> Result<(), Error>,
    ) -> Self {
        Self {
            provider,
            checkpoint,
            results: RefCell::default(),
            work: Cell::new(0),
            bytes: Cell::new(0),
        }
    }
}
impl Provider for Memo<'_> {
    fn matches(&self, key: &str, value: &str) -> Result<bool, Error> {
        (self.checkpoint)()?;
        let identity = (key.to_owned(), value.to_owned());
        if let Some(result) = self.results.borrow().get(&identity) {
            return Ok(*result);
        }
        let work = self
            .work
            .get()
            .checked_add(self.provider.work(key, value)?)
            .ok_or(Error::Limit)?;
        let bytes = self
            .bytes
            .get()
            .checked_add(key.len() + value.len())
            .ok_or(Error::Limit)?;
        if work > 32_000_000 || bytes > 32 * 1024 * 1024 || self.results.borrow().len() >= 4096 {
            return Err(Error::Limit);
        }
        self.work.set(work);
        self.bytes.set(bytes);
        let result = self.provider.matches(key, value)?;
        self.results.borrow_mut().insert(identity, result);
        Ok(result)
    }
    fn work(&self, key: &str, value: &str) -> Result<usize, Error> {
        self.provider.work(key, value)
    }
}

#[derive(Default)]
pub(super) struct Store {
    pub(super) images: BTreeMap<String, Vec<u8>>,
    programs: BTreeMap<String, PikeVM>,
    bytes: usize,
}

impl Store {
    pub(super) fn collect(&mut self, value: &Value, domain: &str) -> Result<(), Error> {
        match value {
            Value::Object(fields) => {
                if fields.len() == 1 {
                    for kind in ["regex", "glob"] {
                        if let Some(source) = fields.get(kind).and_then(Value::as_str) {
                            self.insert(kind, source)?;
                            return Ok(());
                        }
                    }
                }
                for (key, value) in fields {
                    if key == "environment" {
                        for selector in selectors::environment_selectors(value) {
                            self.collect(selector, domain)?;
                        }
                    } else if !(["exact", "input", "context"].contains(&key.as_str())
                        || domain == "mcp" && key == "arguments")
                    {
                        self.collect(value, domain)?;
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.collect(item, domain)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn insert(&mut self, kind: &str, source: &str) -> Result<(), Error> {
        let key = format!("{kind}:{source}");
        if self.programs.contains_key(&key) {
            return Ok(());
        }
        selectors::literal(source, false)?;
        let (expression, utf8) = if kind == "glob" {
            let glob = globset::GlobBuilder::new(source)
                .case_insensitive(false)
                .literal_separator(true)
                .backslash_escape(false)
                .empty_alternates(false)
                .allow_unclosed_class(false)
                .build()
                .map_err(|_| Error::Contract("invalid glob"))?;
            validate_glob(source)?;
            (format!("\\A(?:{})\\z", glob.regex()), false)
        } else {
            // Parse the source alone first: an unbalanced group such as
            // `x)|(?s:.*` would otherwise escape the full-field anchors.
            syntax::parse_with(source, &syntax::Config::new().utf8(true).nest_limit(128))
                .map_err(|_| Error::Contract("invalid regex"))?;
            (format!("\\A(?:{source})\\z"), true)
        };
        let nfa = NFA::compiler()
            .configure(
                thompson::Config::new()
                    .utf8(utf8)
                    .which_captures(thompson::WhichCaptures::None)
                    .nfa_size_limit(Some(MAX_PATTERN_BYTES)),
            )
            .syntax(syntax::Config::new().utf8(utf8).nest_limit(128))
            .build(&expression)
            .map_err(|_| Error::Contract("invalid or over-budget pattern"))?;
        let image = Image::capture(&nfa)?;
        let bytes = serde_json::to_vec(&image).map_err(|_| Error::Syntax)?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len().max(nfa.memory_usage()))
            .ok_or(Error::Limit)?;
        if bytes.len() > MAX_PATTERN_BYTES || self.bytes > MAX_PREPARED_BYTES {
            return Err(Error::Limit);
        }
        self.images.insert(key.clone(), bytes);
        self.programs
            .insert(key, PikeVM::new_from_nfa(nfa).map_err(|_| Error::Syntax)?);
        Ok(())
    }
}

impl Provider for Store {
    fn matches(&self, key: &str, value: &str) -> Result<bool, Error> {
        let vm = self
            .programs
            .get(key)
            .ok_or(Error::Contract("missing prepared pattern"))?;
        run(vm, value)
    }
    fn work(&self, key: &str, value: &str) -> Result<usize, Error> {
        work(
            self.programs
                .get(key)
                .ok_or(Error::Contract("missing prepared pattern"))?,
            value,
        )
    }
}

// Globset validates the grammar first. Enforce the stricter source contract while
// skipping complete classes, including a literal leading ']' after negation.
fn validate_glob(source: &str) -> Result<(), Error> {
    let bytes = source.as_bytes();
    let mut in_alternate = false;
    let mut empty = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'[' {
            empty = false;
            index += 1;
            if matches!(bytes.get(index), Some(b'!' | b'^')) {
                index += 1;
            }
            if bytes.get(index) == Some(&b']') {
                index += 1;
            }
            while bytes.get(index).is_some_and(|byte| *byte != b']') {
                index += 1;
            }
            index += 1;
            continue;
        }
        match byte {
            b'{' => {
                if in_alternate {
                    return Err(Error::Contract("nested glob alternation"));
                }
                in_alternate = true;
                empty = true;
            }
            b',' if in_alternate => {
                if empty {
                    return Err(Error::Contract("empty glob alternative"));
                }
                empty = true;
            }
            b'}' if in_alternate => {
                if empty {
                    return Err(Error::Contract("empty glob alternative"));
                }
                in_alternate = false;
            }
            b'*' if bytes.get(index + 1) == Some(&b'*') => {
                let before = index.checked_sub(1).and_then(|i| bytes.get(i));
                let after = bytes.get(index + 2);
                let leading = index == 0 || in_alternate && matches!(before, Some(b'{' | b','));
                let trailing =
                    after.is_none() || in_alternate && matches!(after, Some(b'}' | b','));
                let prefix = leading && (after == Some(&b'/') || index == 0 && trailing);
                let directory = before == Some(&b'/') && (after == Some(&b'/') || trailing);
                if !prefix && !directory {
                    return Err(Error::Contract("unsupported recursive glob stars"));
                }
                index += 1;
                empty = false;
            }
            _ => empty = false,
        }
        index += 1;
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Image {
    utf8: bool,
    start: usize,
    states: Vec<Instruction>,
}

#[derive(Deserialize, Serialize)]
enum Instruction {
    Ranges(Vec<(u8, u8, usize)>),
    Look(u32, usize),
    Union(Vec<usize>),
    Fail,
    Match,
}

impl Image {
    fn capture(nfa: &NFA) -> Result<Self, Error> {
        let states = nfa
            .states()
            .iter()
            .map(|state| {
                Ok(match state {
                    State::ByteRange { trans } => {
                        Instruction::Ranges(vec![(trans.start, trans.end, trans.next.as_usize())])
                    }
                    State::Sparse(trans) => Instruction::Ranges(
                        trans
                            .transitions
                            .iter()
                            .map(|t| (t.start, t.end, t.next.as_usize()))
                            .collect(),
                    ),
                    State::Dense(trans) => Instruction::Ranges(
                        trans
                            .transitions
                            .iter()
                            .enumerate()
                            .filter(|(_, id)| **id != StateID::ZERO)
                            .map(|(b, id)| {
                                Ok((
                                    u8::try_from(b).map_err(|_| Error::Limit)?,
                                    u8::try_from(b).map_err(|_| Error::Limit)?,
                                    id.as_usize(),
                                ))
                            })
                            .collect::<Result<_, Error>>()?,
                    ),
                    State::Look { look, next } => {
                        Instruction::Look(look.as_repr(), next.as_usize())
                    }
                    State::Union { alternates } => Instruction::Union(
                        alternates
                            .iter()
                            .map(regex_automata::util::primitives::StateID::as_usize)
                            .collect(),
                    ),
                    State::BinaryUnion { alt1, alt2 } => {
                        Instruction::Union(vec![alt1.as_usize(), alt2.as_usize()])
                    }
                    State::Fail => Instruction::Fail,
                    State::Match { .. } => Instruction::Match,
                    State::Capture { .. } => {
                        return Err(Error::Contract("unexpected capture in prepared pattern"));
                    }
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self {
            utf8: nfa.is_utf8(),
            start: nfa.start_anchored().as_usize(),
            states,
        })
    }

    fn restore(self) -> Result<PikeVM, Error> {
        if self.states.is_empty() || self.states.len() > MAX_PATTERN_BYTES / 8 {
            return Err(Error::Limit);
        }
        let state = |index: usize| {
            if index < self.states.len() {
                StateID::new(index).map_err(|_| Error::Limit)
            } else {
                Err(Error::Contract("invalid pattern transition"))
            }
        };
        let mut builder = thompson::Builder::new();
        builder.set_utf8(self.utf8);
        builder
            .set_size_limit(Some(MAX_PATTERN_BYTES))
            .map_err(|_| Error::Limit)?;
        builder.start_pattern().map_err(|_| Error::Syntax)?;
        for (position, instruction) in self.states.iter().enumerate() {
            let added = match instruction {
                Instruction::Ranges(ranges) => {
                    let mut previous = None;
                    let mut transitions = Vec::new();
                    for &(start, end, next) in ranges {
                        if start > end || previous.is_some_and(|p| start <= p) {
                            return Err(Error::Contract("invalid pattern byte ranges"));
                        }
                        previous = Some(end);
                        transitions.push(Transition {
                            start,
                            end,
                            next: state(next)?,
                        });
                    }
                    builder.add_sparse(transitions)
                }
                Instruction::Look(look, next) => {
                    builder.add_look(state(*next)?, Look::from_repr(*look).ok_or(Error::Syntax)?)
                }
                Instruction::Union(targets) => builder.add_union(
                    targets
                        .iter()
                        .map(|n| state(*n))
                        .collect::<Result<_, _>>()?,
                ),
                Instruction::Fail => builder.add_fail(),
                Instruction::Match => builder.add_match(),
            }
            .map_err(|_| Error::Contract("invalid prepared pattern"))?;
            if added.as_usize() != position {
                return Err(Error::Contract("invalid pattern numbering"));
            }
        }
        let start = state(self.start)?;
        builder.finish_pattern(start).map_err(|_| Error::Syntax)?;
        let nfa = builder
            .build(start, start)
            .map_err(|_| Error::Contract("invalid prepared pattern graph"))?;
        PikeVM::new_from_nfa(nfa).map_err(|_| Error::Syntax)
    }
}

pub(super) fn load(bytes: &[u8]) -> Result<PikeVM, Error> {
    if bytes.len() > MAX_PATTERN_BYTES {
        return Err(Error::Limit);
    }
    let image: Image = serde_json::from_slice(bytes).map_err(|_| Error::Syntax)?;
    image.restore()
}

pub(super) fn run(vm: &PikeVM, value: &str) -> Result<bool, Error> {
    if work(vm, value)? > 8_000_000 {
        return Err(Error::Limit);
    }
    let mut cache = vm.create_cache();
    if cache.memory_usage() > 32 * 1024 * 1024 {
        return Err(Error::Limit);
    }
    Ok(vm.is_match(&mut cache, value))
}

pub(super) fn work(vm: &PikeVM, value: &str) -> Result<usize, Error> {
    vm.get_nfa()
        .states()
        .len()
        .checked_mul(value.len().max(1))
        .ok_or(Error::Limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn malformed_images_refuse_invalid_transitions_and_numbering() {
        for image in [
            serde_json::json!({"utf8":true,"start":5,"states":["Match"]}),
            serde_json::json!({"utf8":true,"start":0,"states":[{"Look":[0,9]}]}),
            serde_json::json!({"utf8":true,"start":0,"states":[{"Ranges":[[2,1,0]]}]}),
            serde_json::json!({"utf8":true,"start":0,"states":[{"Union":[4]}]}),
        ] {
            assert!(load(&serde_json::to_vec(&image).unwrap()).is_err());
        }
    }

    proptest! {
        #[test]
        fn bounded_image_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = load(&bytes);
        }
        #[test]
        fn persisted_literal_programs_agree_with_original(value in "[a-zA-Zé_0-9]{0,60}") {
            static PREPARED: std::sync::OnceLock<(Store, PikeVM)> = std::sync::OnceLock::new();
            let key = r"regex:(?:\p{L}|[0-9_]){0,60}";
            let (store, vm) = PREPARED.get_or_init(|| {
                let mut store = Store::default();
                store.insert("regex", r"(?:\p{L}|[0-9_]){0,60}").unwrap();
                let vm = load(&store.images[key]).unwrap();
                (store, vm)
            });
            prop_assert_eq!(run(vm, &value).unwrap(), store.matches(key, &value).unwrap());
        }
    }
}
