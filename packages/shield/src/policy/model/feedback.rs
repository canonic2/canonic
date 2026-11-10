//! Bounded nonauthorizing source identity and selected agent-feedback changes.
use super::{CustomDocument, PackDocument};
use crate::policy::{
    CustomRules, Pack, Profile,
    rules::{Precondition, Rule, Unmet},
};
use ring::digest::{Context, SHA256};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::io::{self, Write};
mod decisions;
pub(crate) use decisions::custom_bound;

pub(crate) const WORK_BYTES: usize = 16 * 1024 * 1024;
const ENTRY_BYTES: usize = 2 * 1024 * 1024;
const SUMMARY_BYTES: usize = 64 * 1024;
const SUMMARY_ENTRIES: usize = 128;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SourceIdentity {
    digest: String,
    bytes: usize,
}

struct Hash {
    context: Context,
    bytes: usize,
    entry: usize,
    limit: usize,
}
impl Hash {
    fn new(limit: usize) -> Self {
        let mut context = Context::new(&SHA256);
        let domain = b"shield-permission-source-v1\n";
        context.update(domain);
        Self {
            context,
            bytes: domain.len(),
            entry: 0,
            limit,
        }
    }
    fn value(&mut self, value: &impl Serialize) -> bool {
        self.entry = 0;
        serde_json::to_writer(&mut *self, value).is_ok() && self.write_all(b"\n").is_ok()
    }
    fn finish(self) -> SourceIdentity {
        use std::fmt::Write as _;
        let hash = self.context.finish();
        let mut digest = String::with_capacity(64);
        for byte in hash.as_ref() {
            write!(&mut digest, "{byte:02x}").expect("formatting SHA-256 into String");
        }
        SourceIdentity {
            digest,
            bytes: self.bytes,
        }
    }
}
impl Write for Hash {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes)
            || bytes.len() > ENTRY_BYTES.saturating_sub(self.entry)
        {
            return Err(io::Error::other("comparison work budget"));
        }
        self.context.update(bytes);
        self.bytes += bytes.len();
        self.entry += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PermissionRule<'a>(&'a Rule);
impl Serialize for PermissionRule<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Exhaustive destructuring makes a new semantic field a compile error.
        let Rule {
            id,
            legacy,
            legacy_normalized,
            invocation,
            command_set,
            decision,
            conditions,
            requirements,
            on_unmet,
            reason: _,
            guidance,
            expires_at,
            fallback,
            when,
            agents,
            matcher: _,
        } = self.0;
        if let Some(crate::policy::rules::Guidance {
            message: _,
            alternatives: _,
        }) = guidance
        {
            // A future guidance field must receive an explicit classification.
        }
        // matcher is a derived, nonserialized cache of these same typed fields.
        (
            id,
            legacy,
            legacy_normalized,
            invocation,
            command_set,
            decision,
            conditions,
            requirements,
            on_unmet.as_ref().map(PermissionUnmet),
            expires_at,
            fallback,
            when,
            agents,
        )
            .serialize(serializer)
    }
}
struct PermissionUnmet<'a>(&'a Unmet);
impl Serialize for PermissionUnmet<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Unmet {
            decision,
            message: _,
            alternatives: _,
        } = self.0;
        decision.serialize(serializer)
    }
}
struct PermissionGate<'a>(&'a Precondition);
impl Serialize for PermissionGate<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Precondition {
            id,
            conditions,
            when,
            requires_any,
            on_unmet,
            reason: _,
        } = self.0;
        (
            id,
            conditions,
            when,
            requires_any,
            PermissionUnmet(on_unmet),
        )
            .serialize(serializer)
    }
}

fn capture(hash: Hash, success: bool) -> Option<SourceIdentity> {
    crate::diagnostics::trace::count("comparison.source_identity_bytes", hash.bytes as u64);
    if !success {
        crate::diagnostics::trace::count("comparison.source_identity_budget", 1);
        return None;
    }
    Some(hash.finish())
}
pub(in crate::policy) fn source_pack(document: &PackDocument) -> Option<SourceIdentity> {
    let _trace = crate::diagnostics::trace::span("comparison.source_identity");
    let PackDocument {
        retained_legacy,
        schema_version,
        kind,
        id,
        version,
        description,
        license,
        maintainers,
        engines,
        capabilities,
        rules,
        invocations,
        command_sets,
        dependencies,
        variables,
        preconditions,
        directory_options,
        mcp_integrations,
    } = document;
    let mut hash = Hash::new(WORK_BYTES);
    let mut ok = hash.value(&(
        (
            retained_legacy,
            schema_version,
            kind,
            id,
            version,
            description,
            license,
            maintainers,
            engines,
            capabilities,
        ),
        (
            invocations,
            command_sets,
            dependencies,
            variables,
            directory_options,
            mcp_integrations,
            rules.len(),
            preconditions.len(),
        ),
    ));
    for rule in rules {
        if !ok {
            break;
        }
        ok = hash.value(&PermissionRule(rule));
    }
    for gate in preconditions {
        if !ok {
            break;
        }
        ok = hash.value(&PermissionGate(gate));
    }
    capture(hash, ok)
}
pub(in crate::policy) fn source_custom(document: &CustomDocument) -> Option<SourceIdentity> {
    let _trace = crate::diagnostics::trace::span("comparison.source_identity");
    let CustomDocument {
        schema_version,
        kind,
        capabilities,
        rules,
        directory_options,
    } = document;
    let mut hash = Hash::new(WORK_BYTES);
    let mut ok = hash.value(&(
        schema_version,
        kind,
        capabilities,
        directory_options,
        rules.len(),
    ));
    for rule in rules {
        if !ok {
            break;
        }
        ok = hash.value(&PermissionRule(rule));
    }
    capture(hash, ok)
}

#[derive(Debug, Serialize)]
pub(crate) struct Summary {
    pub(crate) changed: bool,
    complete: bool,
    truncated: bool,
    work_budget_bytes: usize,
    entry_limit: usize,
    byte_limit: usize,
    incomplete_reason: Option<&'static str>,
    entries: Vec<Value>,
}
impl Summary {
    fn empty() -> Self {
        Self {
            changed: false,
            complete: true,
            truncated: false,
            work_budget_bytes: WORK_BYTES,
            entry_limit: SUMMARY_ENTRIES,
            byte_limit: SUMMARY_BYTES,
            incomplete_reason: None,
            entries: Vec::new(),
        }
    }
}
pub(crate) struct Inspection {
    pub(crate) before: Option<Vec<SourceIdentity>>,
    pub(crate) after: Option<Vec<SourceIdentity>>,
    pub(crate) summary: Summary,
    pub(crate) remaining: usize,
    pub(crate) unavailable_reason: Option<&'static str>,
}
fn sources(packs: &[(&Pack, Profile)], custom: &[&CustomRules]) -> Option<Vec<SourceIdentity>> {
    let mut sources = Vec::new();
    for (pack, _) in packs {
        sources.push(pack.comparison_source.clone()?);
    }
    for custom in custom {
        sources.extend(custom.comparison_sources.as_ref()?.iter().cloned());
    }
    Some(sources)
}
impl SourceIdentity {
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.digest == other.digest && self.bytes == other.bytes
    }
}
pub(crate) fn descriptor(value: &impl Serialize, limit: usize) -> Option<SourceIdentity> {
    let mut hash = Hash::new(limit);
    let ok = hash.value(value);
    crate::diagnostics::trace::count("comparison.composition_bytes", hash.bytes as u64);
    ok.then(|| hash.finish())
}
pub(crate) fn byte_len(identity: &SourceIdentity) -> usize {
    identity.bytes
}

#[derive(Serialize)]
struct Feedback<'a> {
    reason: Option<&'a str>,
    guidance: Option<&'a crate::policy::rules::Guidance>,
    on_unmet: Option<UnmetFeedback<'a>>,
}
#[derive(Serialize)]
struct UnmetFeedback<'a> {
    message: Option<&'a str>,
    alternatives: Option<&'a [String]>,
}
fn unmet(value: &Unmet) -> UnmetFeedback<'_> {
    let Unmet {
        decision: _,
        message,
        alternatives,
    } = value;
    UnmetFeedback {
        message: message.as_deref(),
        alternatives: alternatives.as_deref(),
    }
}
fn has_unmet_feedback(value: &Unmet) -> bool {
    value.message.is_some() || value.alternatives.is_some()
}
fn rule_feedback(rule: &Rule) -> Feedback<'_> {
    Feedback {
        reason: rule.reason.as_deref(),
        guidance: rule.guidance.as_ref(),
        on_unmet: rule
            .on_unmet
            .as_ref()
            .filter(|value| has_unmet_feedback(value))
            .map(unmet),
    }
}
fn gate_feedback(gate: &Precondition) -> Feedback<'_> {
    Feedback {
        reason: gate.reason.as_deref(),
        guidance: None,
        on_unmet: has_unmet_feedback(&gate.on_unmet).then(|| unmet(&gate.on_unmet)),
    }
}
impl Feedback<'_> {
    fn present(&self) -> bool {
        self.reason.is_some() || self.guidance.is_some() || self.on_unmet.is_some()
    }
}

struct Entry<'a> {
    owner: &'a str,
    kind: &'static str,
    id: &'a str,
    feedback: Feedback<'a>,
}
fn entries<'a>(
    packs: &'a [(&'a Pack, Profile)],
    custom: &'a [&'a CustomRules],
) -> impl Iterator<Item = Entry<'a>> + 'a {
    packs
        .iter()
        .flat_map(|(pack, _)| {
            pack.document
                .rules
                .iter()
                .map(move |rule| Entry {
                    owner: pack.id(),
                    kind: "rule",
                    id: &rule.id,
                    feedback: rule_feedback(rule),
                })
                .chain(pack.document.preconditions.iter().map(move |gate| Entry {
                    owner: pack.id(),
                    kind: "precondition",
                    id: &gate.id,
                    feedback: gate_feedback(gate),
                }))
        })
        .chain(custom.iter().flat_map(|custom| {
            custom.document.rules.iter().map(|rule| Entry {
                owner: "custom",
                kind: "rule",
                id: &rule.id,
                feedback: rule_feedback(rule),
            })
        }))
}
struct Bounded {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("feedback summary budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn inspect(
    before_packs: &[(&Pack, Profile)],
    before_custom: &[&CustomRules],
    after_packs: &[(&Pack, Profile)],
    after_custom: &[&CustomRules],
) -> Inspection {
    let before = sources(before_packs, before_custom);
    let after = sources(after_packs, after_custom);
    let mut summary = Summary::empty();
    let source_work = source_work(before.as_deref(), after.as_deref());
    let mut work = source_work.unwrap_or(0);
    let mut retained = 0_usize;
    let mut first = entries(before_packs, before_custom);
    let mut second = entries(after_packs, after_custom);
    loop {
        let before = first.next();
        let after = second.next();
        if before.is_none() && after.is_none() {
            break;
        }
        let mut old = Bounded {
            bytes: Vec::new(),
            limit: (ENTRY_BYTES / 2).min(WORK_BYTES.saturating_sub(work)),
        };
        let ok = serde_json::to_writer(
            &mut old,
            &before
                .as_ref()
                .map(|row| &row.feedback)
                .filter(|feedback| feedback.present()),
        )
        .is_ok();
        work = work.saturating_add(old.bytes.len());
        let mut new = Bounded {
            bytes: Vec::new(),
            limit: (ENTRY_BYTES / 2).min(WORK_BYTES.saturating_sub(work)),
        };
        let ok = ok
            && serde_json::to_writer(
                &mut new,
                &after
                    .as_ref()
                    .map(|row| &row.feedback)
                    .filter(|feedback| feedback.present()),
            )
            .is_ok();
        work = work.saturating_add(new.bytes.len());
        if !ok || work > WORK_BYTES {
            summary.complete = false;
            summary.truncated = true;
            summary.incomplete_reason = Some("work-budget");
            break;
        }
        if old.bytes == new.bytes {
            continue;
        }
        summary.changed = true;
        if summary.entries.len() >= SUMMARY_ENTRIES {
            summary.truncated = true;
            summary.incomplete_reason.get_or_insert("entry-limit");
            continue;
        }
        if retained
            .saturating_add(old.bytes.len())
            .saturating_add(new.bytes.len())
            .saturating_add(1024)
            > SUMMARY_BYTES - 1024
        {
            summary.truncated = true;
            summary.incomplete_reason.get_or_insert("byte-limit");
            continue;
        }
        let row = entry_row(before.as_ref(), after.as_ref(), &old.bytes, &new.bytes);
        let bytes = crate::json::encoded_len(&row).unwrap_or(SUMMARY_BYTES);
        if retained.saturating_add(bytes).saturating_add(1) > SUMMARY_BYTES - 1024 {
            summary.truncated = true;
            summary.incomplete_reason.get_or_insert("byte-limit");
            continue;
        }
        retained += bytes + 1;
        summary.entries.push(row);
    }
    crate::diagnostics::trace::count(
        "comparison.feedback_bytes",
        work.saturating_sub(source_work.unwrap_or(0)) as u64,
    );
    let admitted = source_work.is_some() && summary.complete;
    let unavailable_reason = if !summary.complete {
        Some("feedback-work-budget")
    } else if before.is_none() || after.is_none() {
        Some("source-evidence-unavailable")
    } else if source_work.is_none() {
        Some("aggregate-source-budget")
    } else {
        None
    };
    summary.complete &= !summary.truncated;
    Inspection {
        before: if admitted { before } else { None },
        after: if admitted { after } else { None },
        remaining: WORK_BYTES.saturating_sub(work),
        unavailable_reason,
        summary,
    }
}

fn source_work(
    before: Option<&[SourceIdentity]>,
    after: Option<&[SourceIdentity]>,
) -> Option<usize> {
    before
        .zip(after)
        .and_then(|(before, after)| {
            before
                .iter()
                .chain(after)
                .try_fold(0_usize, |used, source| used.checked_add(source.bytes))
        })
        .filter(|bytes| *bytes <= WORK_BYTES)
}

fn entry_row(
    before: Option<&Entry<'_>>,
    after: Option<&Entry<'_>>,
    old: &[u8],
    new: &[u8],
) -> Value {
    let entry = after.or(before).expect("at least one entry");
    let mut row = json!({"owner":entry.owner,"entry_kind":entry.kind,"entry_id":entry.id,"before":serde_json::from_slice::<Value>(old).unwrap_or(Value::Null),"after":serde_json::from_slice::<Value>(new).unwrap_or(Value::Null)});
    let identity = |entry: &Entry<'_>| json!({"owner":entry.owner,"kind":entry.kind,"id":entry.id});
    row["before_entry"] = before.map_or(Value::Null, identity);
    row["after_entry"] = after.map_or(Value::Null, identity);
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_identity_keeps_variable_declarations_and_original_activation_tests() {
        let original = json!({"schema_version":1,"kind":"pack","id":"acme/variables","version":"0.1.0","description":"Acme variable proof","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1","variables-v1"],"variables":{"target":{"type":"string","description":"Original declaration","default":"read"}},"rules":[{"id":"inspect","when":[{"var":"target","set":true}],"reason":"Before","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[{"var":"target"}]}}}]});
        let before = Pack::from_yaml(&original.to_string()).unwrap();
        assert!(before.document.variables.is_empty());
        assert!(before.document.rules[0].when.is_none());
        for field in ["declaration", "when"] {
            let mut changed = original.clone();
            changed["rules"][0]["reason"] = json!("After");
            if field == "declaration" {
                changed["variables"]["target"]["description"] = json!("Different declaration");
            } else {
                changed["rules"][0]["when"] = json!([{"var":"target","equals":"read"}]);
            }
            let after = Pack::from_yaml(&changed.to_string()).unwrap();
            assert_eq!(
                serde_json::to_value(PermissionRule(&before.document.rules[0])).unwrap(),
                serde_json::to_value(PermissionRule(&after.document.rules[0])).unwrap()
            );
            assert!(
                !before
                    .comparison_source
                    .as_ref()
                    .unwrap()
                    .same(after.comparison_source.as_ref().unwrap()),
                "{field}"
            );
        }
    }
    fn custom(reason: &str, count: usize) -> CustomRules {
        let rules=(0..count).map(|index|json!({"id":format!("r{index}"),"decision":"allow","reason":reason,"conditions":{"domain":"mcp","server":"acme","tool":"inspect"}})).collect::<Vec<_>>();
        CustomRules::from_yaml(&crate::policy::rules::document(&rules).unwrap().to_string())
            .unwrap()
    }
    #[test]
    fn feedback_proof_admits_aggregate_source_work_and_missing_evidence_falls_back() {
        let mut before = custom("Before", 1);
        let mut after = custom("After", 1);
        for custom in [&mut before, &mut after] {
            custom.comparison_sources.as_mut().unwrap()[0].bytes = WORK_BYTES / 2 + 1;
        }
        let report = inspect(&[], &[&before], &[], &[&after]);
        assert!(report.summary.changed);
        assert!(report.before.is_none() && report.after.is_none());
        before.comparison_sources = None;
        assert!(inspect(&[], &[&before], &[], &[&after]).before.is_none());
    }
    #[test]
    fn feedback_summary_has_bounded_rows_bytes_and_explicit_truncation() {
        let before = custom("Before", SUMMARY_ENTRIES + 2);
        let after = custom("After", SUMMARY_ENTRIES + 2);
        let report = inspect(&[], &[&before], &[], &[&after]);
        assert!(report.summary.changed);
        assert!(report.summary.truncated);
        assert!(!report.summary.complete);
        assert_eq!(report.summary.entries.len(), SUMMARY_ENTRIES);
        assert!(crate::json::encoded_len(&report.summary).unwrap() <= SUMMARY_BYTES);
        assert_eq!(report.summary.incomplete_reason, Some("entry-limit"));
    }
}
