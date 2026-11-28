//! Compile bounded ASCII DFAs once and export only reachable byte-class transitions.
use super::*;
use regex_automata::{
    Input,
    dfa::{Automaton, StartKind, dense},
    util::syntax,
};

const MAX_STATES: usize = 512;
const MAX_BYTES: usize = 16384;

/// The linear DFA stays the fast path. A pattern it cannot run, such as
/// lookaround, backreferences, non-ASCII, `\p{}`, `v` or an automaton over its
/// bounds, runs on the page's native `RegExp` like the SDK's driver-side test,
/// after the pinned engine confirms its syntax.
pub(super) fn compile(pattern: &str, flags: &str) -> Result<Value, &'static str> {
    if pattern.len() > 2048 || flags.len() > 8 {
        return Err("WEBSOCKET_REGEX_BOUNDS");
    }
    // As for HTTP routes, the translator's refusal of ASCII syntax is final;
    // only unsupported features and non-ASCII sources reach the pinned engine.
    match crate::browser::routing::translate_regex(pattern, flags) {
        Ok(translated) => {
            if let Ok((program, _)) = export(&translated, flags, MAX_BYTES) {
                return Ok(json!({"automaton":program}));
            }
        }
        Err("ROUTE_REGEX_UNSUPPORTED") => {}
        Err(_) if pattern.is_ascii() => return Err("WEBSOCKET_REGEX_INVALID"),
        Err(_) => {}
    }
    match crate::browser::routing::ecma::Regexp::compile(pattern, flags) {
        Ok(_) => Ok(json!({"nativeRegex":{"source":pattern,"flags":flags}})),
        Err("ROUTE_REGEX_INVALID") => Err("WEBSOCKET_REGEX_INVALID"),
        Err(_) => Err("WEBSOCKET_REGEX_BOUNDS"),
    }
}
/// One program per non-wildcard URLPattern component (`null` for `*`), sharing
/// one regex's state and byte budget so a pattern costs the page no more.
pub(super) fn compile_pattern(
    pattern: &crate::browser::routing::UrlPattern,
) -> Result<Value, &'static str> {
    let sources = pattern
        .sources()
        .map_err(|_| "WEBSOCKET_REGEX_UNSUPPORTED")?;
    let mut budget = MAX_BYTES;
    let mut programs = Vec::with_capacity(sources.len());
    for source in sources {
        programs.push(match source {
            Some(source) => {
                let (program, bytes) = export(&source, "i", budget)?;
                budget -= bytes;
                program
            }
            None => Value::Null,
        });
    }
    Ok(json!({"urlPattern":programs}))
}
/// A translated Rust `source` as a page program and its decoded table bytes,
/// refused when the tables would exceed `budget`.
fn export(source: &str, flags: &str, budget: usize) -> Result<(Value, usize), &'static str> {
    let pattern = source;
    let dfa = dense::Builder::new()
        .configure(
            dense::Config::new()
                .start_kind(StartKind::Unanchored)
                .dfa_size_limit(Some(65536))
                .determinize_size_limit(Some(4 * 1024 * 1024)),
        )
        .syntax(
            syntax::Config::new()
                .unicode(false)
                .utf8(false)
                .case_insensitive(flags.contains('i'))
                .multi_line(flags.contains('m'))
                .dot_matches_new_line(flags.contains('s')),
        )
        .build(pattern)
        .map_err(|_| "WEBSOCKET_REGEX_BOUNDS")?;
    let start = dfa
        .start_state_forward(&Input::new(b""))
        .map_err(|_| "WEBSOCKET_REGEX_UNSUPPORTED")?;
    let mut classes = Vec::with_capacity(128);
    let mut representatives = Vec::new();
    let mut alphabet = BTreeMap::new();
    for byte in 0..128_u8 {
        let native = dfa.byte_classes().get(byte);
        let class = *alphabet.entry(native).or_insert_with(|| {
            representatives.push(byte);
            u8::try_from(representatives.len() - 1).expect("ASCII alphabet")
        });
        classes.push(class);
    }
    let width = representatives.len();
    let mut states = vec![start];
    let mut indexes = BTreeMap::from([(start, 0_u16)]);
    let mut transitions = Vec::new();
    let mut acceptance = Vec::new();
    let mut cursor = 0;
    while cursor < states.len() {
        let state = states[cursor];
        acceptance.push(
            u8::from(dfa.is_match_state(state))
                | (u8::from(dfa.is_match_state(dfa.next_eoi_state(state))) << 1),
        );
        for byte in &representatives {
            let next = dfa.next_state(state, *byte);
            if dfa.is_quit_state(next) {
                return Err("WEBSOCKET_REGEX_UNSUPPORTED");
            }
            let index = if let Some(index) = indexes.get(&next) {
                *index
            } else {
                let count = states.len() + 1;
                if count > MAX_STATES || 128 + count * (width * 2 + 1) > budget {
                    return Err("WEBSOCKET_REGEX_BOUNDS");
                }
                let index = u16::try_from(states.len()).expect("Bounded DFA states");
                states.push(next);
                indexes.insert(next, index);
                index
            };
            transitions.extend_from_slice(&index.to_le_bytes());
        }
        cursor += 1;
    }
    let bytes = classes.len() + transitions.len() + acceptance.len();
    if bytes > budget {
        return Err("WEBSOCKET_REGEX_BOUNDS");
    }
    Ok((
        json!({"width":width,"classes":STANDARD.encode(classes),"transitions":STANDARD.encode(transitions),"acceptance":STANDARD.encode(acceptance)}),
        bytes,
    ))
}
