//! Immutable bundled and dynamic workflows, prepared manifests and directory
//! listings, and resource namespace ownership.

mod source;
#[cfg(test)]
mod tests;

use crate::{
    ReadResourceResult, Resource, ResourceCatalog, ResourceContents, json, resource,
    rpc::{self, RpcError},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

/// Skills extension identifier.
pub const EXTENSION: &str = "io.modelcontextprotocol/skills";
/// Maximum skills in a catalog, including unlisted entries.
pub const MAX_SKILLS: usize = 128;
/// Maximum files in one manifest.
pub const MAX_FILES: usize = 512;
/// Maximum aggregate raw bytes in one manifest.
pub const MAX_SKILL_BYTES: usize = 16 * 1024 * 1024;
/// Maximum prepared directories across static skill namespaces.
pub const MAX_DIRECTORIES: usize = 4096;
const ENTRY_BYTES: usize = 256 * 1024;
const PAGE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy)]
struct ByteBudgets {
    skill: usize,
    stored: usize,
    metadata: usize,
}

impl ByteBudgets {
    const DEFAULT: Self = Self {
        skill: MAX_SKILL_BYTES,
        stored: 64 * 1024 * 1024,
        metadata: 8 * 1024 * 1024,
    };
}

/// A supplied file. Paths are decoded relative names, never filesystem access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFile {
    /// Relative path, with `/` separators.
    pub path: String,
    /// Exact raw bytes.
    pub bytes: Arc<[u8]>,
    /// Whether these bytes must be valid UTF-8 and returned as text.
    pub text: bool,
    /// Optional media type; text/plain or application/octet-stream by default.
    pub mime_type: Option<String>,
}

impl SkillFile {
    /// Supplies a UTF-8 file; bytes are preserved by catalog construction.
    #[must_use]
    pub fn text(path: impl Into<String>, text: impl AsRef<str>) -> Self {
        Self {
            path: path.into(),
            bytes: Arc::from(text.as_ref().as_bytes()),
            text: true,
            mime_type: None,
        }
    }
    /// Supplies a binary file.
    #[must_use]
    pub fn binary(path: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            text: false,
            mime_type: None,
        }
    }
}

/// One skill's complete source file set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    /// Hierarchical root URI without a trailing slash or SKILL.md suffix.
    pub root: String,
    /// Complete supplied files, including exactly one root SKILL.md.
    pub files: Vec<SkillFile>,
    /// Whether skill/resource discovery enumerates this root. Not access control.
    pub listed: bool,
}

impl SkillDefinition {
    /// Creates an enumerated definition.
    #[must_use]
    pub fn new(root: impl Into<String>, files: Vec<SkillFile>) -> Self {
        Self {
            root: root.into(),
            files,
            listed: true,
        }
    }
}

/// A generated skill whose files the server's read handler serves on request.
///
/// Its entry publishes `"resources": "dynamic"` instead of digests. Every
/// `SKILL.md` the handler serves must carry the same frontmatter as `document`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSkill {
    /// Hierarchical root URI without a trailing slash or SKILL.md suffix.
    pub root: String,
    /// A `SKILL.md` whose frontmatter the entry publishes; its body is not served.
    pub document: Arc<[u8]>,
    /// Whether skill/resource discovery enumerates this root. Not access control.
    pub listed: bool,
}

impl DynamicSkill {
    /// Creates an enumerated dynamic skill.
    #[must_use]
    pub fn new(root: impl Into<String>, document: impl AsRef<str>) -> Self {
        Self {
            root: root.into(),
            document: Arc::from(document.as_ref().as_bytes()),
            listed: true,
        }
    }
}

/// Invalid source data or composition; no partial catalog is published.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    /// A particular published file failed validation; the typed cause is retained.
    #[error("invalid skill file {uri}: {source}")]
    File {
        /// Published URI, never a local filesystem path.
        uri: String,
        /// Specific validation failure.
        source: Box<SkillError>,
    },
    /// Invalid frontmatter or required fields.
    #[error("invalid skill source: {0}")]
    Source(&'static str),
    /// Invalid or ambiguous root or relative path.
    #[error("invalid skill address: {0}")]
    Address(String),
    /// Duplicate or conflicting supplied identity.
    #[error("duplicate or conflicting skill file: {0}")]
    Conflict(String),
    /// A nested manifest omits published content.
    #[error("incomplete enclosing skill manifest: {0}")]
    Incomplete(String),
    /// A storage or representation budget is exceeded.
    #[error("skill limit exceeded: {0}")]
    Limit(&'static str),
    /// A resource content primitive refused the file.
    #[error(transparent)]
    Resource(#[from] resource::ResourceError),
    /// A handler's read or directory listing for a dynamic skill is inconsistent.
    #[error("invalid dynamic skill reply: {0}")]
    Reply(&'static str),
}

/// Where a skill-owned address is answered.
pub(crate) enum Route {
    /// Library-held bytes.
    Bundled(BundledRead),
    /// The consumer's handler, inside a dynamic skill namespace.
    Consumer,
    /// Inside a static namespace but not published.
    Missing,
}

/// How a directory read is answered.
pub(crate) enum DirectoryRoute {
    /// A prepared static page.
    Page(Value),
    /// The consumer's handler, inside a dynamic skill namespace.
    Consumer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Directories {
    snapshot: String,
    /// Directory URI to its ordinal and prepared pages.
    index: HashMap<String, (usize, Vec<Value>)>,
}

/// A prepared, immutable file read. Clone shares the encoded content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledRead {
    /// Exact published URI.
    pub uri: String,
    result: Arc<ReadResourceResult>,
}

impl BundledRead {
    /// The validated read result, ready for bounded serialization.
    #[must_use]
    pub fn result(&self) -> &ReadResourceResult {
        &self.result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredFile {
    bytes: Arc<[u8]>,
    mime: String,
    text: bool,
    read: BundledRead,
    manifest: Value,
}

/// Validated static catalog; construction prepares hashes, entries and pages once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCatalog {
    roots: Vec<String>,
    root_index: HashSet<String>,
    dynamic_roots: HashSet<String>,
    /// Dynamic `SKILL.md` URIs and the frontmatter their entries publish.
    documents: HashMap<String, Value>,
    entries: HashMap<String, Value>,
    files: BTreeMap<String, StoredFile>,
    descriptors: Vec<Resource>,
    pages: Vec<Value>,
    snapshot: String,
    raw_bytes: usize,
    directories: Option<Directories>,
}

impl SkillCatalog {
    /// Builds the complete catalog atomically from exact supplied bytes.
    ///
    /// # Errors
    /// Returns [`SkillError`] for invalid source, namespace conflicts, incomplete
    /// nested manifests or any documented byte/count budget.
    pub fn new(definitions: Vec<SkillDefinition>) -> Result<Self, SkillError> {
        Self::with_budgets(definitions, &[], ByteBudgets::DEFAULT)
    }

    /// Builds a catalog of static skills followed by generated ones, listed in that order.
    ///
    /// A dynamic skill's namespace cannot contain or sit inside another skill root.
    ///
    /// # Errors
    /// Returns [`SkillError`] as [`Self::new`] does, and for an invalid dynamic
    /// document, root or nesting.
    pub fn with_dynamic(
        definitions: Vec<SkillDefinition>,
        dynamic: &[DynamicSkill],
    ) -> Result<Self, SkillError> {
        Self::with_budgets(definitions, dynamic, ByteBudgets::DEFAULT)
    }

    fn with_budgets(
        definitions: Vec<SkillDefinition>,
        dynamic: &[DynamicSkill],
        budgets: ByteBudgets,
    ) -> Result<Self, SkillError> {
        if definitions.len() + dynamic.len() > MAX_SKILLS {
            return Err(SkillError::Limit("skill count"));
        }
        let mut catalog = Self {
            roots: Vec::new(),
            root_index: HashSet::new(),
            dynamic_roots: HashSet::new(),
            documents: HashMap::new(),
            entries: HashMap::new(),
            files: BTreeMap::new(),
            descriptors: Vec::new(),
            pages: Vec::new(),
            snapshot: String::new(),
            raw_bytes: 0,
            directories: None,
        };
        let mut listed = Vec::new();
        let mut memberships = Vec::new();
        let mut metadata_bytes = 0;
        let mut measure = |entry: &Value| {
            metadata_bytes += json::measure(entry, ENTRY_BYTES, &|| Ok(()))
                .map_err(|_| SkillError::Limit("entry bytes"))?;
            if metadata_bytes > budgets.metadata {
                return Err(SkillError::Limit("catalog metadata bytes"));
            }
            Ok(())
        };
        for definition in definitions {
            let (entry, members) = catalog.insert(definition, budgets)?;
            measure(&entry)?;
            if members.1 {
                listed.push(entry);
            }
            memberships.push(members.0);
        }
        for skill in dynamic {
            let (entry, enumerated) = catalog.insert_dynamic(skill)?;
            measure(&entry)?;
            if enumerated {
                listed.push(entry);
            }
        }
        for (root, members) in catalog.roots.iter().zip(memberships) {
            if catalog
                .files
                .keys()
                .any(|uri| under(uri, root) && !members.contains(uri))
            {
                return Err(SkillError::Incomplete(root.clone()));
            }
        }
        if let Some(nested) = catalog.dynamic_roots.iter().find(|dynamic| {
            catalog
                .root_index
                .iter()
                .any(|root| under(root, dynamic) || under(dynamic, root))
        }) {
            return Err(SkillError::Conflict(nested.clone()));
        }
        let fingerprint = json::encode(&listed, budgets.metadata + 1024)
            .map_err(|_| SkillError::Limit("catalog metadata bytes"))?;
        catalog.snapshot = digest(&fingerprint);
        catalog.prepare_pages(listed)?;
        Ok(catalog)
    }

    fn insert(
        &mut self,
        definition: SkillDefinition,
        budgets: ByteBudgets,
    ) -> Result<(Value, (HashSet<String>, bool)), SkillError> {
        if definition.files.is_empty() || definition.files.len() > MAX_FILES {
            return Err(SkillError::Limit("files per skill"));
        }
        if definition
            .files
            .iter()
            .any(|file| file.bytes.len() > resource::MAX_BYTES)
        {
            return Err(SkillError::Limit("file raw bytes"));
        }
        let root = canonical_root(&definition.root)?;
        let uri = format!("{root}/SKILL.md");
        if self.entries.contains_key(&uri) {
            return Err(SkillError::Conflict(uri));
        }
        let document = definition
            .files
            .iter()
            .find(|file| file.path == "SKILL.md")
            .ok_or(SkillError::Source("missing SKILL.md"))?;
        let frontmatter =
            source::frontmatter(&document.bytes).map_err(|source| SkillError::File {
                uri: uri.clone(),
                source: Box::new(source),
            })?;
        let name = named(&root, &frontmatter)?;
        let mut members = HashSet::new();
        let mut manifest = BTreeMap::new();
        let mut total = 0;
        for file in definition.files {
            total += file.bytes.len();
            if total > budgets.skill {
                return Err(SkillError::Limit("skill raw bytes"));
            }
            let file_uri = file_uri(&root, &file.path)?;
            if !members.insert(file_uri.clone()) {
                return Err(SkillError::Conflict(file_uri));
            }
            let stored = if let Some(previous) = self.files.get(&file_uri) {
                let (text, mime) = representation(&file);
                if previous.bytes != file.bytes || previous.mime != mime || previous.text != text {
                    return Err(SkillError::Conflict(file_uri));
                }
                previous.clone()
            } else {
                if self.files.len() == 4096 {
                    return Err(SkillError::Limit("distinct files"));
                }
                self.raw_bytes += file.bytes.len();
                if self.raw_bytes > budgets.stored {
                    return Err(SkillError::Limit("stored bytes"));
                }
                let stored = prepare_file(file, &file_uri).map_err(|source| SkillError::File {
                    uri: file_uri.clone(),
                    source: Box::new(source),
                })?;
                self.files.insert(file_uri.clone(), stored.clone());
                stored
            };
            manifest.insert(file_uri, stored.manifest);
        }
        let entry = json!({"uri":uri,"frontmatter":frontmatter,"resources":manifest.into_values().collect::<Vec<_>>()});
        if definition.listed {
            let mut resource = Resource::new(&uri, name);
            resource.description = frontmatter["description"].as_str().map(str::to_owned);
            resource.mime_type = Some("text/markdown".into());
            resource.size = Some(document_size(&self.files, &uri));
            self.descriptors.push(resource);
        }
        self.root_index.insert(root.clone());
        self.roots.push(root);
        self.entries.insert(uri, entry.clone());
        Ok((entry, (members, definition.listed)))
    }

    fn insert_dynamic(&mut self, skill: &DynamicSkill) -> Result<(Value, bool), SkillError> {
        if skill.document.len() > resource::MAX_BYTES {
            return Err(SkillError::Limit("file raw bytes"));
        }
        let root = canonical_root(&skill.root)?;
        let uri = format!("{root}/SKILL.md");
        if self.entries.contains_key(&uri) {
            return Err(SkillError::Conflict(uri));
        }
        let frontmatter =
            source::frontmatter(&skill.document).map_err(|source| SkillError::File {
                uri: uri.clone(),
                source: Box::new(source),
            })?;
        let name = named(&root, &frontmatter)?;
        if skill.listed {
            let mut resource = Resource::new(&uri, name);
            resource.description = frontmatter["description"].as_str().map(str::to_owned);
            resource.mime_type = Some("text/markdown".into());
            self.descriptors.push(resource);
        }
        let entry = json!({"uri":uri,"frontmatter":frontmatter,"resources":"dynamic"});
        self.root_index.insert(root.clone());
        self.dynamic_roots.insert(root);
        self.documents.insert(uri.clone(), frontmatter);
        self.entries.insert(uri, entry.clone());
        Ok((entry, skill.listed))
    }

    /// Prepares `resources/directory/read` for every directory in the static
    /// skill namespaces and advertises `directoryRead`. Dynamic namespaces are
    /// listed by the server's handler.
    ///
    /// # Errors
    /// Returns [`SkillError::Limit`] above [`MAX_DIRECTORIES`] directories, and
    /// [`SkillError::Conflict`] when one path is both a file and a directory.
    pub fn with_directory_reads(mut self) -> Result<Self, SkillError> {
        let mut index: BTreeMap<String, BTreeMap<String, Resource>> = self
            .roots
            .iter()
            .map(|root| (root.clone(), BTreeMap::new()))
            .collect();
        for (uri, file) in &self.files {
            for root in self.roots.iter().filter(|root| under(uri, root)) {
                let mut parent = root.clone();
                let mut segments = uri[root.len() + 1..].split('/').peekable();
                while let Some(segment) = segments.next() {
                    let child = format!("{parent}/{segment}");
                    let mut resource = Resource::new(&child, decode(segment).unwrap_or_default());
                    if segments.peek().is_some() {
                        resource.mime_type = Some(resource::DIRECTORY_MIME_TYPE.into());
                    } else {
                        resource.mime_type = Some(file.mime.clone());
                        resource.size = Some(file.bytes.len() as u64);
                    }
                    let existing = index
                        .entry(parent)
                        .or_default()
                        .entry(child.clone())
                        .or_insert_with(|| resource.clone());
                    // A path cannot be both a file and a directory.
                    if *existing != resource {
                        return Err(SkillError::Conflict(child));
                    }
                    if index.len() > MAX_DIRECTORIES {
                        return Err(SkillError::Limit("directories"));
                    }
                    parent = child;
                }
            }
        }
        let mut hasher = Sha256::new();
        let mut prepared = HashMap::new();
        for (ordinal, (directory, children)) in index.into_iter().enumerate() {
            hasher.update(directory.as_bytes());
            for child in children.values() {
                hasher.update([0]);
                hasher.update(child.uri.as_bytes());
                hasher.update(child.mime_type.as_deref().unwrap_or_default().as_bytes());
                hasher.update(child.size.unwrap_or_default().to_le_bytes());
            }
            hasher.update([1]);
            let children: Vec<_> = children.into_values().collect();
            let mut pages: Vec<Value> = children
                .chunks(resource::PAGE_SIZE)
                .map(|page| json!({"resources":page}))
                .collect();
            if pages.is_empty() {
                pages.push(json!({"resources":[]}));
            }
            prepared.insert(directory, (ordinal, pages));
        }
        self.directories = Some(Directories {
            snapshot: format!("{:x}", hasher.finalize()),
            index: prepared,
        });
        Ok(self)
    }

    /// Whether `resources/directory/read` is served.
    #[must_use]
    pub fn directory_reads(&self) -> bool {
        self.directories.is_some()
    }

    pub(crate) fn dynamic_namespace(&self, uri: &str) -> bool {
        self.dynamic_roots.contains(uri)
            || uri
                .match_indices('/')
                .any(|(offset, _)| self.dynamic_roots.contains(&uri[..offset]))
    }

    /// Where a read of an owned address is answered; `None` outside skill namespaces.
    pub(crate) fn route(&self, uri: &str) -> Option<Route> {
        if !self.owns(uri) {
            return None;
        }
        Some(match self.read(uri) {
            Some(read) => Route::Bundled(read),
            None if self.dynamic_namespace(uri) => Route::Consumer,
            None => Route::Missing,
        })
    }

    /// Whether `uri` is a dynamic skill's `SKILL.md`, whose reads [`Self::check_read`] checks.
    #[must_use]
    pub fn dynamic_document(&self, uri: &str) -> bool {
        self.documents.contains_key(uri)
    }

    /// Checks a handler's read of a dynamic skill's `SKILL.md`: one text content
    /// at that URI whose frontmatter equals the published entry. Other reads pass.
    ///
    /// # Errors
    /// Returns [`SkillError::Reply`] when the served document would fail a host's
    /// frontmatter check.
    pub fn check_read(&self, uri: &str, result: &ReadResourceResult) -> Result<(), SkillError> {
        let Some(expected) = self.documents.get(uri) else {
            return Ok(());
        };
        let [content] = result.contents.as_slice() else {
            return Err(SkillError::Reply("SKILL.md needs exactly one content"));
        };
        let text = content
            .text_value()
            .filter(|_| content.uri() == uri)
            .ok_or(SkillError::Reply("SKILL.md must be text at its own URI"))?;
        match source::frontmatter(text.as_bytes()) {
            Ok(frontmatter) if frontmatter == *expected => Ok(()),
            Ok(_) => Err(SkillError::Reply(
                "SKILL.md frontmatter does not match its entry",
            )),
            Err(_) => Err(SkillError::Reply("SKILL.md frontmatter is invalid")),
        }
    }

    /// Checks a handler's directory listing inside a dynamic skill namespace:
    /// every child is a distinct direct child of `uri`.
    ///
    /// # Errors
    /// Returns [`SkillError::Reply`] outside a dynamic namespace or for a child
    /// that is not a single, distinct path segment below `uri`.
    pub fn check_directory(
        &self,
        uri: &str,
        result: &crate::ReadDirectoryResult,
    ) -> Result<(), SkillError> {
        if !self.dynamic_namespace(uri) {
            return Err(SkillError::Reply(
                "handlers list only dynamic skill directories",
            ));
        }
        let mut seen = HashSet::new();
        for child in result.resources() {
            let direct = child
                .uri
                .strip_prefix(uri)
                .and_then(|rest| rest.strip_prefix('/'))
                .and_then(decode)
                .is_some_and(|segment| {
                    !segment.is_empty()
                        && segment != "."
                        && segment != ".."
                        && !segment.contains(['/', '\\'])
                });
            if !direct || !seen.insert(child.uri.as_str()) {
                return Err(SkillError::Reply(
                    "listing entries must be distinct direct children",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn directory(&self, params: &Value) -> Result<DirectoryRoute, RpcError> {
        let invalid = || RpcError::new(rpc::INVALID_PARAMS, "Unknown or invalid directory");
        if !rpc::only(params, &["uri", "cursor", "_meta"]) {
            return Err(invalid());
        }
        let uri = params["uri"].as_str().ok_or_else(invalid)?;
        resource::validate_uri(uri).map_err(|_| invalid())?;
        let cursor = match params.get("cursor") {
            None => None,
            Some(cursor) => Some(
                cursor
                    .as_str()
                    .filter(|text| text.len() <= resource::MAX_CURSOR_BYTES)
                    .ok_or_else(invalid)?,
            ),
        };
        if params["_meta"]
            .get("progressToken")
            .is_some_and(|token| !(token.is_string() || token.is_i64() || token.is_u64()))
        {
            return Err(invalid());
        }
        if self.dynamic_namespace(uri) {
            return Ok(DirectoryRoute::Consumer);
        }
        let directories = self.directories.as_ref().ok_or_else(invalid)?;
        let (ordinal, pages) = directories.index.get(uri).ok_or_else(invalid)?;
        let prefix = format!("{}:{ordinal}:", directories.snapshot);
        let index = match cursor {
            None => 0,
            Some(cursor) => {
                let offset = cursor
                    .strip_prefix(&prefix)
                    .and_then(|offset| offset.parse::<usize>().ok())
                    .ok_or_else(invalid)?;
                if offset == 0 || offset >= pages.len() || cursor != format!("{prefix}{offset}") {
                    return Err(invalid());
                }
                offset
            }
        };
        let mut page = pages[index].clone();
        if index + 1 < pages.len() {
            page["nextCursor"] = json!(format!("{prefix}{}", index + 1));
        }
        Ok(DirectoryRoute::Page(page))
    }

    fn prepare_pages(&mut self, entries: Vec<Value>) -> Result<(), SkillError> {
        let mut page = Vec::new();
        // Reserve bounded server identity, cache fields, cursor and envelope metadata.
        let mut bytes = 32768;
        for entry in entries {
            let length = json::measure(&entry, ENTRY_BYTES, &|| Ok(()))
                .map_err(|_| SkillError::Limit("entry bytes"))?;
            if page.len() == 32 || bytes + length + 1 > PAGE_BYTES {
                self.pages.push(json!({"skills":page}));
                page = Vec::new();
                bytes = 32768;
            }
            bytes += length + 1;
            page.push(entry);
        }
        self.pages.push(json!({"skills":page}));
        Ok(())
    }

    pub(crate) fn compose(
        &self,
        resources: Option<&ResourceCatalog>,
    ) -> Result<ResourceCatalog, SkillError> {
        let (mut definitions, templates) =
            resources.map_or_else(|| (Vec::new(), Vec::new()), ResourceCatalog::definitions);
        if let Some(conflict) = definitions.iter().find(|resource| self.owns(&resource.uri)) {
            return Err(SkillError::Conflict(conflict.uri.clone()));
        }
        definitions.extend(self.descriptors.iter().cloned());
        Ok(ResourceCatalog::new(definitions, templates)?)
    }

    /// Whether the URI belongs to a reserved skill root namespace.
    #[must_use]
    pub fn owns(&self, uri: &str) -> bool {
        self.root_index.contains(uri)
            || uri
                .match_indices('/')
                .any(|(offset, _)| self.root_index.contains(&uri[..offset]))
    }

    /// Retrieves a prepared file without resolution or I/O.
    #[must_use]
    pub fn read(&self, uri: &str) -> Option<BundledRead> {
        self.files.get(uri).map(|file| file.read.clone())
    }

    pub(crate) fn query(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if method == "skills/get" {
            if !rpc::only(params, &["uri", "_meta"]) {
                return Err(invalid());
            }
            let uri = params["uri"].as_str().ok_or_else(invalid)?;
            resource::validate_uri(uri).map_err(|_| invalid())?;
            return self
                .entries
                .get(uri)
                .map(|entry| json!({"skill":entry}))
                .ok_or_else(invalid);
        }
        if !rpc::only(params, &["cursor", "_meta"]) {
            return Err(invalid());
        }
        let index = match params.get("cursor") {
            None => 0,
            Some(cursor) => {
                let cursor = cursor
                    .as_str()
                    .filter(|text| text.len() <= 256)
                    .ok_or_else(invalid)?;
                let offset = cursor
                    .strip_prefix(&format!("{}:", self.snapshot))
                    .ok_or_else(invalid)?;
                let offset = offset.parse::<usize>().map_err(|_| invalid())?;
                if offset == 0
                    || offset >= self.pages.len()
                    || cursor != format!("{}:{offset}", self.snapshot)
                {
                    return Err(invalid());
                }
                offset
            }
        };
        let mut page = self.pages[index].clone();
        if index + 1 < self.pages.len() {
            page["nextCursor"] = json!(format!("{}:{}", self.snapshot, index + 1));
        }
        Ok(page)
    }
}

fn named<'a>(root: &str, frontmatter: &'a Value) -> Result<&'a str, SkillError> {
    let name = frontmatter["name"]
        .as_str()
        .ok_or(SkillError::Source("missing name"))?;
    if root.rsplit('/').next() != Some(name) {
        return Err(SkillError::Address(root.into()));
    }
    Ok(name)
}

/// Decodes one percent-encoded URI path segment; `None` for malformed escapes or UTF-8.
fn decode(segment: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(segment.len());
    let mut input = segment.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hex = [input.next()?, input.next()?];
            bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn document_size(files: &BTreeMap<String, StoredFile>, uri: &str) -> u64 {
    files.get(uri).map_or(0, |file| file.bytes.len() as u64)
}
fn under(uri: &str, root: &str) -> bool {
    uri.strip_prefix(root)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
fn invalid() -> RpcError {
    RpcError::new(
        rpc::INVALID_PARAMS,
        "Unknown skill or invalid skill parameters",
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn prepare_file(file: SkillFile, uri: &str) -> Result<StoredFile, SkillError> {
    let (text, mime) = representation(&file);
    let content = if text {
        ResourceContents::text(
            uri,
            Some(mime.clone()),
            std::str::from_utf8(&file.bytes)
                .map_err(|_| SkillError::Source("invalid UTF-8 text file"))?,
        )?
    } else {
        ResourceContents::blob(uri, Some(mime.clone()), &file.bytes)?
    };
    let manifest = json!({"uri":uri,"digest":digest(&file.bytes),"size":file.bytes.len()});
    Ok(StoredFile {
        bytes: file.bytes,
        mime,
        text,
        read: BundledRead {
            uri: uri.into(),
            result: Arc::new(ReadResourceResult::new(vec![content])?),
        },
        manifest,
    })
}

fn representation(file: &SkillFile) -> (bool, String) {
    let document = file.path == "SKILL.md" || file.path.ends_with("/SKILL.md");
    let text = document || file.text;
    let mime = if document {
        "text/markdown".into()
    } else {
        file.mime_type.clone().unwrap_or_else(|| {
            if text {
                "text/plain".into()
            } else {
                "application/octet-stream".into()
            }
        })
    };
    (text, mime)
}

fn canonical_root(root: &str) -> Result<String, SkillError> {
    resource::validate_uri(root)?;
    if !root.contains("://") || root.ends_with('/') || root.contains(['?', '#', '\\']) {
        return Err(SkillError::Address(root.into()));
    }
    let (_, path) = root
        .split_once("://")
        .ok_or_else(|| SkillError::Address(root.into()))?;
    if path.is_empty()
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(SkillError::Address(root.into()));
    }
    let mut canonical = String::new();
    let mut chars = root.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            canonical.push(c);
            continue;
        }
        let a = chars
            .next()
            .ok_or_else(|| SkillError::Address(root.into()))?;
        let b = chars
            .next()
            .ok_or_else(|| SkillError::Address(root.into()))?;
        let byte = u8::from_str_radix(&format!("{a}{b}"), 16)
            .map_err(|_| SkillError::Address(root.into()))?;
        if matches!(byte, b'/' | b'\\') {
            return Err(SkillError::Address(root.into()));
        }
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            canonical.push(char::from(byte));
        } else {
            let _ = write!(canonical, "%{byte:02X}");
        }
    }
    if canonical.split('/').any(|part| part == "." || part == "..") {
        return Err(SkillError::Address(root.into()));
    }
    Ok(canonical)
}

fn file_uri(root: &str, path: &str) -> Result<String, SkillError> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\\')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(SkillError::Address(path.into()));
    }
    let mut uri = format!("{root}/");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    resource::validate_uri(&uri)?;
    Ok(uri)
}
