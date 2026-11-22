//! Indexed scope postings and invocation dispatch; authority remains in the resolver.
mod invocations;
mod mcp;
mod workspace;
use crate::policy::{
    CustomRules, Error, Pack, Profile, Request, model,
    resolution::{index, normalized},
    rules::{
        Facts, Rule,
        patterns::{self, Provider},
        selectors,
    },
};
pub(crate) use mcp::ResolvedMcp;
use regex_automata::nfa::thompson::pikevm::PikeVM;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
/// The plan format this engine writes and loads. Change it whenever compiled
/// bytes would decide differently, such as how patterns are prepared, so
/// plans written before the change are rebuilt instead of loaded.
pub(crate) const FORMAT: &str = "SHPLAN16";
const MAGIC: &[u8] = FORMAT.as_bytes();
const HEADER: usize = 16;
const ENTRY: usize = 16;
pub(crate) const MAX_BYTES: usize = 512 * 1024 * 1024;
const MAX_ROWS: usize = 1_000_000;
const PATTERN_OWNER: usize = u32::MAX as usize;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    engine: String,
    packs: Vec<PackMetadata>,
    custom: Option<CustomMetadata>,
    effective: serde_json::Value,
    historical_semantic: bool,
    /// Codex prefixes of active rules that allow, in owner order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    codex_prefixes: Vec<crate::policy::rules::agent_outputs::CodexPrefix>,
}
impl Metadata {
    fn validate(&self) -> Result<(), Error> {
        for pack in &self.packs {
            if pack.origins.len() > 1024 {
                return Err(Error::Limit);
            }
            pack.document.validate_schema()?;
            crate::policy::resolution::invocations::definitions(&pack.document)?;
            for precondition in &pack.document.preconditions {
                let mut precondition = precondition.clone();
                crate::graphql::descriptor(&mut precondition.conditions)?;
                for alternative in &mut precondition.requires_any {
                    crate::graphql::descriptor(alternative)?;
                }
                precondition.validate_bound()?;
            }
        }
        if let Some(custom) = &self.custom {
            custom.document.validate_schema()?;
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PackMetadata {
    document: model::PackDocument,
    retained_legacy: bool,
    digest: String,
    semantic: bool,
    profile: Profile,
    origins: BTreeMap<String, crate::policy::resolution::provenance::Original>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CustomMetadata {
    document: model::CustomDocument,
    digest: String,
    semantic: bool,
    time_sensitive: bool,
}
struct Entry {
    start: usize,
    end: usize,
    key_end: usize,
    owner: usize,
}
pub(crate) struct Plan {
    bytes: Vec<u8>,
    metadata: Metadata,
    table: usize,
    count: usize,
    custom: Option<CustomRules>,
    programs: Mutex<Programs>,
    /// Directory options declared by the selected packs and custom rules.
    directory_options: Vec<crate::policy::rules::directories::DirectoryOption>,
}
/// Codex prefixes of every pack rule under its profile, then of custom rules,
/// which select as Recommended.
fn codex_prefixes(
    packs: &[(Pack, Profile)],
    custom: Option<&CustomRules>,
) -> Vec<crate::policy::rules::agent_outputs::CodexPrefix> {
    let packs = packs.iter().flat_map(|(pack, profile)| {
        pack.document
            .rules
            .iter()
            .map(move |rule| (rule, pack.id(), *profile))
    });
    let custom = custom.into_iter().flat_map(|custom| {
        custom
            .document
            .rules
            .iter()
            .map(|rule| (rule, "custom", Profile::Recommended))
    });
    packs
        .chain(custom)
        .flat_map(|(rule, owner, profile)| {
            crate::policy::rules::agent_outputs::codex_prefixes(rule, owner, profile)
        })
        .collect()
}
#[derive(Default)]
struct Programs {
    items: BTreeMap<String, Prepared>,
    bytes: usize,
}
#[derive(Clone)]
enum Prepared {
    Pattern(Arc<PikeVM>),
    Graphql(Arc<serde_json::Value>),
}
impl Plan {
    pub(crate) fn compile(
        packs: Vec<(Pack, Profile)>,
        custom: Option<CustomRules>,
        effective: serde_json::Value,
    ) -> Result<Self, Error> {
        let mut encoding = workspace::Encoding::new();
        let mut metadata = Metadata {
            engine: env!("CARGO_PKG_VERSION").into(),
            packs: vec![],
            custom: None,
            effective,
            historical_semantic: crate::policy::historical_semantic_selection(
                &packs
                    .iter()
                    .map(|(pack, profile)| (pack, *profile))
                    .collect::<Vec<_>>(),
                &custom.iter().collect::<Vec<_>>(),
            ),
            codex_prefixes: codex_prefixes(&packs, custom.as_ref()),
        };
        for (owner, (mut pack, profile)) in packs.into_iter().enumerate() {
            encoding.patterns(std::mem::take(&mut pack.patterns))?;
            for gate in &mut pack.document.preconditions {
                encoding.detach(&mut gate.conditions)?;
                for alternative in &mut gate.requires_any {
                    encoding.detach(alternative)?;
                }
            }
            for mut rule in std::mem::take(&mut pack.document.rules) {
                encoding.detach(&mut rule.conditions)?;
                if let Some(requirements) = &mut rule.requirements {
                    encoding.detach(requirements)?;
                }
                encoding.row(key(&rule, owner), &rule, owner)?;
            }
            metadata.packs.push(PackMetadata {
                retained_legacy: pack.document.retained_legacy,
                document: pack.document,
                digest: pack.digest,
                semantic: pack.semantic,
                profile,
                origins: pack.origins,
            });
        }
        if let Some(mut custom) = custom {
            encoding.patterns(std::mem::take(&mut custom.patterns))?;
            let time_sensitive = crate::policy::has_expiration(&custom);
            for mut rule in std::mem::take(&mut custom.document.rules) {
                encoding.detach(&mut rule.conditions)?;
                if let Some(requirements) = &mut rule.requirements {
                    encoding.detach(requirements)?;
                }
                encoding.row(
                    key(&rule, metadata.packs.len()),
                    &rule,
                    metadata.packs.len(),
                )?;
            }
            metadata.custom = Some(CustomMetadata {
                document: custom.document,
                digest: custom.digest,
                semantic: custom.semantic,
                time_sensitive,
            });
        }
        encoding.finish_images()?;
        let encoded = encoding.metadata(&metadata)?;
        let length = encoding.wire_length();
        let mut rows = encoding.rows;
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| Error::Limit)?;
        bytes.extend(MAGIC);
        number(&mut bytes, encoded.len())?;
        number(&mut bytes, rows.len())?;
        bytes.extend(encoded);
        let mut offset = bytes
            .len()
            .checked_add(rows.len() * ENTRY)
            .ok_or(Error::Limit)?;
        for (key, row, owner) in &rows {
            number(&mut bytes, offset)?;
            number(&mut bytes, key.len() + row.len())?;
            number(&mut bytes, key.len())?;
            number(&mut bytes, *owner)?;
            offset = offset
                .checked_add(key.len() + row.len())
                .ok_or(Error::Limit)?;
        }
        if offset > MAX_BYTES {
            return Err(Error::Limit);
        }
        if offset != length {
            return Err(Error::Syntax);
        }
        for (key, row, _) in rows {
            bytes.extend(key);
            bytes.extend(row);
        }
        // These postings and matcher images were encoded above from validated
        // typed owners. Keep structural/metadata admission without decoding and
        // validating every just-encoded rule and matcher a second time.
        let result = Self::load_authenticated(bytes)?;
        result.validate_rows(false)?;
        Ok(result)
    }
    #[cfg(test)]
    pub(crate) fn load(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::load_inner(bytes, true)
    }
    pub(crate) fn load_authenticated(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::load_inner(bytes, false)
    }
    fn load_inner(bytes: Vec<u8>, full: bool) -> Result<Self, Error> {
        if bytes.len() < HEADER
            || bytes.len() > MAX_BYTES
            || bytes.get(..MAGIC.len()) != Some(MAGIC)
        {
            return Err(Error::Contract("invalid compiled policy format"));
        }
        let metadata_len = read_number(&bytes, 8)?;
        let count = read_number(&bytes, 12)?;
        if metadata_len > 4 * 1024 * 1024 || count > MAX_ROWS {
            return Err(Error::Limit);
        }
        let table = HEADER.checked_add(metadata_len).ok_or(Error::Limit)?;
        let body = table.checked_add(count * ENTRY).ok_or(Error::Limit)?;
        if body > bytes.len() {
            return Err(Error::Contract("truncated compiled plan"));
        }
        let mut metadata: Metadata =
            serde_json::from_slice(&bytes[HEADER..table]).map_err(|_| Error::Syntax)?;
        for pack in &mut metadata.packs {
            pack.document.retained_legacy = pack.retained_legacy;
        }
        if metadata.engine != env!("CARGO_PKG_VERSION")
            || metadata.packs.len() > 64
            || metadata.packs.iter().any(|p| !p.document.rules.is_empty())
            || metadata
                .custom
                .as_ref()
                .is_some_and(|c| !c.document.rules.is_empty())
        {
            return Err(Error::Contract("incompatible compiled metadata"));
        }
        metadata.validate()?;
        let directory_options = crate::policy::rules::directories::combine(
            metadata
                .packs
                .iter()
                .map(|pack| {
                    (
                        crate::policy::rules::directories::Source::Pack(&pack.document.id),
                        pack.document.directory_options.as_slice(),
                    )
                })
                .chain(metadata.custom.iter().map(|custom| {
                    (
                        crate::policy::rules::directories::Source::Project,
                        custom.document.directory_options.as_slice(),
                    )
                })),
        )?;
        let custom = metadata.custom.as_ref().map(|c| CustomRules {
            comparison_sources: None,
            document: model::CustomDocument::empty(),
            digest: c.digest.clone(),
            semantic: c.semantic,
            historical_semantic: metadata.historical_semantic,
            index: index::Index::new(std::iter::empty()),
            patterns: Arc::default(),
        });
        let result = Self {
            bytes,
            metadata,
            table,
            count,
            custom,
            programs: Mutex::default(),
            directory_options,
        };
        if full {
            result.validate_rows(true)?;
        }
        Ok(result)
    }

    /// Constructed images still check every encoded boundary, owner and order.
    /// Only untrusted images need to decode and validate the source records again.
    fn validate_rows(&self, full: bool) -> Result<(), Error> {
        crate::diagnostics::trace::count("policy.layout_checks", self.count as u64);
        if full {
            crate::diagnostics::trace::count("policy.posting_validations", self.count as u64);
        }
        let mut expected = self.table + self.count * ENTRY;
        let mut previous: &[u8] = &[];
        let mut historical_semantic = false;
        for position in 0..self.count {
            let entry = self.entry(position)?;
            let key = &self.bytes[entry.start..entry.key_end];
            if entry.start != expected || key < previous || std::str::from_utf8(&key[1..]).is_err()
            {
                return Err(Error::Contract("invalid compiled posting order"));
            }
            if full {
                let row = &self.bytes[entry.key_end..entry.end];
                if entry.owner == PATTERN_OWNER {
                    if key.get(1..).is_some_and(|k| k.starts_with(b"graphql:")) {
                        let model: serde_json::Value =
                            serde_json::from_slice(row).map_err(|_| Error::Syntax)?;
                        crate::graphql::validate_model(&model)?;
                    } else {
                        patterns::load(row)?;
                    }
                } else {
                    let mut rule: Rule = serde_json::from_slice(row).map_err(|_| Error::Syntax)?;
                    crate::graphql::descriptor(&mut rule.conditions)?;
                    if let Some(requirements) = &mut rule.requirements {
                        crate::graphql::descriptor(requirements)?;
                    }
                    rule.validate_for(entry.owner < self.metadata.packs.len())?;
                    historical_semantic |= (rule.legacy.is_some()
                        || rule.legacy_normalized.is_some())
                        && rule.semantic();
                    if let Some(pack) = self.metadata.packs.get(entry.owner) {
                        crate::policy::resolution::invocations::binding(&pack.document, &rule)?;
                    }
                    // Shared-body keys also carry dispatch bindings, so each
                    // must equal its rule's own key.
                    if self::key(&rule, entry.owner) != key {
                        return Err(Error::Contract("unsound compiled scope key"));
                    }
                }
            }
            expected = entry.end;
            previous = key;
        }
        if expected != self.bytes.len() {
            return Err(Error::Contract("trailing compiled data"));
        }
        if full && historical_semantic != self.metadata.historical_semantic {
            return Err(Error::Contract("inconsistent historical semantic coverage"));
        }
        Ok(())
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn effective(&self) -> &serde_json::Value {
        &self.metadata.effective
    }
    pub(crate) fn codex_prefixes(&self) -> &[crate::policy::rules::agent_outputs::CodexPrefix] {
        &self.metadata.codex_prefixes
    }
    pub(crate) fn directory_options(
        &self,
    ) -> &[crate::policy::rules::directories::DirectoryOption] {
        &self.directory_options
    }
    pub(crate) fn time_sensitive(&self) -> bool {
        self.metadata
            .custom
            .as_ref()
            .is_some_and(|c| c.time_sensitive)
    }
    pub(crate) fn semantic(&self) -> bool {
        self.metadata.packs.iter().any(|p| p.semantic)
            || self.metadata.custom.as_ref().is_some_and(|c| c.semantic)
    }
    #[cfg(test)]
    pub(crate) fn evaluate(
        &self,
        request: &Request,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        known_cwd: bool,
        known_arguments: bool,
    ) -> Result<crate::policy::Evaluation, Error> {
        let input = normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: serde_json::json!({}),
        };
        let context = normalized::Context {
            working_files: None,
            cwd: request.cwd.clone(),
            environment: None,
        };
        let now = crate::evidence::now();
        let facts = Facts {
            input: &input,
            context: &context,
            executable: None,
            now: &now,
            known_cwd,
            known_arguments,
            locations: Some(&crate::policy::rules::resources::Unavailable),
            container_read: None,
            effects: None,
        };
        self.evaluate_input(request, &facts, checkpoint)
    }
    pub(crate) fn evaluate_input(
        &self,
        request: &Request,
        facts: &Facts<'_>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<crate::policy::Evaluation, Error> {
        self.evaluate_input_mcp(request, facts, None, checkpoint)
    }

    pub(crate) fn evaluate_input_mcp(
        &self,
        request: &Request,
        facts: &Facts<'_>,
        mcp: Option<&ResolvedMcp>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<crate::policy::Evaluation, Error> {
        request.validate()?;
        let mut packs = self
            .metadata
            .packs
            .iter()
            .map(|p| {
                (
                    Pack {
                        comparison_source: None,
                        document: p.document.descriptor(|gate| gate.candidate(facts)),
                        digest: p.digest.clone(),
                        semantic: p.semantic,
                        // These descriptors contain only query candidates; retain
                        // the full selection's historical coverage obligation.
                        historical_semantic: self.metadata.historical_semantic,
                        index: index::Index::new(std::iter::empty()),
                        patterns: Arc::default(),
                        resolution: Vec::new(),
                        origins: p.origins.clone(),
                    },
                    p.profile,
                )
            })
            .collect::<Vec<_>>();
        let mut custom = self.custom.as_ref().map(|c| CustomRules {
            comparison_sources: None,
            document: model::CustomDocument::empty(),
            digest: c.digest.clone(),
            semantic: c.semantic,
            historical_semantic: c.historical_semantic,
            index: index::Index::new(std::iter::empty()),
            patterns: Arc::default(),
        });
        let argv = matches!(facts.input, normalized::Input::Argv { .. });
        if argv || crate::policy::rules::legacy_read(request, facts) {
            if argv {
                self.collect_invocations(request, &mut packs, checkpoint)?;
            }
            let mut q = query(0, &request.executable);
            for length in 0..=request.arguments.len() {
                self.collect(&q, &mut packs, &mut custom, checkpoint, None)?;
                if let Some(arg) = request.arguments.get(length) {
                    q.extend(arg.as_bytes());
                    q.push(0);
                }
            }
            q[0] = 1;
            self.collect(&q, &mut packs, &mut custom, checkpoint, None)?;
        }
        let domain = match facts.input {
            normalized::Input::Argv { .. } => "argv",
            normalized::Input::Shell { .. } => "shell",
            normalized::Input::File { .. } => "file",
            normalized::Input::Web { .. } => "web",
            normalized::Input::Mcp { .. } => "mcp",
            normalized::Input::Unknown { .. } => "unknown",
        };
        for domain in [domain, "exact_request"] {
            self.collect(
                &query(8, domain),
                &mut packs,
                &mut custom,
                checkpoint,
                (domain == "argv").then_some(request.executable.as_str()),
            )?;
        }
        if matches!(facts.input, normalized::Input::Mcp { .. }) {
            let empty = BTreeMap::new();
            for (owner, (pack, _)) in packs.iter_mut().enumerate() {
                pack.bind_mcp_scopes(
                    mcp.and_then(|mcp| mcp.bindings.get(owner))
                        .unwrap_or(&empty),
                    self,
                )?;
            }
        }
        let selected: Vec<_> = packs.iter().map(|(p, profile)| (p, *profile)).collect();
        let customs: Vec<_> = custom.as_ref().into_iter().collect();
        let mut evaluation = crate::policy::evaluate_facts(
            &selected,
            &customs,
            request,
            facts,
            false,
            checkpoint,
            if facts.known_arguments && self.semantic() {
                crate::effects::classify(request)
            } else {
                None
            },
            Some(self),
        )?;
        self.constrain_historical_effects(request, facts, &mut evaluation);
        if let Some(observer) = facts.locations {
            observer.ensure_budget()?;
        }
        checkpoint()?;
        Ok(evaluation)
    }

    fn constrain_historical_effects(
        &self,
        request: &Request,
        facts: &Facts<'_>,
        evaluation: &mut crate::policy::Evaluation,
    ) {
        let argv = matches!(facts.input, normalized::Input::Argv { .. });
        if self.metadata.historical_semantic
            && (argv || crate::policy::rules::legacy_read(request, facts))
        {
            evaluation.semantic_coverage = Some("incomplete-possible-effects-v1");
            let exact = evaluation.matches.iter().any(|matched| {
                matched.exact_request
                    && matched.applied
                    && matched.decision == crate::policy::Decision::Allow
            });
            if !argv || !exact {
                evaluation.decision = evaluation.decision.max(crate::policy::Decision::Ask);
                if evaluation.decision == crate::policy::Decision::Ask {
                    evaluation.reason = "incomplete-effect-evidence";
                }
            }
        }
    }
    fn collect(
        &self,
        query: &[u8],
        packs: &mut [(Pack, Profile)],
        custom: &mut Option<CustomRules>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
        executable: Option<&str>,
    ) -> Result<(), Error> {
        for position in self.bound(query, false)?..self.bound(query, true)? {
            checkpoint()?;
            let entry = self.entry(position)?;
            let bytes = &self.bytes[entry.key_end..entry.end];
            if let Some(executable) = executable
                && !executable_candidate(bytes, executable)?
            {
                continue;
            }
            crate::diagnostics::trace::count("policy.collected_rules", 1);
            crate::diagnostics::trace::count(
                "policy.collected_bytes",
                (entry.end - entry.key_end) as u64,
            );
            let mut rule: Rule = serde_json::from_slice(bytes).map_err(|_| Error::Syntax)?;
            rule.prepare();
            if let Some((pack, _)) = packs.get_mut(entry.owner) {
                pack.document.rules.push(rule);
            } else {
                custom
                    .as_mut()
                    .ok_or(Error::Contract("missing custom owner"))?
                    .document
                    .rules
                    .push(rule);
            }
        }
        Ok(())
    }
    fn entry(&self, position: usize) -> Result<Entry, Error> {
        if position >= self.count {
            return Err(Error::Syntax);
        }
        let at = self.table + position * ENTRY;
        let start = read_number(&self.bytes, at)?;
        let length = read_number(&self.bytes, at + 4)?;
        let key_len = read_number(&self.bytes, at + 8)?;
        let owner = read_number(&self.bytes, at + 12)?;
        let end = start.checked_add(length).ok_or(Error::Limit)?;
        let key_end = start.checked_add(key_len).ok_or(Error::Limit)?;
        if start < self.table + self.count * ENTRY
            || end > self.bytes.len()
            || key_len < 3
            || key_end >= end
            || key_len > 65536
            || self.bytes.get(key_end - 1) != Some(&0)
        {
            return Err(Error::Contract("invalid compiled posting bounds"));
        }
        let kind = self.bytes[start];
        if (owner == PATTERN_OWNER && (kind != 9 || length > patterns::MAX_PATTERN_BYTES + 65536))
            || (owner != PATTERN_OWNER
                && (kind > 8
                    || length > 2 * 1024 * 1024
                    || owner > self.metadata.packs.len()
                    || (owner == self.metadata.packs.len() && self.metadata.custom.is_none())))
        {
            return Err(Error::Contract("invalid compiled posting owner"));
        }
        Ok(Entry {
            start,
            end,
            key_end,
            owner,
        })
    }
    fn bound(&self, query: &[u8], inclusive: bool) -> Result<usize, Error> {
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            let entry = self.entry(mid)?;
            let key = &self.bytes[entry.start..entry.key_end];
            if key < query || (inclusive && key == query) {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(low)
    }
}
impl Plan {
    fn program(&self, key: &str) -> Result<Arc<PikeVM>, Error> {
        let existing = self
            .programs
            .lock()
            .map_err(|_| Error::Contract("pattern cache unavailable"))?
            .items
            .get(key)
            .cloned();
        let vm = if let Some(Prepared::Pattern(vm)) = existing {
            vm
        } else {
            let q = query(9, key);
            let entry = self.entry(self.bound(&q, false)?)?;
            if &self.bytes[entry.start..entry.key_end] != q.as_slice()
                || entry.owner != PATTERN_OWNER
            {
                return Err(Error::Contract("missing prepared pattern"));
            }
            let vm = Arc::new(patterns::load(&self.bytes[entry.key_end..entry.end])?);
            let mut cache = self
                .programs
                .lock()
                .map_err(|_| Error::Contract("pattern cache unavailable"))?;
            if let Some(Prepared::Pattern(existing)) = cache.items.get(key) {
                return Ok(existing.clone());
            }
            let bytes = cache
                .bytes
                .checked_add(vm.get_nfa().memory_usage())
                .ok_or(Error::Limit)?;
            if bytes > patterns::MAX_PREPARED_BYTES {
                return Err(Error::Limit);
            }
            cache.bytes = bytes;
            cache
                .items
                .insert(key.into(), Prepared::Pattern(vm.clone()));
            vm
        };
        Ok(vm)
    }
}
impl Provider for Plan {
    fn model(&self, key: &str) -> Result<Arc<serde_json::Value>, Error> {
        if !key.starts_with("graphql:") {
            return Err(Error::Contract("invalid GraphQL model key"));
        }
        if let Some(Prepared::Graphql(model)) = self
            .programs
            .lock()
            .map_err(|_| Error::Contract("prepared cache unavailable"))?
            .items
            .get(key)
        {
            return Ok(model.clone());
        }
        let q = query(9, key);
        let entry = self.entry(self.bound(&q, false)?)?;
        if self.bytes[entry.start..entry.key_end] != q || entry.owner != PATTERN_OWNER {
            return Err(Error::Contract("missing prepared GraphQL model"));
        }
        let row = &self.bytes[entry.key_end..entry.end];
        let model: Arc<serde_json::Value> =
            Arc::new(serde_json::from_slice(row).map_err(|_| Error::Syntax)?);
        let mut cache = self
            .programs
            .lock()
            .map_err(|_| Error::Contract("prepared cache unavailable"))?;
        if let Some(Prepared::Graphql(existing)) = cache.items.get(key) {
            return Ok(existing.clone());
        }
        let bytes = cache
            .bytes
            .checked_add(row.len().checked_mul(16).ok_or(Error::Limit)?)
            .ok_or(Error::Limit)?;
        if bytes > patterns::MAX_PREPARED_BYTES {
            return Err(Error::Limit);
        }
        cache.bytes = bytes;
        cache
            .items
            .insert(key.into(), Prepared::Graphql(model.clone()));
        crate::graphql::record("graphql.model_loaded");
        Ok(model)
    }
    fn matches(&self, key: &str, value: &str) -> Result<bool, Error> {
        patterns::run(self.program(key)?.as_ref(), value)
    }
    fn work(&self, key: &str, value: &str) -> Result<usize, Error> {
        patterns::work(self.program(key)?.as_ref(), value)
    }
}

fn query(kind: u8, name: &str) -> Vec<u8> {
    let mut q = vec![kind];
    q.extend(name.as_bytes());
    q.push(0);
    q
}
fn key(rule: &Rule, owner: usize) -> Vec<u8> {
    invocations::key(rule, scope_key(rule), owner)
}
fn scope_key(rule: &Rule) -> Vec<u8> {
    let c = &rule.conditions;
    if c["domain"] == "argv"
        && let Some(executable) = selectors::exact(&c["executable"])
    {
        let arguments = &c["arguments"];
        let kind = arguments["kind"].as_str().unwrap_or("any");
        let prefix: Vec<_> =
            if ["exact", "prefix"].contains(&kind) && arguments.get("leading").is_none() {
                arguments["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map_while(selectors::exact)
                    .collect()
            } else if ["sequence", "options", "risk_tokens"].contains(&kind)
                && arguments.get("leading").is_none()
            {
                arguments["prefix"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map_while(selectors::exact)
                    .collect()
            } else {
                vec![]
            };
        let exact = kind == "exact"
            && arguments["values"]
                .as_array()
                .is_some_and(|v| v.len() == prefix.len());
        // Location identity is observed per request. Its source spelling cannot
        // prune a rule before aliases and symlinks have been resolved.
        let length = executable.len().checked_add(2).and_then(|length| {
            prefix.iter().try_fold(length, |sum, value| {
                sum.checked_add(value.len())?.checked_add(1)
            })
        });
        if length.is_none_or(|length| length > 65_536) {
            return query(8, "argv");
        }
        let mut key = Vec::with_capacity(length.unwrap_or_default());
        key.push(u8::from(exact));
        key.extend(executable.as_bytes());
        key.push(0);
        for argument in prefix {
            key.extend(argument.as_bytes());
            key.push(0);
        }
        if key.len() > 65_536 {
            query(8, "argv")
        } else {
            key
        }
    } else {
        query(8, c["domain"].as_str().unwrap_or_default())
    }
}
/// Broad postings include finite executable choices. Decode only their small
/// selector before allocating the complete rule and its argument grammar.
/// Every unrecognized selector stays a candidate for the ordinary resolver.
fn executable_candidate(bytes: &[u8], executable: &str) -> Result<bool, Error> {
    #[derive(Deserialize)]
    struct Row {
        conditions: Conditions,
    }
    #[derive(Deserialize)]
    struct Conditions {
        #[serde(default)]
        domain: Option<String>,
        #[serde(default)]
        executable: serde_json::Value,
    }
    let row: Row = serde_json::from_slice(bytes).map_err(|_| Error::Syntax)?;
    if row.conditions.domain.as_deref() != Some("argv") {
        return Ok(true);
    }
    let selector = &row.conditions.executable;
    if let Some(literal) = selector.as_str() {
        return Ok(literal == executable);
    }
    let Some(fields) = selector.as_object().filter(|fields| fields.len() == 1) else {
        return Ok(true);
    };
    if let Some(literal) = fields.get("exact").and_then(serde_json::Value::as_str) {
        return Ok(literal == executable);
    }
    if let Some(choices) = fields.get("one_of").and_then(serde_json::Value::as_array)
        && choices.iter().all(serde_json::Value::is_string)
    {
        return Ok(choices
            .iter()
            .any(|choice| choice.as_str() == Some(executable)));
    }
    Ok(true)
}

fn number(bytes: &mut Vec<u8>, value: usize) -> Result<(), Error> {
    bytes.extend(
        u32::try_from(value)
            .map_err(|_| Error::Limit)?
            .to_le_bytes(),
    );
    Ok(())
}
fn read_number(bytes: &[u8], at: usize) -> Result<usize, Error> {
    let value = bytes
        .get(at..at.checked_add(4).ok_or(Error::Limit)?)
        .ok_or(Error::Syntax)?;
    Ok(u32::from_le_bytes(value.try_into().map_err(|_| Error::Syntax)?) as usize)
}
#[cfg(test)]
mod tests;
