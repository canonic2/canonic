//! Diagnostic-only privacy annotations; raw values and matching stay unchanged.
use super::{HereDoc, Node, Span, Word};
use crate::evidence::{projection::Sensitive, sensitive_key};
use serde::{Serialize, ser::SerializeSeq};

fn sensitive_assignment(word: &Word) -> bool {
    word.literal
        .as_deref()
        .unwrap_or(&word.raw)
        .split_once('=')
        .is_some_and(|(name, _)| sensitive_key(name.trim_start_matches('-')))
}
pub(in crate::shell) fn mark(node: &mut Node) {
    walk(
        node,
        &mut |words| {
            let mut next = false;
            for word in words {
                let consumed = next;
                word.private_evidence |= consumed || sensitive_assignment(word);
                let value = word
                    .literal
                    .as_deref()
                    .unwrap_or(&word.raw)
                    .trim_matches(['\'', '"']);
                next = !consumed
                    && value.starts_with("--")
                    && sensitive_key(value.trim_start_matches('-'));
            }
        },
        &mut |_| {},
    );
}
pub(in crate::shell) fn mark_span(node: &mut Node, span: Span) {
    walk(
        node,
        &mut |words| {
            for word in words {
                mark_word(word, span);
            }
        },
        &mut |body| {
            body.private_evidence |= overlaps(body.span, span);
        },
    );
}
pub(in crate::shell) fn mark_word(word: &mut Word, span: Span) {
    word.private_evidence |= overlaps(word.span, span);
}
pub(in crate::shell) fn mark_ranges(word: &mut Word, spans: &[Span]) {
    let index = spans.partition_point(|span| span.end <= word.span.start);
    if let Some(span) = spans.get(index) {
        mark_word(word, *span);
    }
}
pub(in crate::shell) fn intersects(span: Span, spans: &[Span]) -> bool {
    let index = spans.partition_point(|candidate| candidate.end <= span.start);
    spans
        .get(index)
        .is_some_and(|candidate| overlaps(span, *candidate))
}
pub(in crate::shell) fn overlaps(left: Span, right: Span) -> bool {
    left.start < right.end && right.start < left.end
}
pub(in crate::shell) fn spans(node: &mut Node) -> Vec<Span> {
    let mut words = Vec::new();
    let mut bodies = Vec::new();
    walk(
        node,
        &mut |items| {
            words.extend(
                items
                    .iter()
                    .filter(|word| word.private_evidence)
                    .map(|word| word.span),
            );
        },
        &mut |body| {
            if body.private_evidence {
                bodies.push(body.span);
            }
        },
    );
    words.extend(bodies);
    words.sort_unstable_by_key(|span| (span.start, span.end));
    let mut merged: Vec<Span> = Vec::with_capacity(words.len());
    for span in words {
        if let Some(last) = merged.last_mut()
            && span.start <= last.end
        {
            last.end = last.end.max(span.end);
        } else {
            merged.push(span);
        }
    }
    merged
}
pub(in crate::shell) fn opaque(node: &Node) -> bool {
    match node {
        Node::Opaque { .. } => true,
        Node::Command { .. } | Node::Expression { .. } => false,
        Node::Sequence { children } => children.iter().any(opaque),
        Node::Binary { left, right, .. } => opaque(left) || opaque(right),
        Node::Group { body, .. }
        | Node::Background { body }
        | Node::Decorated { body, .. }
        | Node::Redirected { body, .. }
        | Node::For { body, .. }
        | Node::ArithmeticLoop { body, .. }
        | Node::Function { body, .. } => opaque(body),
        Node::Conditional { condition, yes, no } => {
            opaque(condition) || opaque(yes) || no.as_deref().is_some_and(opaque)
        }
        Node::Loop {
            condition, body, ..
        } => opaque(condition) || opaque(body),
        Node::Case { arms, .. } => arms.iter().any(|arm| opaque(&arm.body)),
    }
}
fn redirects(
    items: &mut [super::Redirection],
    words: &mut impl FnMut(&mut [Word]),
    body: &mut impl FnMut(&mut HereDoc),
) {
    for item in items {
        words(std::slice::from_mut(&mut item.target));
        if let Some(here) = &mut item.heredoc {
            body(here);
        }
    }
}
fn walk(node: &mut Node, words: &mut impl FnMut(&mut [Word]), body: &mut impl FnMut(&mut HereDoc)) {
    match node {
        Node::Opaque { .. } => {}
        Node::Command {
            words: items,
            redirections,
            ..
        } => {
            words(items);
            redirects(redirections, words, body);
        }
        Node::Sequence { children } => {
            for child in children {
                walk(child, words, body);
            }
        }
        Node::Binary { left, right, .. } => {
            walk(left, words, body);
            walk(right, words, body);
        }
        Node::Group { body: child, .. }
        | Node::Background { body: child }
        | Node::Decorated { body: child, .. }
        | Node::Function { body: child, .. } => walk(child, words, body),
        Node::Redirected {
            body: child,
            redirections,
        } => {
            walk(child, words, body);
            redirects(redirections, words, body);
        }
        Node::Conditional { condition, yes, no } => {
            walk(condition, words, body);
            walk(yes, words, body);
            if let Some(no) = no {
                walk(no, words, body);
            }
        }
        Node::Loop {
            condition,
            body: child,
            ..
        } => {
            walk(condition, words, body);
            walk(child, words, body);
        }
        Node::For {
            values,
            body: child,
            ..
        } => {
            words(values);
            walk(child, words, body);
        }
        Node::ArithmeticLoop {
            header,
            body: child,
        } => {
            words(header);
            walk(child, words, body);
        }
        Node::Expression { words: items, .. } => words(items),
        Node::Case { subject, arms } => {
            words(std::slice::from_mut(subject));
            for arm in arms {
                words(&mut arm.patterns);
                walk(&mut arm.body, words, body);
            }
        }
    }
}

fn protect<T: Serialize + ?Sized, S: serde::Serializer>(
    value: &T,
    private: bool,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if private {
        Sensitive(value).serialize(serializer)
    } else {
        value.serialize(serializer)
    }
}
struct Text<'a> {
    value: &'a str,
    private: bool,
}
impl Serialize for Text<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        protect(self.value, self.private, serializer)
    }
}
impl Serialize for Word {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Evidence<'a> {
            span: Span,
            raw: Text<'a>,
            literal: Option<Text<'a>>,
            substitutions: Substitutions<'a>,
            fragments: Fragments<'a>,
        }
        let private = self.private_evidence;
        Evidence {
            span: self.span,
            raw: Text {
                value: &self.raw,
                private,
            },
            literal: self.literal.as_deref().map(|value| Text { value, private }),
            substitutions: Substitutions {
                items: &self.substitutions,
                private,
            },
            fragments: Fragments {
                items: &self.fragments,
                private,
            },
        }
        .serialize(serializer)
    }
}
#[derive(Serialize)]
struct Fragment<'a> {
    kind: &'static str,
    span: Span,
    raw: Text<'a>,
}
#[derive(Serialize)]
struct Substitution<'a> {
    span: Span,
    source: Text<'a>,
    kind: &'static str,
}
struct Fragments<'a> {
    items: &'a [super::Fragment],
    private: bool,
}
impl Serialize for Fragments<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.items.len()))?;
        for item in self.items {
            sequence.serialize_element(&Fragment {
                kind: item.kind,
                span: item.span,
                raw: Text {
                    value: &item.raw,
                    private: self.private,
                },
            })?;
        }
        sequence.end()
    }
}
struct Substitutions<'a> {
    items: &'a [super::Substitution],
    private: bool,
}
impl Serialize for Substitutions<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.items.len()))?;
        for item in self.items {
            sequence.serialize_element(&Substitution {
                span: item.span,
                source: Text {
                    value: &item.source,
                    private: self.private,
                },
                kind: item.kind,
            })?;
        }
        sequence.end()
    }
}
impl Serialize for HereDoc {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Evidence<'a> {
            span: Span,
            source: Text<'a>,
            raw_source: Text<'a>,
            expand: bool,
            source_map: &'a [super::SourceMapping],
        }
        Evidence {
            span: self.span,
            source: Text {
                value: &self.source,
                private: self.private_evidence,
            },
            raw_source: Text {
                value: &self.raw_source,
                private: self.private_evidence,
            },
            expand: self.expand,
            source_map: &self.source_map,
        }
        .serialize(serializer)
    }
}
