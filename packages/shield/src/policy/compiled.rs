//! Version-six scope postings and shared invocation dispatch; authority remains in the resolver.
mod invocations;
use super::{
    CustomRules, Error, Pack, Profile, Request, index, model, normalized,
    patterns::{self, Provider},
    rules::{Facts, Rule},
    selectors,
};
use regex_automata::nfa::thompson::pikevm::PikeVM;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
/// The plan format this engine writes and loads. Change it whenever compiled
/// bytes would decide differently, such as how patterns are prepared, so
/// plans written before the change are rebuilt instead of loaded.
pub(crate) const FORMAT: &str = "SHPLAN08";
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
    /// Codex prefixes of active rules that allow, in owner order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    codex_prefixes: Vec<super::agent_outputs::CodexPrefix>,
}
impl Metadata {
    fn validate(&self) -> Result<(), Error> {
        for pack in &self.packs {
            pack.document.validate_schema()?;
            super::invocations::definitions(&pack.document)?;
            for precondition in &pack.document.preconditions {
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
    digest: String,
    semantic: bool,
    profile: Profile,
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
    directory_options: Vec<super::directories::DirectoryOption>,
}
/// Codex prefixes of every pack rule under its profile, then of custom rules,
/// which select as Recommended.
fn codex_prefixes(
    packs: &[(Pack, Profile)],
    custom: Option<&CustomRules>,
) -> Vec<super::agent_outputs::CodexPrefix> {
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
            super::agent_outputs::codex_prefixes(rule, owner, profile)
        })
        .collect()
}
#[derive(Default)]
struct Programs {
    items: BTreeMap<String, Arc<PikeVM>>,
    bytes: usize,
}
impl Plan {
    pub(crate) fn compile(
        packs: Vec<(Pack, Profile)>,
        custom: Option<CustomRules>,
        effective: serde_json::Value,
    ) -> Result<Self, Error> {
        let mut rows = Vec::new();
        let mut images = BTreeMap::new();
        let mut metadata = Metadata {
            engine: env!("CARGO_PKG_VERSION").into(),
            packs: vec![],
            custom: None,
            effective,
            codex_prefixes: codex_prefixes(&packs, custom.as_ref()),
        };
        for (owner, (mut pack, profile)) in packs.into_iter().enumerate() {
            images.extend(
                pack.patterns
                    .images
                    .iter()
                    .map(|(key, bytes)| (key.clone(), bytes.clone())),
            );
            for rule in std::mem::take(&mut pack.document.rules) {
                rows.push((key(&rule, owner), encode(&rule)?, owner));
            }
            metadata.packs.push(PackMetadata {
                document: pack.document,
                digest: pack.digest,
                semantic: pack.semantic,
                profile,
            });
        }
        if let Some(mut custom) = custom {
            images.extend(
                custom
                    .patterns
                    .images
                    .iter()
                    .map(|(key, bytes)| (key.clone(), bytes.clone())),
            );
            let time_sensitive = super::has_expiration(&custom);
            for rule in std::mem::take(&mut custom.document.rules) {
                rows.push((
                    key(&rule, metadata.packs.len()),
                    encode(&rule)?,
                    metadata.packs.len(),
                ));
            }
            metadata.custom = Some(CustomMetadata {
                document: custom.document,
                digest: custom.digest,
                semantic: custom.semantic,
                time_sensitive,
            });
        }
        if images.values().map(Vec::len).sum::<usize>() > patterns::MAX_PREPARED_BYTES {
            return Err(Error::Limit);
        }
        for (name, image) in images {
            rows.push((query(9, &name), image, PATTERN_OWNER));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let encoded = encode(&metadata)?;
        if encoded.len() > 4 * 1024 * 1024 || rows.len() > MAX_ROWS {
            return Err(Error::Limit);
        }
        let mut bytes = MAGIC.to_vec();
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
        for (key, row, _) in rows {
            bytes.extend(key);
            bytes.extend(row);
        }
        Self::load(bytes)
    }
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
        let metadata: Metadata =
            serde_json::from_slice(&bytes[HEADER..table]).map_err(|_| Error::Syntax)?;
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
        let directory_options = super::directories::combine(
            metadata
                .packs
                .iter()
                .map(|pack| {
                    (
                        super::directories::Source::Pack(&pack.document.id),
                        pack.document.directory_options.as_slice(),
                    )
                })
                .chain(metadata.custom.iter().map(|custom| {
                    (
                        super::directories::Source::Project,
                        custom.document.directory_options.as_slice(),
                    )
                })),
        )?;
        let custom = metadata.custom.as_ref().map(|c| CustomRules {
            document: model::CustomDocument::empty(),
            digest: c.digest.clone(),
            semantic: c.semantic,
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
            let mut expected = body;
            let mut previous: &[u8] = &[];
            for position in 0..count {
                let entry = result.entry(position)?;
                let key = &result.bytes[entry.start..entry.key_end];
                if entry.start != expected
                    || key < previous
                    || std::str::from_utf8(&key[1..]).is_err()
                {
                    return Err(Error::Contract("invalid compiled posting order"));
                }
                let row = &result.bytes[entry.key_end..entry.end];
                if entry.owner == PATTERN_OWNER {
                    patterns::load(row)?;
                } else {
                    let rule: Rule = serde_json::from_slice(row).map_err(|_| Error::Syntax)?;
                    rule.validate_for(entry.owner < result.metadata.packs.len())?;
                    if let Some(pack) = result.metadata.packs.get(entry.owner) {
                        super::invocations::binding(&pack.document, &rule)?;
                    }
                    // Shared-body keys also carry dispatch bindings, so each
                    // must equal its rule's own key.
                    if self::key(&rule, entry.owner) != key {
                        return Err(Error::Contract("unsound compiled scope key"));
                    }
                }
                expected = entry.end;
                previous = key;
            }
            if expected != result.bytes.len() {
                return Err(Error::Contract("trailing compiled data"));
            }
        }
        Ok(result)
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn effective(&self) -> &serde_json::Value {
        &self.metadata.effective
    }
    pub(crate) fn codex_prefixes(&self) -> &[super::agent_outputs::CodexPrefix] {
        &self.metadata.codex_prefixes
    }
    pub(crate) fn directory_options(&self) -> &[super::directories::DirectoryOption] {
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
    ) -> Result<super::Evaluation, Error> {
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
            locations: Some(&super::resources::Unavailable),
            effects: None,
        };
        self.evaluate_input(request, &facts, checkpoint)
    }
    pub(crate) fn evaluate_input(
        &self,
        request: &Request,
        facts: &Facts<'_>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<super::Evaluation, Error> {
        request.validate()?;
        let mut packs = self
            .metadata
            .packs
            .iter()
            .map(|p| {
                (
                    Pack {
                        document: p.document.descriptor(),
                        digest: p.digest.clone(),
                        semantic: p.semantic,
                        index: index::Index::new(std::iter::empty()),
                        patterns: Arc::default(),
                        resolution: Vec::new(),
                    },
                    p.profile,
                )
            })
            .collect::<Vec<_>>();
        let mut custom = self.custom.as_ref().map(|c| CustomRules {
            document: model::CustomDocument::empty(),
            digest: c.digest.clone(),
            semantic: c.semantic,
            index: index::Index::new(std::iter::empty()),
            patterns: Arc::default(),
        });
        if matches!(facts.input, normalized::Input::Argv { .. }) {
            self.collect_invocations(request, &mut packs, checkpoint)?;
            let mut q = query(0, &request.executable);
            for length in 0..=request.arguments.len() {
                self.collect(&q, &mut packs, &mut custom, checkpoint)?;
                if let Some(arg) = request.arguments.get(length) {
                    q.extend(arg.as_bytes());
                    q.push(0);
                }
            }
            q[0] = 1;
            self.collect(&q, &mut packs, &mut custom, checkpoint)?;
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
            self.collect(&query(8, domain), &mut packs, &mut custom, checkpoint)?;
        }
        let selected: Vec<_> = packs.iter().map(|(p, profile)| (p, *profile)).collect();
        let customs: Vec<_> = custom.as_ref().into_iter().collect();
        let evaluation = super::evaluate_facts(
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
        if let Some(observer) = facts.locations {
            observer.ensure_budget()?;
        }
        checkpoint()?;
        Ok(evaluation)
    }
    fn collect(
        &self,
        query: &[u8],
        packs: &mut [(Pack, Profile)],
        custom: &mut Option<CustomRules>,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<(), Error> {
        for position in self.bound(query, false)?..self.bound(query, true)? {
            checkpoint()?;
            let entry = self.entry(position)?;
            let rule: Rule = serde_json::from_slice(&self.bytes[entry.key_end..entry.end])
                .map_err(|_| Error::Syntax)?;
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
        let vm = if let Some(vm) = existing {
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
            if let Some(existing) = cache.items.get(key) {
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
            cache.items.insert(key.into(), vm.clone());
            vm
        };
        Ok(vm)
    }
}
impl Provider for Plan {
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
        let mut key = vec![u8::from(exact)];
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
fn encode(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value).map_err(|_| Error::Syntax)
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
