//! Bounded YAML sources, deterministic overlays and explicit project-file writes.
use super::*;
use canonic_agent_kit::document::{GIT_CONFLICT_MESSAGE, git_conflict_line};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
};

pub(crate) fn yaml(text: &str, file: &Path, local: bool) -> Result<Value> {
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        strict_booleans: true, reject_unsupported_tags: true, with_snippet: false,
        budget: serde_saphyr::budget! { max_depth:32, max_documents:1, max_nodes:50_000, max_events:100_000, max_aliases:0, max_anchors:0, max_total_scalar_bytes:1_048_576 },
    };
    let value: Value = serde_saphyr::from_str_with_options(text, options).map_err(|error| {
        let (message, location) = git_conflict_line(text).map_or_else(
            || {
                (
                    "Invalid YAML syntax, duplicate key, alias, tag or document limit",
                    error
                        .location()
                        .map(|point| format!("{}:{}", point.line(), point.column())),
                )
            },
            |line| (GIT_CONFLICT_MESSAGE, Some(format!("{line}:1"))),
        );
        Error::new("CONFIG_INVALID", message)
            .details(json!({"file":file,"pointer":"","location":location}))
    })?;
    schema::validate_document(&value, local).map_err(|e| super::provenance::with_file(e, file))?;
    Ok(value)
}
fn read(path: &Path) -> Result<String> {
    let file = fs::File::open(path).map_err(|_| {
        Error::new("CONFIG_IO", "Cannot read configuration").details(json!({"file":path}))
    })?;
    let mut value = String::new();
    file.take(1_048_577)
        .read_to_string(&mut value)
        .map_err(|_| Error::new("CONFIG_IO", "Cannot read UTF-8 configuration"))?;
    if value.len() > 1_048_576 {
        return Err(Error::new("CONFIG_INVALID", "Configuration exceeds 1 MiB"));
    }
    Ok(value)
}
fn provenance(value: &Value, pointer: &str, source: &str, result: &mut BTreeMap<String, String>) {
    if let Some(map) = value.as_object() {
        for (key, value) in map {
            provenance(
                value,
                &format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1")),
                source,
                result,
            );
        }
    } else {
        result.insert(pointer.into(), source.into());
    }
}
fn merge(base: &mut Value, local: Value, pointer: &str) -> Result<()> {
    let parts: Vec<_> = pointer.split('/').skip(1).collect();
    let execution = matches!(
        parts.as_slice(),
        ["services", _, "execution"] | ["projects", _, "services", _, "execution"]
    );
    if execution
        && base.get("kind").is_some()
        && local.get("kind").is_some()
        && base["kind"] != local["kind"]
    {
        *base = local;
        return Ok(());
    }
    if let (Some(base), Some(local)) = (base.as_object_mut(), local.as_object()) {
        for (key, value) in local {
            if value.is_null() && (pointer.ends_with("/env")) {
                base.remove(key);
                continue;
            }
            if let Some(old) = base.get_mut(key) {
                let identity = match parts.as_slice() {
                    ["builds", _] => ["builder", "platform", "target_kind"].contains(&key.as_str()),
                    ["projects", _] => key == "kind",
                    ["projects", _, "profiles", _] => {
                        ["platform", "target_kind"].contains(&key.as_str())
                    }
                    ["projects", _, "profiles", _, "build"] => key == "builder",
                    _ => false,
                };
                if identity && old != value {
                    return Err(Error::new(
                        "CONFIG_INVALID",
                        "Local overlays cannot change project kind, builder, platform or target_kind",
                    )
                    .details(json!({"pointer":format!("{pointer}/{key}")})));
                }
                merge(old, value.clone(), &format!("{pointer}/{key}"))?;
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = local;
    }
    Ok(())
}
/// Default base files inside a configuration root, in selection order.
pub(crate) const DEFAULTS: [&str; 2] = ["testrun.yml", ".config/testrun.yml"];
/// The optional overlay's name beside whichever base file is selected.
pub(crate) const LOCAL: &str = "testrun.local.yml";

/// A configuration root and the one base file selected for it.
#[derive(Clone, Debug)]
pub(crate) struct Location {
    pub root: PathBuf,
    pub base: PathBuf,
    pub explicit: bool,
    /// Present default files that the selected base hides.
    pub shadowed: Vec<PathBuf>,
}
fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(
            Error::new("CONFIG_IO", "Cannot inspect configuration location")
                .details(json!({"file":path})),
        ),
    }
}
/// Selects `config`, else root `testrun.yml`, else `.config/testrun.yml`,
/// in the exact root only. A present root file wins even when invalid.
pub(crate) fn locate(root: &Path, config: Option<&Path>) -> Result<Location> {
    let root = root
        .canonicalize()
        .map_err(|_| Error::new("CONFIG_PATH", "Project directory does not exist"))?;
    let defaults = DEFAULTS.map(|name| root.join(name));
    let (base, explicit) = if let Some(config) = config {
        if !config.is_absolute() {
            return Err(Error::invalid("config must be an absolute file path"));
        }
        let base = config
            .canonicalize()
            .ok()
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                Error::new("CONFIG_IO", "Selected configuration file is unavailable")
                    .details(json!({"file":config}))
            })?;
        if base.file_name().is_some_and(|name| name == LOCAL) {
            return Err(
                Error::invalid("Select a base configuration file, not its local overlay")
                    .details(json!({"file":base})),
            );
        }
        (base, true)
    } else {
        let selected = if present(&defaults[0])? || !present(&defaults[1])? {
            defaults[0].clone()
        } else {
            defaults[1].clone()
        };
        (selected, false)
    };
    let mut shadowed = Vec::new();
    for path in defaults {
        if path != base && present(&path)? && path.canonicalize().map_or(true, |path| path != base)
        {
            shadowed.push(path);
        }
    }
    Ok(Location {
        root,
        base,
        explicit,
        shadowed,
    })
}
impl Location {
    pub(crate) fn local(&self) -> PathBuf {
        self.base.with_file_name(LOCAL)
    }
    /// A provenance label: root-relative when inside the root, otherwise absolute.
    fn label(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }
    pub(crate) fn describe(&self) -> Value {
        json!({"file":self.base,"local":self.local(),"explicit":self.explicit,"shadowed":self.shadowed})
    }
}
impl Loaded {
    pub(crate) fn describe(&self) -> Value {
        self.location.describe()
    }
}
#[cfg(test)]
pub(crate) fn load(root: &Path) -> Result<Loaded> {
    load_from(&locate(root, None)?)
}
/// Loads the selected base and the overlay beside it.
pub(crate) fn load_at(root: &Path, config: Option<&Path>) -> Result<Loaded> {
    load_from(&locate(root, config)?)
}
fn load_from(location: &Location) -> Result<Loaded> {
    let root = location.root.clone();
    let base_path = location.base.clone();
    if !location.explicit && !present(&base_path)? {
        return Err(Error::new(
            "CONFIG_IO",
            "No Testrun configuration; create testrun.yml or .config/testrun.yml, or select a file with config",
        )
        .details(json!({"file":base_path,"candidates":DEFAULTS.map(|name| root.join(name))})));
    }
    let text = read(&base_path)?;
    let mut base = yaml(&text, &base_path, false)?;
    let shared = base.clone();
    decode(base.clone()).map_err(|e| super::provenance::with_file(e, &base_path))?;
    let mut sources =
        json!([{ "file":base_path,"sha256":format!("{:x}",Sha256::digest(text.as_bytes())) }]);
    let mut origins = BTreeMap::new();
    provenance(&base, "", &location.label(&base_path), &mut origins);
    let local_path = location.local();
    match fs::symlink_metadata(&local_path) {
        Ok(_) => {
            let text = read(&local_path)?;
            let local = yaml(&text, &local_path, true)?;
            if local["version"] != base["version"] {
                return Err(Error::new(
                    "CONFIG_INVALID",
                    "Shared and local configuration versions must agree",
                )
                .details(json!({"file":local_path,"pointer":"/version"})));
            }
            provenance(&local, "", &location.label(&local_path), &mut origins);
            merge(&mut base, local, "")
                .map_err(|e| super::provenance::with_file(e, &local_path))?;
            sources.as_array_mut().unwrap().push(
                json!({"file":local_path,"sha256":format!("{:x}",Sha256::digest(text.as_bytes()))}),
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(Error::new(
                "CONFIG_IO",
                "Cannot inspect local configuration",
            ));
        }
    }
    origins.retain(|pointer, _| base.pointer(pointer).is_some());
    let document = decode(base.clone()).map_err(|e| {
        let file = e
            .details
            .as_ref()
            .and_then(|d| d["pointer"].as_str())
            .and_then(|pointer| origins.get(pointer))
            .map(|file| root.join(file))
            // The shared document was validated before merging. A semantic
            // failure without a leaf pointer can therefore only originate in
            // the local overlay when one was applied.
            .unwrap_or_else(|| {
                if sources.as_array().is_some_and(|values| values.len() > 1) {
                    local_path.clone()
                } else {
                    base_path.clone()
                }
            });
        super::provenance::with_file(e, file)
    })?;
    super::provenance::fields(
        &serde_json::to_value(&document).unwrap(),
        "",
        "default",
        &mut origins,
        false,
    );
    Ok(Loaded {
        root,
        location: location.clone(),
        document,
        sources,
        provenance: origins,
        value: base,
        base: shared,
    })
}
pub(crate) fn inspect(root: &Path, config: Option<&Path>) -> Result<Value> {
    let loaded = load_at(root, config)?;
    if loaded.document.version == 2 {
        return projects::inspect(&loaded, None);
    }
    let mut configuration = serde_json::to_value(&loaded.document).unwrap();
    super::redact(&mut configuration);
    Ok(
        json!({"valid":true,"project":loaded.root,"location":loaded.describe(),"configuration":configuration,"sources":loaded.sources,"provenance":loaded.provenance}),
    )
}
/// The file an initializer creates in the canonical `root`: the explicit
/// file, with an existing parent resolved, or root `testrun.yml`.
pub(crate) fn target(root: &Path, config: Option<&Path>) -> Result<PathBuf> {
    let Some(config) = config else {
        return Ok(root.join(DEFAULTS[0]));
    };
    if !config.is_absolute() {
        return Err(Error::invalid("config must be an absolute file path"));
    }
    let name = config
        .file_name()
        .filter(|name| *name != LOCAL)
        .ok_or_else(|| Error::invalid("config must name a base configuration file"))?;
    // Canonicalize the nearest existing ancestor so a directory the
    // initializer will create still compares against the canonical root.
    let parent = config.parent().unwrap_or(root);
    let mut missing = Vec::new();
    let mut existing = parent;
    let base = loop {
        if let Ok(path) = existing.canonicalize() {
            break path;
        }
        match (existing.file_name(), existing.parent()) {
            (Some(component), Some(up)) => {
                missing.push(component);
                existing = up;
            }
            _ => break parent.to_path_buf(),
        }
    };
    Ok(missing
        .iter()
        .rev()
        .fold(base, |path, component| path.join(component))
        .join(name))
}
/// Where an initializer writes, refusing a base or overlay already present
/// at an explicit destination or in either default location.
pub(crate) fn destination(root: &Path, config: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
    let root = root
        .canonicalize()
        .map_err(|_| Error::new("CONFIG_PATH", "Configuration root is unavailable"))?;
    if !root.is_dir() {
        return Err(Error::invalid("Configuration root must be a directory"));
    }
    let path = target(&root, config)?;
    let guarded = if config.is_some() {
        vec![path.clone()]
    } else {
        DEFAULTS.iter().map(|name| root.join(name)).collect()
    };
    for base in guarded {
        for file in [base.clone(), base.with_file_name(LOCAL)] {
            if present(&file)? {
                return Err(Error::new(
                    "CONFIG_EXISTS",
                    "Testrun configuration already exists; use config show or project list; init preserves existing files",
                )
                .details(json!({"file":file})));
            }
        }
    }
    Ok((root, path))
}
pub(crate) fn init(root: &Path, config: Option<&Path>) -> Result<Value> {
    let (root, path) = destination(root, config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| {
            Error::new("CONFIG_IO", "Cannot create the configuration directory")
                .details(json!({"file":path}))
        })?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| {
            Error::new(
                "CONFIG_EXISTS",
                "Cannot create the configuration file; existing files are preserved",
            )
            .details(json!({"file":path}))
        })?;
    file.write_all(b"version: 2\nprojects: {}\n").map_err(|_| {
        Error::new("CONFIG_IO", "Cannot write the configuration file").details(json!({"file":path}))
    })?;
    let ignore = ignore_local(&root, &path.with_file_name(LOCAL))
        .map_err(|e| e.details(json!({"created":path,"ignoreUpdated":false})))?;
    Ok(json!({"created":path,"ignoreUpdated":true,"ignore":ignore}))
}
/// The ignore file and entry that cover the overlay beside a base file:
/// the root's `.gitignore` for an overlay inside the root, else the
/// overlay directory's own `.gitignore`.
pub(crate) fn ignore_target(root: &Path, local: &Path) -> (PathBuf, String) {
    match local.strip_prefix(root) {
        Ok(relative) => (
            root.join(".gitignore"),
            format!(
                "/{}",
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            ),
        ),
        Err(_) => (local.with_file_name(".gitignore"), format!("/{LOCAL}")),
    }
}
pub(super) fn ignore_local(root: &Path, local: &Path) -> Result<Value> {
    let (ignore, entry) = ignore_target(root, local);
    if fs::symlink_metadata(&ignore).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(Error::new(
            "CONFIG_IO",
            "Refusing to update a symlinked .gitignore",
        ));
    }
    let before = match fs::read_to_string(&ignore) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err(Error::new("CONFIG_IO", "Cannot read .gitignore")),
    };
    // A bare name ignores the overlay in every directory; a nested path is
    // anchored with or without its leading slash.
    if !before
        .lines()
        .any(|line| [LOCAL, entry.as_str(), &entry[1..]].contains(&line.trim()))
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&ignore)
            .map_err(|_| Error::new("CONFIG_IO", "Cannot update .gitignore"))?;
        if !before.is_empty() && !before.ends_with('\n') {
            file.write_all(b"\n")
                .map_err(|_| Error::new("CONFIG_IO", "Cannot update .gitignore"))?;
        }
        file.write_all(format!("{entry}\n").as_bytes())
            .map_err(|_| Error::new("CONFIG_IO", "Cannot update .gitignore"))?;
    }
    Ok(json!({"file":ignore,"entry":entry}))
}
