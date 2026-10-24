//! The source export of a space: each page's design and implementation entry
//! points, their transitive local imports and assets, the configuration that
//! builds them, a guide and content hash per page, the reference screenshots
//! beside each guide, and the portable viewer when one was built. Reads the
//! project's files; writes nothing.
use super::archive::{split_archives, zip};
use super::paths::{
    basename, display, extension, inside, iso_now, normalize, posix_basename, posix_dirname,
    posix_join, posix_relative, relative, resolve, short_hash, slug,
};
use super::references::{
    is_script, is_stylesheet, package_name, parse_references, stylesheet_imports,
};
use super::types::{
    Archive, BrowserPreview, BrowserReport, Dependency, ExportOptions, ExportReport, ExportResult,
    ExportView, PageEntry, PageReport, PageScreenshot, PortableExport, Screenshot, ZipEntry,
};
use crate::order::locale_compare;
use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const TEXT_EXTENSIONS: [&str; 11] = [
    ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".json", ".css", ".scss", ".sass", ".less",
];
const RESOLVE_EXTENSIONS: [&str; 21] = [
    "", ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".json", ".css", ".scss", ".sass", ".less",
    ".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".woff", ".woff2",
];
const OMIT_DIRS: [&str; 8] = [
    ".git",
    ".canonic",
    "node_modules",
    "dist",
    "build",
    "coverage",
    ".next",
    ".turbo",
];
const DEPENDENCY_GROUPS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];
const WORKBENCH_SDK: &str = "@canonic2/workbench";

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("source export pattern")
}

static OMIT_FILE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"(?i)(?:^|/)(?:\.env(?:\.|$)|.*\.(?:test|spec)\.[cm]?[jt]sx?|.*\.snap$)")
});
static CONFIG_FILES: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"^(?:package\.json|(?:ts|js)config(?:\.[^.]+)?\.json|vite\.config\.[cm]?[jt]s|tailwind\.config\.[cm]?[jt]s|postcss\.config\.[cm]?[jt]s)$",
    )
});
static EXTERNAL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(?:[a-z]+:|//|#)"));
static ALIAS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[@~]/"));
static LINKED: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(?:link|file):(.+)$"));
static VITE_CONFIG: LazyLock<Regex> = LazyLock::new(|| pattern(r"^vite\.config\.[cm]?[jt]s$"));
static ICON_DIRS: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r#"iconDirs\s*:\s*\[\s*path\.resolve\(\s*process\.cwd\(\)\s*,\s*["']([^"']+)["']\s*\)"#)
});
static STORYBOOK_SCRIPT: LazyLock<Regex> = LazyLock::new(|| pattern(r"\bstorybook\b"));
static CONFIG_DIR: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?:--config-dir|-c)(?:=|\s+)(?:"([^"]+)"|'([^']+)'|([^\s]+))"#));
static SCRIPT_FILE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\.[cm]?[jt]sx?$"));

/// Where a group of exported files comes from and goes in the archive.
#[derive(Debug, Clone)]
struct Owner {
    key: String,
    root: PathBuf,
    destination: String,
}

#[derive(Debug, Clone)]
struct SourceFile {
    source: PathBuf,
    destination: String,
    owner: usize,
}

#[derive(Debug, Clone)]
struct PackageInfo {
    manifest: PathBuf,
    root: PathBuf,
    package: Value,
}

struct PageData {
    id: String,
    label: String,
    direct: BTreeSet<PathBuf>,
    entries: Vec<PageEntry>,
}

fn is_config_file(name: &str) -> bool {
    CONFIG_FILES.is_match(name)
}

fn is_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

fn is_dir(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Names in a folder, sorted, or none when it can't be read.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    entry
                        .ok()
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn page_slugs(sources: &[String]) -> HashMap<String, String> {
    let mut sorted = sources.to_vec();
    sorted.sort();
    let mut used = BTreeSet::new();
    let mut out = HashMap::new();
    for source in sorted {
        let mut value = slug(Some(&source));
        if used.contains(&value) {
            value = format!("{value}-{}", short_hash(&source));
        }
        used.insert(value.clone());
        out.insert(source, value);
    }
    out
}

/// SHA-256 over each file's archive path and bytes, in path order.
fn content_hash(items: &[&SourceFile]) -> String {
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| a.destination.cmp(&b.destination));
    let mut hash = Sha256::new();
    for item in sorted {
        let body = fs::read(&item.source).unwrap_or_default();
        let name = item.destination.as_bytes();
        hash.update(u32::try_from(name.len()).unwrap_or(u32::MAX).to_be_bytes());
        hash.update(u32::try_from(body.len()).unwrap_or(u32::MAX).to_be_bytes());
        hash.update(name);
        hash.update(&body);
    }
    let digest = hash.finalize();
    format!("sha256:{}", super::paths::hex(&digest))
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn page_readme(page: &PageReport) -> String {
    let folder = posix_dirname(&page.readme);
    let link = |file: &str| {
        let relative = posix_relative(&folder, file);
        if relative.is_empty() {
            posix_basename(file)
        } else {
            relative
        }
    };
    let entry_lines = if page.entries.is_empty() {
        "- No portable design or source entry point was resolved for this page.".to_owned()
    } else {
        page.entries
            .iter()
            .map(|entry| {
                let detail = if entry.kind == "design" {
                    "Design".to_owned()
                } else {
                    let implementation = entry
                        .implementation
                        .as_ref()
                        .and_then(Value::as_str)
                        .unwrap_or("null");
                    format!("Source ({implementation})")
                };
                format!("- {detail}: [`{}`]({})", entry.path, link(&entry.path))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let file_lines = if page.files.is_empty() {
        "- No files are included in this page hash.".to_owned()
    } else {
        page.files
            .iter()
            .map(|file| format!("- [`{file}`]({})", link(file)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let screenshot_lines = if page.screenshots.is_empty() {
        "No reference screenshot was captured for this page.".to_owned()
    } else {
        page.screenshots
            .iter()
            .map(|shot| {
                let size_label = shot
                    .size_label
                    .clone()
                    .unwrap_or_else(|| capitalized(&shot.size));
                let label = if size_label.is_empty() {
                    shot.label.clone()
                } else {
                    format!("{} · {size_label}", shot.label)
                };
                format!(
                    "### {label}\n\n![{} — {label}]({})\n\n`{} × {}` · [`{}`]({})",
                    page.label,
                    link(&shot.path),
                    shot.width,
                    shot.height,
                    shot.path,
                    link(&shot.path)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    format!(
        "# {label}\n\n\
         This file describes one Workbench page for AI agents and other importers.\n\n\
         - Page ID: `{id}`\n\
         - Content hash: `{hash}`\n\
         - Hash algorithm: SHA-256 over each included file’s archive-relative path and raw bytes, in lexical path order\n\n\
         Compare the complete content hash with an earlier export to tell whether this page or one of its exported local dependencies changed. Generated READMEs, archive metadata, package configuration added by the exporter, and files outside the list below are not part of the hash.\n\n\
         ## Entry points\n\n{entry_lines}\n\n\
         Start with the entry points, preserve the existing component API and project conventions, and use the included dependencies and assets as supporting context. Do not assume omitted application code, installed packages, secrets, tests, or build output are available.\n\n\
         ## Reference screenshots\n\n{screenshot_lines}\n\n\
         ## Files in this page hash\n\n{file_lines}\n",
        label = page.label,
        id = page.id,
        hash = page.hash,
    )
}

/// The file a package's `exports` entry points at for an importer.
fn package_export_target(value: &Value) -> Option<String> {
    match value {
        Value::String(target) => Some(target.clone()),
        Value::Array(choices) => choices.iter().find_map(package_export_target),
        Value::Object(conditions) => ["default", "import", "browser", "require", "react-native"]
            .iter()
            .find_map(|condition| conditions.get(*condition).and_then(package_export_target)),
        _ => None,
    }
}

fn storybook_config_directories(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let conventional = root.join(".storybook");
    if conventional.exists() {
        found.push(conventional);
    }
    let Some(package) = fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return found;
    };
    for script in package["scripts"]
        .as_object()
        .into_iter()
        .flat_map(|scripts| scripts.values())
    {
        let script = script.as_str().unwrap_or_default();
        if !STORYBOOK_SCRIPT.is_match(script) {
            continue;
        }
        for matched in CONFIG_DIR.captures_iter(script) {
            let named = matched
                .get(1)
                .or(matched.get(2))
                .or(matched.get(3))
                .map_or("", |m| m.as_str());
            let directory = resolve(root, named);
            if inside(root, &directory) && directory.exists() && !found.contains(&directory) {
                found.push(directory);
            }
        }
    }
    found
}

fn resolve_reference(
    from: &Path,
    specifier: &str,
    owner_root: &Path,
    allowed: &[PathBuf],
) -> Option<PathBuf> {
    let clean = specifier.split(['?', '#']).next().unwrap_or_default();
    if clean.is_empty() || EXTERNAL.is_match(clean) {
        return None;
    }
    let start = if ALIAS.is_match(clean) {
        resolve(&owner_root.join("src"), &clean[2..])
    } else if clean.starts_with('/') {
        resolve(owner_root, &format!(".{clean}"))
    } else {
        resolve(from.parent().unwrap_or(from), clean)
    };
    if !allowed.iter().any(|root| inside(root, &start)) {
        return None;
    }
    if clean.ends_with('/') && is_dir(&start) {
        return Some(start);
    }
    let with = |suffix: &str| PathBuf::from(format!("{}{suffix}", display(&start)));
    if let Some(found) = RESOLVE_EXTENSIONS
        .iter()
        .map(|suffix| with(suffix))
        .find(|candidate| is_file(candidate))
    {
        return Some(found);
    }
    // TypeScript ESM source commonly spells a future JavaScript output path.
    let text = display(&start);
    let script = text
        .strip_suffix(".jsx")
        .map(|stem| (stem, [".tsx", ".ts"]))
        .or_else(|| text.strip_suffix(".js").map(|stem| (stem, [".ts", ".tsx"])));
    if let Some((stem, typescript)) = script
        && let Some(found) = typescript
            .iter()
            .map(|ext| PathBuf::from(format!("{stem}{ext}")))
            .find(|path| is_file(path))
    {
        return Some(found);
    }
    RESOLVE_EXTENSIONS[1..]
        .iter()
        .map(|suffix| start.join(format!("index{suffix}")))
        .find(|candidate| is_file(candidate))
}

fn walk(dir: &Path, visit: &mut dyn FnMut(PathBuf)) {
    for name in names_in(dir) {
        if name.starts_with('.') && name != ".storybook" {
            continue;
        }
        let file = dir.join(&name);
        let Ok(kind) = fs::symlink_metadata(&file).map(|meta| meta.file_type()) else {
            continue;
        };
        if kind.is_dir() {
            if !OMIT_DIRS.contains(&name.as_str()) {
                walk(&file, visit);
            }
        } else if kind.is_file() {
            visit(file);
        }
    }
}

fn nearest_package(file: &Path, boundary: &Path) -> Option<PathBuf> {
    let mut dir = if is_dir(file) {
        file.to_path_buf()
    } else {
        file.parent()?.to_path_buf()
    };
    while inside(boundary, &dir) {
        let candidate = dir.join("package.json");
        if candidate.exists() {
            return Some(candidate);
        }
        if normalize(&dir) == normalize(boundary) {
            break;
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// The files of an export and how they depend on each other.
struct Collector {
    owners: Vec<Owner>,
    allowed: Vec<PathBuf>,
    files: Vec<SourceFile>,
    index: HashMap<PathBuf, usize>,
    queue: Vec<PathBuf>,
    dependencies: HashMap<PathBuf, BTreeSet<PathBuf>>,
    warnings: Vec<String>,
    packages: BTreeSet<String>,
    package_info: HashMap<PathBuf, Option<PackageInfo>>,
    package_roots: HashMap<String, PackageInfo>,
    configured: BTreeSet<PathBuf>,
    sprites: Vec<PathBuf>,
    pages: BTreeMap<String, PageData>,
}

impl Collector {
    fn owner_for(&self, file: &Path, preferred: Option<usize>) -> Option<usize> {
        if let Some(preferred) = preferred
            && inside(&self.owners[preferred].root, file)
        {
            return Some(preferred);
        }
        let mut candidates: Vec<usize> = (0..self.owners.len())
            .filter(|at| inside(&self.owners[*at].root, file))
            .collect();
        candidates.sort_by_key(|at| std::cmp::Reverse(self.owners[*at].root.as_os_str().len()));
        candidates.first().copied()
    }

    fn destination_for(&self, file: &Path, owner: usize) -> String {
        let owner = &self.owners[owner];
        let relative = relative(&owner.root, file);
        if owner.destination.is_empty() {
            relative
        } else {
            format!("{}/{relative}", owner.destination.trim_end_matches('/'))
        }
    }

    fn remember_package(&mut self, file: &Path, boundary: &Path) -> Option<PackageInfo> {
        let manifest = nearest_package(file, boundary)?;
        if !self.package_info.contains_key(&manifest) {
            let info = fs::read_to_string(&manifest)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .map(|package| PackageInfo {
                    root: manifest.parent().map(Path::to_path_buf).unwrap_or_default(),
                    manifest: manifest.clone(),
                    package,
                });
            if let Some(info) = &info
                && let Some(name) = info.package["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
            {
                self.package_roots.insert(name.to_owned(), info.clone());
            }
            self.package_info.insert(manifest.clone(), info);
        }
        self.package_info.get(&manifest).cloned().flatten()
    }

    /// A package of this project the specifier names, and the file it resolves to.
    fn local_package(
        &mut self,
        from: &Path,
        specifier: &str,
        owner: usize,
    ) -> Option<(PackageInfo, Option<PathBuf>)> {
        let name = package_name(specifier)?;
        let mut info = self.package_roots.get(&name).cloned();
        if info.is_none() {
            let owner_root = self.owners[owner].root.clone();
            let importer = self.remember_package(from, &owner_root);
            let version = importer.as_ref().and_then(|importer| {
                DEPENDENCY_GROUPS
                    .iter()
                    .find_map(|group| importer.package[*group][&name].as_str().map(str::to_owned))
            });
            if let (Some(importer), Some(version)) = (&importer, version)
                && let Some(linked) = LINKED.captures(&version)
            {
                let linked_root = resolve(&importer.root, &linked[1]);
                if let Some(linked_owner) = self.owner_for(&linked_root, None) {
                    let boundary = self.owners[linked_owner].root.clone();
                    info = self.remember_package(&linked_root, &boundary);
                }
            }
        }
        let info = info.filter(|info| info.package["name"] == name.as_str())?;
        let subpath = &specifier[name.len()..];
        let key = if subpath.is_empty() {
            ".".to_owned()
        } else {
            format!(".{subpath}")
        };
        let exported = &info.package["exports"];
        let mut target = match exported {
            Value::String(only) if key == "." => Some(only.clone()),
            Value::Object(map) if map.contains_key(&key) => package_export_target(&map[&key]),
            Value::Object(map) if key == "." && !map.keys().any(|entry| entry.starts_with('.')) => {
                package_export_target(exported)
            }
            Value::Array(_) if key == "." => package_export_target(exported),
            _ => None,
        };
        if target.is_none() && key == "." {
            target = info.package["module"]
                .as_str()
                .or(info.package["main"].as_str())
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned);
        }
        if target.is_none() && !subpath.is_empty() {
            target = Some(format!(".{subpath}"));
        }
        let target = target?;
        let resolved = resolve_reference(&info.manifest, &target, &info.root, &self.allowed);
        Some((info, resolved))
    }

    fn add(&mut self, file: &Path, preferred: Option<usize>, reason: &str, page: Option<&str>) {
        let file = normalize(file);
        let Some(owner) = self.owner_for(&file, preferred) else {
            return;
        };
        if OMIT_FILE.is_match(&display(&file).replace('\\', "/")) {
            return;
        }
        let Ok(meta) = fs::metadata(&file) else {
            self.warnings
                .push(format!("Missing {reason}: {}", display(&file)));
            return;
        };
        if meta.is_dir() {
            let mut children = Vec::new();
            walk(&file, &mut |child| children.push(child));
            for child in children {
                self.add(&child, Some(owner), reason, page);
            }
            return;
        }
        if !meta.is_file() {
            return;
        }
        let relative = relative(&self.owners[owner].root, &file);
        if relative.split('/').any(|part| OMIT_DIRS.contains(&part)) {
            return;
        }
        if let Some(page) = page
            && let Some(data) = self.pages.get_mut(page)
        {
            data.direct.insert(file.clone());
        }
        if self.index.contains_key(&file) {
            return;
        }
        let destination = self.destination_for(&file, owner);
        self.index.insert(file.clone(), self.files.len());
        self.files.push(SourceFile {
            source: file.clone(),
            destination,
            owner,
        });
        let boundary = self.owners[owner].root.clone();
        self.remember_package(&file, &boundary);
        self.queue.push(file);
    }

    fn link(&mut self, from: &Path, target: &Path) {
        let target = normalize(target);
        let linked: Vec<PathBuf> = if self.index.contains_key(&target) {
            vec![target]
        } else {
            self.files
                .iter()
                .filter(|item| inside(&target, &item.source))
                .map(|item| item.source.clone())
                .collect()
        };
        if !linked.is_empty() {
            self.dependencies
                .entry(from.to_path_buf())
                .or_default()
                .extend(linked);
        }
    }

    /// Follows one queued file's configuration, imports and assets.
    fn follow(&mut self, file: &Path) {
        let item = self.files[self.index[file]].clone();
        let owner_root = self.owners[item.owner].root.clone();
        let package = self.remember_package(file, &owner_root);
        if let Some(package) = &package
            && self.configured.insert(package.root.clone())
        {
            for name in names_in(&package.root)
                .into_iter()
                .filter(|name| is_config_file(name))
            {
                self.add(
                    &package.root.join(name),
                    Some(item.owner),
                    "package configuration",
                    None,
                );
            }
        }
        if let Some(package) = &package
            && !is_config_file(&basename(file))
        {
            let configs: Vec<PathBuf> = self
                .files
                .iter()
                .filter(|candidate| {
                    candidate.source.parent() == Some(package.root.as_path())
                        && is_config_file(&basename(&candidate.source))
                })
                .map(|candidate| candidate.source.clone())
                .collect();
            for config in configs {
                self.link(file, &config);
            }
        }
        let ext = extension(file);
        if !TEXT_EXTENSIONS.contains(&ext.as_str()) && ext != ".html" && ext != ".htm" {
            return;
        }
        let Ok(bytes) = fs::read(file) else {
            return;
        };
        let body = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(package) = &package
            && VITE_CONFIG.is_match(&basename(file))
        {
            for found in ICON_DIRS.captures_iter(&body) {
                let asset = resolve(&package.root, &found[1]);
                if !self.allowed.iter().any(|root| inside(root, &asset)) {
                    continue;
                }
                self.add(&asset, Some(item.owner), "Vite asset directory", None);
                self.link(file, &asset);
                self.sprites.push(asset);
            }
        }
        let css_imports = is_stylesheet(&ext).then(|| stylesheet_imports(&body));
        for specifier in parse_references(&ext, &body) {
            self.follow_reference(file, &item, &ext, &specifier, css_imports.as_ref());
        }
    }

    fn follow_reference(
        &mut self,
        file: &Path,
        item: &SourceFile,
        ext: &str,
        specifier: &str,
        css: Option<&BTreeSet<String>>,
    ) {
        let dependency = (is_script(ext) && !ALIAS.is_match(specifier))
            .then(|| package_name(specifier))
            .flatten();
        // CSS imports can use the same package export map as JavaScript imports.
        let css_package = css
            .filter(|imports| imports.contains(specifier))
            .and_then(|_| package_name(specifier));
        let named = dependency.clone().or(css_package);
        let local = match &named {
            Some(_) => self.local_package(file, specifier, item.owner),
            None => None,
        };
        if let (Some((info, Some(target))), Some(named)) = (&local, &named) {
            let owner = self.owner_for(&info.root, None);
            self.add(
                &info.manifest,
                owner,
                &format!("package metadata for {named}"),
                None,
            );
            self.add(
                target,
                owner,
                &format!("local package dependency of {}", basename(file)),
                None,
            );
            self.link(file, &info.manifest);
            self.link(file, target);
        } else if let Some(dependency) = dependency {
            if dependency != WORKBENCH_SDK {
                self.packages.insert(dependency);
            }
        } else if let Some(resolved) = resolve_reference(
            file,
            specifier,
            &self.owners[item.owner].root,
            &self.allowed,
        ) {
            self.add(
                &resolved,
                Some(item.owner),
                &format!("dependency of {}", basename(file)),
                None,
            );
            self.link(file, &resolved);
        } else if !EXTERNAL.is_match(specifier) {
            self.warnings.push(format!(
                "Could not resolve “{specifier}” from {}",
                item.destination
            ));
        }
    }
}

impl Collector {
    /// The project and each implementation folder outside it, as owners.
    fn new(root: &Path, view: &ExportView, warnings: Vec<String>) -> Self {
        let mut owners = vec![Owner {
            key: "project".into(),
            root: root.to_path_buf(),
            destination: String::new(),
        }];
        for (key, implementation) in &view.implementations {
            let Some(implementation_root) = &implementation.root else {
                continue;
            };
            let implementation_root = normalize(implementation_root);
            if owners.iter().any(|owner| owner.root == implementation_root) {
                continue;
            }
            let destination = if inside(root, &implementation_root) {
                relative(root, &implementation_root)
            } else {
                format!("implementations/{}", slug(Some(key)))
            };
            owners.push(Owner {
                key: key.clone(),
                root: implementation_root,
                destination,
            });
        }
        Self {
            allowed: owners.iter().map(|owner| owner.root.clone()).collect(),
            owners,
            files: Vec::new(),
            index: HashMap::new(),
            queue: Vec::new(),
            dependencies: HashMap::new(),
            warnings,
            packages: BTreeSet::new(),
            package_info: HashMap::new(),
            package_roots: HashMap::new(),
            configured: BTreeSet::new(),
            sprites: Vec::new(),
            pages: view
                .pages
                .iter()
                .map(|(src, page)| {
                    (
                        src.clone(),
                        PageData {
                            id: src.clone(),
                            label: page.label.clone(),
                            direct: BTreeSet::new(),
                            entries: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn entry(&mut self, src: &str, entry: PageEntry) {
        if let Some(data) = self.pages.get_mut(src) {
            data.entries.push(entry);
        }
    }

    /// The manifest, every page's design and implementation entry points,
    /// the files portable previews were built from, and each owner's configuration.
    fn entry_points(&mut self, root: &Path, view: &ExportView, options: &ExportOptions) {
        // A space of a file that lists several, or whose root is elsewhere,
        // names its file; one outside the project isn't exported.
        let manifest = options
            .manifest
            .clone()
            .unwrap_or_else(|| root.join("workbench.yaml"));
        self.add(&manifest, Some(0), "workbench manifest", None);
        for (src, page) in &view.pages {
            if let Some(design) = &page.design {
                self.add(
                    design,
                    Some(0),
                    &format!("design: {}", page.label),
                    Some(src),
                );
                let design = normalize(design);
                if self.index.contains_key(&design) {
                    let path = self.destination_for(&design, 0);
                    self.entry(
                        src,
                        PageEntry {
                            kind: "design",
                            path,
                            implementation: None,
                        },
                    );
                }
            }
            for entry in &page.code {
                let Some(path) = entry.path.as_ref().filter(|_| entry.exists) else {
                    continue;
                };
                let owner = self
                    .owners
                    .iter()
                    .position(|candidate| Some(&candidate.key) == entry.implementation.as_ref())
                    .or_else(|| self.owner_for(path, None));
                self.add(path, owner, &format!("source: {}", page.label), Some(src));
                if let Some(owner) = owner {
                    let path = self.destination_for(&normalize(path), owner);
                    let implementation = Some(
                        entry
                            .implementation
                            .clone()
                            .map_or(Value::Null, Value::String),
                    );
                    self.entry(
                        src,
                        PageEntry {
                            kind: "source",
                            path,
                            implementation,
                        },
                    );
                }
            }
        }
        for preview in options
            .portable
            .iter()
            .flat_map(|portable| &portable.previews)
        {
            if !self.pages.contains_key(&preview.file) {
                continue;
            }
            for file in &preview.files {
                let owner = self.owner_for(Path::new(file), None);
                self.add(
                    Path::new(file),
                    owner,
                    "Workbench preview dependency",
                    Some(&preview.file),
                );
            }
        }
        for owner in 0..self.owners.len() {
            let owner_root = self.owners[owner].root.clone();
            let storybook = owner_root.join(".storybook");
            if storybook.exists() {
                self.add(&storybook, Some(owner), "Storybook configuration", None);
            }
            for name in names_in(&owner_root)
                .into_iter()
                .filter(|name| is_config_file(name))
            {
                self.add(
                    &owner_root.join(name),
                    Some(owner),
                    "project configuration",
                    None,
                );
            }
        }
        self.storybook_configs(view);
    }

    /// A Storybook implementation's configuration folders, in its story pages' closures.
    fn storybook_configs(&mut self, view: &ExportView) {
        for (key, implementation) in &view.implementations {
            let Some(implementation_root) = implementation
                .root
                .as_ref()
                .filter(|_| implementation.kind.as_deref() == Some("storybook"))
            else {
                continue;
            };
            let implementation_root = normalize(implementation_root);
            let owner = self.owner_for(&implementation_root, None);
            let story_pages: Vec<&String> = view
                .pages
                .iter()
                .filter(|(_, page)| {
                    page.code
                        .iter()
                        .any(|entry| entry.implementation.as_deref() == Some(key.as_str()))
                })
                .map(|(src, _)| src)
                .collect();
            for directory in storybook_config_directories(&implementation_root) {
                if story_pages.is_empty() {
                    self.add(&directory, owner, "Storybook configuration", None);
                }
                for src in &story_pages {
                    self.add(&directory, owner, "Storybook configuration", Some(src));
                }
            }
        }
    }

    /// Follows every queued file, then ties sprite folders to the components using them.
    fn follow_all(&mut self) {
        let mut at = 0;
        while at < self.queue.len() {
            let file = self.queue[at].clone();
            self.follow(&file);
            at += 1;
        }
        // A virtual sprite is registered by the app, while components reference
        // its generated symbol IDs: keep the SVG sources in those components' closures.
        if self.sprites.is_empty() {
            return;
        }
        let sources: Vec<PathBuf> = self.files.iter().map(|item| item.source.clone()).collect();
        for source in sources {
            if !SCRIPT_FILE.is_match(&display(&source)) {
                continue;
            }
            let Ok(body) = fs::read(&source) else {
                continue;
            };
            if !String::from_utf8_lossy(&body).contains("#icon-") {
                continue;
            }
            for directory in self.sprites.clone() {
                self.link(&source, &directory);
            }
        }
    }

    /// The packages the exported files import, with the versions their manifests ask for.
    fn dependencies(&mut self, portable: Option<&PortableExport>) -> Vec<Dependency> {
        let mut manifests: Vec<PathBuf> = Vec::new();
        for item in &self.files {
            if let Some(found) = nearest_package(&item.source, &self.owners[item.owner].root)
                && !manifests.contains(&found)
            {
                manifests.push(found);
            }
        }
        let mut versions: HashMap<String, Option<String>> = HashMap::new();
        for package in portable
            .iter()
            .flat_map(|portable| &portable.previews)
            .flat_map(|preview| &preview.packages)
        {
            self.packages.insert(package.name.clone());
            versions.insert(package.name.clone(), package.version.clone());
        }
        for manifest in &manifests {
            let Some(package) = fs::read_to_string(manifest)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            else {
                self.warnings.push(format!(
                    "Could not read package manifest: {}",
                    display(manifest)
                ));
                continue;
            };
            for group in DEPENDENCY_GROUPS {
                for (name, version) in package[group].as_object().into_iter().flatten() {
                    let unknown = versions
                        .get(name)
                        .is_none_or(|known| known.as_deref().is_none_or(str::is_empty));
                    if self.packages.contains(name) && unknown {
                        versions.insert(name.clone(), version.as_str().map(str::to_owned));
                    }
                }
            }
        }
        self.packages
            .iter()
            .map(|name| Dependency {
                name: name.clone(),
                version: versions
                    .get(name)
                    .cloned()
                    .flatten()
                    .filter(|version| !version.is_empty()),
            })
            .collect()
    }

    /// Each page's files, its direct entry points and everything they depend on, and their hash.
    fn page_reports(&self) -> Vec<PageReport> {
        let mut sources: Vec<&String> = self.pages.keys().collect();
        sources.sort();
        sources
            .into_iter()
            .map(|src| {
                let data = &self.pages[src];
                let mut included = BTreeSet::new();
                let mut pending: Vec<PathBuf> = data.direct.iter().cloned().collect();
                while let Some(current) = pending.pop() {
                    if included.contains(&current) || !self.index.contains_key(&current) {
                        continue;
                    }
                    pending.extend(
                        self.dependencies
                            .get(&current)
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                    included.insert(current);
                }
                let items: Vec<&SourceFile> = included
                    .iter()
                    .map(|file| &self.files[self.index[file]])
                    .collect();
                let mut files: Vec<String> =
                    items.iter().map(|item| item.destination.clone()).collect();
                files.sort();
                PageReport {
                    id: data.id.clone(),
                    label: data.label.clone(),
                    hash: content_hash(&items),
                    readme: String::new(),
                    entries: data.entries.clone(),
                    files,
                    screenshots: Vec::new(),
                }
            })
            .collect()
    }

    fn destinations(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .files
            .iter()
            .map(|item| item.destination.clone())
            .collect();
        out.sort();
        out
    }
}

/// Puts each page's guide beside its primary entry point, named after the
/// page where pages share a folder; answers each page's screenshot folder.
fn place_guides(
    pages: &mut [PageReport],
    mut reserved: BTreeSet<String>,
) -> HashMap<String, String> {
    let directories = page_slugs(&pages.iter().map(|page| page.id.clone()).collect::<Vec<_>>());
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for page in pages.iter() {
        let primary = page
            .entries
            .iter()
            .find(|entry| entry.kind == "design")
            .or(page.entries.first());
        let mut parent = primary.map_or_else(
            || format!("workbench-pages/{}", directories[&page.id]),
            |entry| posix_dirname(&entry.path),
        );
        if parent == "." {
            parent.clear();
        }
        *counts.entry(parent.clone()).or_default() += 1;
        parents.insert(page.id.clone(), parent);
    }
    reserved.insert("README.md".into());
    reserved.insert("canonic-export.json".into());
    let mut screenshots: HashMap<String, String> = HashMap::new();
    let mut names: HashMap<String, BTreeSet<String>> = HashMap::new();
    for page in pages {
        let parent = &parents[&page.id];
        let shared = counts[parent] > 1;
        let taken = names.entry(parent.clone()).or_default();
        let mut base = slug(Some(&page.label));
        if taken.contains(&base) {
            base = format!("{base}-{}", short_hash(&page.id));
        }
        taken.insert(base.clone());
        let own = format!("{base}.README.md");
        let mut readme = posix_join(&[parent, if shared { &own } else { "README.md" }]);
        if reserved.contains(&readme) {
            readme = posix_join(&[parent, &own]);
        }
        reserved.insert(readme.clone());
        page.readme = readme;
        screenshots.insert(
            page.id.clone(),
            posix_join(&[parent, "screenshots", if shared { &base } else { "" }]),
        );
    }
    screenshots
}

/// Each reference screenshot in its page's screenshot folder, recorded in the page's report.
fn place_screenshots(
    prefix: &str,
    pages: &mut [PageReport],
    folders: &HashMap<String, String>,
    shots: &[Screenshot],
) -> Vec<ZipEntry> {
    let mut entries = Vec::new();
    let mut names: HashMap<String, usize> = HashMap::new();
    let or = |text: &str, fallback: &str| {
        if text.is_empty() {
            fallback.to_owned()
        } else {
            text.to_owned()
        }
    };
    for shot in shots {
        let Some(page) = pages.iter_mut().find(|page| page.id == shot.page) else {
            continue;
        };
        let base = slug(Some(&or(&or(&shot.variant, &shot.label), "default")));
        let count = names.entry(format!("{}\0{base}", page.id)).or_default();
        *count += 1;
        let name = if *count > 1 {
            format!("{base}-{count}.jpg")
        } else {
            format!("{base}.jpg")
        };
        let screenshot = PageScreenshot {
            scenario: Some(shot.scenario.clone()).filter(|scenario| !scenario.is_empty()),
            label: or(&shot.label, "Default"),
            size: or(&shot.size, "fit"),
            path: posix_join(&[&folders[&page.id], &name]),
            width: shot.width,
            height: shot.height,
            size_label: Some(shot.size_label.clone()).filter(|label| !label.is_empty()),
        };
        entries.push(ZipEntry::new(
            format!("{prefix}/{}", screenshot.path),
            shot.body.clone(),
        ));
        page.screenshots.push(screenshot);
    }
    entries
}

/// The portable viewer's files and, unless the project has its own, the SDK's types.
fn portable_entries(
    prefix: &str,
    root: &Path,
    collector: &Collector,
    portable: &PortableExport,
    product: Option<&Path>,
) -> Result<Vec<ZipEntry>> {
    let mut entries = Vec::new();
    for file in &portable.files {
        if !file.path.starts_with("browser/") || file.path.split('/').any(|part| part == "..") {
            bail!("Invalid portable export path");
        }
        let body = base64::engine::general_purpose::STANDARD
            .decode(&file.data)
            .map_err(|error| anyhow!("Invalid portable export file {}: {error}", file.path))?;
        entries.push(ZipEntry::new(format!("{prefix}/{}", file.path), body));
    }
    if !collector
        .index
        .contains_key(&root.join("workbench-env.d.ts"))
    {
        let product = product.ok_or_else(|| {
            anyhow!("The Workbench SDK types are unavailable for a portable export.")
        })?;
        let types = fs::read_to_string(product.join("sdk/api.d.ts"))
            .map_err(|error| anyhow!("The Workbench SDK types are unavailable: {error}"))?;
        entries.push(ZipEntry::new(
            format!("{prefix}/workbench-env.d.ts"),
            format!("// Canonic types: see browser/CANONIC-LICENSE.txt.\ndeclare module \"@canonic2/workbench\" {{\n{types}\n}}\n"),
        ));
    }
    Ok(entries)
}

/// The outer ZIP a download of a split export gets.
fn bundle_of(prefix: &str, archives: &[Archive]) -> Option<Archive> {
    (archives.len() > 1).then(|| {
        let mut outer = vec![ZipEntry::new("README.md", format!(
            "# Workbench export ZIP parts\n\nThis bundle contains {} attachment-sized ZIP files. Extract this outer ZIP, then upload the numbered ZIP parts together or extract every part into one directory.\n",
            archives.len()
        ))];
        outer.extend(archives.iter().map(|archive| ZipEntry::new(archive.filename.clone(), archive.body.clone())));
        Archive { filename: format!("{prefix}-parts.zip"), body: zip(&outer), content_type: "application/zip", files: Vec::new() }
    })
}

/// The export of `view`'s pages from the project at `root`.
///
/// # Errors
/// A portable file outside `browser/`, the SDK types missing for a portable
/// export, an exported file that can't be read, or an archive that can't be
/// split within its limit.
pub(crate) fn create(
    root: &Path,
    view: &ExportView,
    options: &ExportOptions,
) -> Result<ExportResult> {
    let root = normalize(root);
    let prefix = format!("{}-design-system", slug(view.name.as_deref()));
    let mut collector = Collector::new(&root, view, options.warnings.clone());
    collector.entry_points(&root, view, options);
    collector.follow_all();
    let dependencies = collector.dependencies(options.portable.as_ref());
    let mut pages = collector.page_reports();
    let folders = place_guides(&mut pages, collector.destinations().into_iter().collect());
    let screenshots = place_screenshots(&prefix, &mut pages, &folders, &options.screenshots);
    let unique = |warnings: &[String]| {
        warnings
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };
    let mut report = ExportReport {
        request: options.request.clone(),
        name: view.name.clone(),
        generated_at: iso_now(),
        selection: "Workbench design/source/story entry points plus transitive local imports, assets, and captured visual references.".into(),
        files: collector.destinations(),
        pages,
        dependencies,
        warnings: unique(&collector.warnings),
        capture_warnings: unique(&options.capture_warnings),
        browser: options.portable.as_ref().map(|portable| BrowserReport {
            entry: "browser/index.html".into(),
            manifest: "browser/workbench.json".into(),
            previews: portable.previews.iter().map(|preview| BrowserPreview {
                id: preview.id.clone(), source: preview.file.clone(),
                entry: format!("{}/index.html", preview.directory), scenarios: preview.scenarios.clone(),
            }).collect(),
            warnings: portable.warnings.clone(),
        }),
        parts: None,
    };

    let mut entries: Vec<ZipEntry> = Vec::new();
    for item in &collector.files {
        let body = fs::read(&item.source)
            .map_err(|error| anyhow!("{}: {error}", display(&item.source)))?;
        entries.push(ZipEntry::new(
            format!("{prefix}/{}", item.destination),
            body,
        ));
    }
    entries.sort_by(|a, b| locale_compare(&a.name, &b.name));
    for page in &report.pages {
        entries.push(ZipEntry::new(
            format!("{prefix}/{}", page.readme),
            page_readme(page),
        ));
    }
    entries.extend(screenshots);
    if let Some(portable) = &options.portable {
        entries.extend(portable_entries(
            &prefix,
            &root,
            &collector,
            portable,
            options.product.as_deref(),
        )?);
    }
    if !collector.index.contains_key(&root.join("package.json")) {
        let mut generated =
            json!({ "private": true, "name": prefix, "version": "0.0.0", "dependencies": {} });
        for dependency in &report.dependencies {
            generated["dependencies"][&dependency.name] =
                json!(dependency.version.as_deref().unwrap_or("*"));
        }
        entries.push(ZipEntry::new(
            format!("{prefix}/package.json"),
            serde_json::to_string_pretty(&generated)? + "\n",
        ));
    }
    entries.sort_by(|a, b| locale_compare(&a.name, &b.name));
    let readme = space_readme(view.name.as_deref(), options.portable.is_some());
    let archives = split_archives(
        &prefix,
        entries,
        &readme,
        &mut report,
        options.max_archive_bytes,
    )?;
    let bundle = bundle_of(&prefix, &archives);
    Ok(ExportResult {
        archives,
        bundle,
        report,
    })
}

fn space_readme(name: Option<&str>, portable: bool) -> String {
    let mut readme = format!(
        "# {} design-system export\n\n\
         This archive was generated from Workbench. Workspace files keep their original project-relative paths at this archive’s root; sources outside the workspace are under `implementations/`.\n\n\
         The export starts with the selected design, component, page, and story sources recorded in `canonic-export.json`, then includes their transitive local imports and referenced assets. Each generated page guide embeds reference screenshots for the requested scenarios, stories, and sizes; a size that fills the canvas, and Fit, are captured 1440 wide or 900 tall. Duplicate capture sizes are removed. A docs page instead embeds, for each lens, the whole page at its 960-pixel layout and each example the lens renders, cropped to its panel. It intentionally excludes dependencies installed in `node_modules`, build output, secrets, tests, and unrelated application files.\n\n\
         See `canonic-export.json` for the exact file list, package dependencies, unresolved references, and each page’s content hash. Each generated page guide and its `screenshots/` directory live beside that component or page’s primary exported entry point. Install the listed packages with your preferred package manager before running the copied Storybook or app setup.\n",
        name.filter(|name| !name.is_empty()).unwrap_or("Workbench")
    );
    if portable {
        readme.push_str("\n## Portable browser previews\n\nServe this extracted directory with any static HTTP server and open `browser/index.html`. The viewer includes searchable previews, scenario and size selection, editable controls, reset, actions, and documentation. Copy its address to share a selection. These compiled Workbench previews need no Electron, Workbench, package installation, or build step. Direct preview pages accept `?scenario=<id>`. Original editable sources remain in their project-relative locations. Check `browser.warnings` in the export report for previews that could not be built.\n");
    }
    readme
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::export::archive::tests::{names, read_zip};
    use crate::export::types::{
        CodeEntry, ExportImplementation, ExportPage, PortableExport, PortableFile, Screenshot,
    };

    /// A temporary project folder, removed when dropped.
    pub(crate) struct Project(pub(crate) PathBuf);

    impl Project {
        pub(crate) fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "canonic-export-{name}-{}",
                super::super::paths::random_hex()
            ));
            fs::create_dir_all(&root).expect("project folder");
            Self(normalize(&fs::canonicalize(&root).expect("canonical root")))
        }
        pub(crate) fn write(&self, file: &str, body: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(file);
            fs::create_dir_all(path.parent().expect("parent")).expect("folder");
            fs::write(&path, body).expect("write");
            path
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }

    fn page(label: &str, design: Option<PathBuf>, code: Vec<(&str, PathBuf)>) -> ExportPage {
        ExportPage {
            label: label.into(),
            design,
            code: code
                .into_iter()
                .map(|(implementation, path)| CodeEntry {
                    path: Some(path),
                    exists: true,
                    implementation: Some(implementation.into()),
                })
                .collect(),
        }
    }

    fn implementation(root: &Path, kind: Option<&str>) -> ExportImplementation {
        ExportImplementation {
            root: Some(root.to_path_buf()),
            kind: kind.map(str::to_owned),
        }
    }

    fn bodies(zip: &[u8]) -> HashMap<String, String> {
        read_zip(zip)
            .into_iter()
            .map(|(name, body)| (name, String::from_utf8_lossy(&body).into_owned()))
            .collect()
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one end-to-end archive check")]
    fn exports_workbench_entries_and_transitive_ui_dependencies_without_unrelated_code() {
        let project = Project::new("entries");
        let root = &project.0;
        project.write("workbench.yaml", "name: Acme\n");
        project.write(
            "package.json",
            r#"{"dependencies":{"react":"^19.0.0","stripe":"^18.0.0"}}"#,
        );
        let story = project.write("src/button/button.stories.tsx", "import './button';\n");
        project.write("src/button/button.tsx", "import React from 'react'; import '../shared/tokens.css'; // import '../billing/charge';\n");
        project.write(
            "src/shared/tokens.css",
            ".button{background:url(./dot.svg)}\n",
        );
        project.write("src/shared/dot.svg", "<svg/>");
        project.write(
            "src/button/button.test.tsx",
            "import '../billing/charge';\n",
        );
        project.write("src/billing/charge.ts", "import Stripe from 'stripe';\n");
        let home = project.write(
            "pages/home.html",
            "<link href=\"./home.css\"><img src=\"../src/shared/dot.svg\">",
        );
        project.write("pages/home.css", "main { color: rebeccapurple }\n");
        let view = ExportView {
            name: Some("Acme UI".into()),
            implementations: vec![("storybook".into(), implementation(root, None))],
            pages: vec![
                ("home".into(), page("Home", Some(home), vec![])),
                (
                    "button".into(),
                    page("Button", None, vec![("storybook", story)]),
                ),
            ],
        };
        let shot = Screenshot {
            page: "button".into(),
            scenario: "default".into(),
            variant: "default-laptop".into(),
            label: "Default".into(),
            size: "laptop".into(),
            width: 1512,
            height: 982,
            body: b"reference jpeg".to_vec(),
            ..Screenshot::default()
        };
        let result = create(
            root,
            &view,
            &ExportOptions {
                screenshots: vec![shot],
                ..ExportOptions::default()
            },
        )
        .expect("export");
        let files = &result.report.files;
        for expected in [
            "src/button/button.stories.tsx",
            "src/button/button.tsx",
            "src/shared/tokens.css",
            "src/shared/dot.svg",
            "pages/home.html",
            "pages/home.css",
        ] {
            assert!(
                files.contains(&expected.to_owned()),
                "{expected} in {files:?}"
            );
        }
        assert!(!files.iter().any(|file| file.starts_with("project/")
            || file.contains("billing")
            || file.contains(".test.")));
        assert_eq!(
            result.report.dependencies,
            [Dependency {
                name: "react".into(),
                version: Some("^19.0.0".into())
            }]
        );
        let body = &result.download().body;
        let archived = names(body);
        for expected in [
            "acme-ui-design-system/canonic-export.json",
            "acme-ui-design-system/pages/README.md",
            "acme-ui-design-system/src/button/README.md",
        ] {
            assert!(archived.contains(&expected.to_owned()), "{expected}");
        }
        let ids: Vec<&str> = result
            .report
            .pages
            .iter()
            .map(|page| page.id.as_str())
            .collect();
        assert_eq!(ids, ["button", "home"]);
        let (button, home) = (&result.report.pages[0], &result.report.pages[1]);
        assert!(button.hash.starts_with("sha256:") && button.hash.len() == 71);
        assert!(button.files.contains(&"src/button/button.tsx".to_owned()));
        assert!(button.files.contains(&"src/shared/dot.svg".to_owned()));
        assert!(home.files.contains(&"pages/home.html".to_owned()));
        assert!(!home.files.contains(&"src/button/button.tsx".to_owned()));
        let contents = bodies(body);
        assert_eq!(
            archived
                .iter()
                .filter(|name| *name == "acme-ui-design-system/package.json")
                .count(),
            1
        );
        assert!(contents["acme-ui-design-system/package.json"].contains("stripe"));
        let guide = &contents["acme-ui-design-system/src/button/README.md"];
        assert!(guide.contains(&button.hash));
        assert!(guide.contains("Source (storybook)"));
        assert!(guide.contains("Default · Laptop"));
        assert!(guide.contains("screenshots/default-laptop.jpg"));
        assert_eq!(
            contents["acme-ui-design-system/src/button/screenshots/default-laptop.jpg"],
            "reference jpeg"
        );
        assert_eq!(
            serde_json::to_value(&button.screenshots).expect("json"),
            json!([{
                "scenario": "default", "label": "Default", "size": "laptop",
                "path": "src/button/screenshots/default-laptop.jpg", "width": 1512, "height": 982,
            }])
        );

        project.write("src/billing/charge.ts", "unrelated change\n");
        let unrelated = create(root, &view, &ExportOptions::default()).expect("export");
        assert_eq!(unrelated.report.pages[0].hash, button.hash);
        project.write("src/shared/dot.svg", "<svg>changed</svg>");
        let changed = create(root, &view, &ExportOptions::default()).expect("export");
        assert_ne!(changed.report.pages[0].hash, button.hash);
        assert_ne!(changed.report.pages[1].hash, home.hash);
        assert_eq!(&body[0..4], &0x0403_4b50_u32.to_le_bytes());
    }

    #[test]
    fn keeps_generated_guides_and_screenshots_distinct_when_pages_share_a_folder() {
        let project = Project::new("pages");
        project.write("workbench.yaml", "name: Pages\n");
        let sign_in = project.write("pages/sign-in.html", "<main>Sign in</main>");
        let account = project.write("pages/account.html", "<main>Account</main>");
        let view = ExportView {
            name: Some("Pages".into()),
            implementations: vec![],
            pages: vec![
                ("signin".into(), page("Sign in", Some(sign_in), vec![])),
                ("account".into(), page("Account", Some(account), vec![])),
            ],
        };
        let shot = |page: &str, body: &str| Screenshot {
            page: page.into(),
            variant: "default-mobile".into(),
            label: "Default".into(),
            size: "mobile".into(),
            width: 393,
            height: 852,
            body: body.as_bytes().to_vec(),
            ..Screenshot::default()
        };
        let result = create(
            &project.0,
            &view,
            &ExportOptions {
                screenshots: vec![shot("signin", "signin"), shot("account", "account")],
                ..ExportOptions::default()
            },
        )
        .expect("export");
        let archived = names(&result.download().body);
        for expected in [
            "pages/sign-in.README.md",
            "pages/account.README.md",
            "pages/screenshots/sign-in/default-mobile.jpg",
            "pages/screenshots/account/default-mobile.jpg",
        ] {
            assert!(
                archived.contains(&format!("pages-design-system/{expected}")),
                "{expected}"
            );
        }
    }

    #[test]
    fn downloads_one_zip_containing_attachment_sized_zip_parts() {
        let project = Project::new("split");
        project.write("workbench.yaml", "name: Split\n");
        let home = project.write("pages/home.html", "<main>Home</main>");
        let view = ExportView {
            name: Some("Split".into()),
            implementations: vec![],
            pages: vec![("home".into(), page("Home", Some(home), vec![]))],
        };
        let screenshots: Vec<Screenshot> = (0..4_u8)
            .map(|index| Screenshot {
                page: "home".into(),
                variant: format!("scenario-{index}-laptop"),
                label: format!("Scenario {index}"),
                size: "laptop".into(),
                width: 1512,
                height: 982,
                body: vec![index; 25_000],
                ..Screenshot::default()
            })
            .collect();
        let result = create(
            &project.0,
            &view,
            &ExportOptions {
                screenshots: screenshots.clone(),
                max_archive_bytes: Some(70_000),
                ..ExportOptions::default()
            },
        )
        .expect("export");
        assert!(result.archives.len() > 1);
        assert_eq!(result.download().filename, "split-design-system-parts.zip");
        let outer: Vec<String> = names(&result.download().body)
            .into_iter()
            .filter(|name| name.rsplit('.').next() == Some("zip"))
            .collect();
        assert_eq!(
            outer,
            result
                .archives
                .iter()
                .map(|archive| archive.filename.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.report.parts.as_ref().map(Vec::len),
            Some(result.archives.len())
        );
        let payload: Vec<String> = result
            .archives
            .iter()
            .flat_map(|archive| archive.files.clone())
            .collect();
        for shot in screenshots {
            assert!(payload.contains(&format!("pages/screenshots/{}.jpg", shot.variant)));
        }
    }

    #[test]
    fn resolves_a_local_package_theme_imported_from_css_through_package_exports() {
        let project = Project::new("theme");
        let root = &project.0;
        project.write("workbench.yaml", "name: Theme\n");
        project.write(
            "packages/frontend-web/package.json",
            r#"{"name":"@acme/frontend","dependencies":{"@acme/ui":"1.0.0"}}"#,
        );
        project.write(
            "packages/ui/package.json",
            r#"{"name":"@acme/ui","exports":{"./theme.css":"./src/styles/theme.css"}}"#,
        );
        project.write("packages/frontend-web/vite.config.ts", "import path from \"node:path\";\nexport default { plugins: [createSvgIconsPlugin({ iconDirs: [path.resolve(process.cwd(), \"../ui/src/components/icons-next/assets\")] })] };\n");
        let globals = project.write(
            "packages/frontend-web/src/globals.css",
            "@import \"@acme/ui/theme.css\";\n",
        );
        let story = project.write("packages/ui/src/button.stories.tsx", "import \"./components/progress-bar/progress-bar.web.tsx\";\nimport \"./components/icons-next/icon-next.web.tsx\";\nexport const Button = () => null;\n");
        let theme = project.write(
            "packages/ui/src/styles/theme.css",
            ":root { --color-primary: blue; }\n",
        );
        project.write(
            "packages/ui/src/components/icons-next/assets/plus.svg",
            "<svg id=\"plus\"/>\n",
        );
        project.write(
            "packages/ui/src/components/icons-next/icon-next.web.tsx",
            "export const Icon = () => <use href=\"#icon-plus\" />;\n",
        );
        project.write(
            "packages/ui/src/components/progress-bar/progress-bar.web.tsx",
            "import type { Props } from \"./types/progress-bar-props.js\";\n",
        );
        project.write(
            "packages/ui/src/components/progress-bar/types/progress-bar-props.ts",
            "export type Props = { current: number };\n",
        );
        let view = ExportView {
            name: Some("Theme".into()),
            implementations: vec![(
                "storybook".into(),
                implementation(&root.join("packages/ui"), None),
            )],
            pages: vec![(
                "button".into(),
                page("Button", Some(globals), vec![("storybook", story)]),
            )],
        };
        let result = create(root, &view, &ExportOptions::default()).expect("export");
        for expected in [
            "packages/ui/src/styles/theme.css",
            "packages/ui/src/components/icons-next/assets/plus.svg",
            "packages/ui/src/components/progress-bar/types/progress-bar-props.ts",
        ] {
            assert!(
                result.report.files.contains(&expected.to_owned()),
                "{expected}: {:?}",
                result.report.files
            );
            assert!(
                result.report.pages[0].files.contains(&expected.to_owned()),
                "{expected} in the page"
            );
        }
        assert!(
            !result
                .report
                .warnings
                .iter()
                .any(|warning| warning.contains("@acme/ui/theme.css")
                    || warning.contains("progress-bar-props.js")),
            "{:?}",
            result.report.warnings
        );
        let before = result.report.pages[0].hash.clone();
        fs::write(theme, ":root { --color-primary: red; }\n").expect("write");
        assert_ne!(
            create(root, &view, &ExportOptions::default())
                .expect("export")
                .report
                .pages[0]
                .hash,
            before
        );
    }

    #[test]
    fn exports_custom_storybook_config_and_assets_discovered_through_a_linked_package_plugin() {
        let project = Project::new("storybook");
        let root = &project.0;
        project.write("workbench.yaml", "name: Sprite\n");
        project.write("apps/storybook/package.json", r#"{"name":"@acme/storybook","scripts":{"storybook":"storybook dev --config-dir web --port 6006"},"dependencies":{"@acme/ui":"link:../../libs/ui"}}"#);
        project.write("apps/storybook/web/main.ts", "import { iconSpriteOptions } from '@acme/ui/vite';\nexport default iconSpriteOptions();\n");
        project.write(
            "apps/storybook/web/preview.ts",
            "import 'virtual:svg-icons-register';\n",
        );
        project.write(
            "libs/ui/package.json",
            r#"{"name":"@acme/ui","exports":{"./vite":{"default":"./vite/index.mjs"}}}"#,
        );
        project.write(
            "libs/ui/vite/index.mjs",
            "export const icons = new URL('../src/icons/assets/', import.meta.url);\n",
        );
        let story = project.write(
            "libs/ui/src/button.stories.tsx",
            "import { Icon } from './icon';\nexport const Play = () => Icon('play');\n",
        );
        project.write(
            "libs/ui/src/icon.tsx",
            "export const Icon = name => '<use href=#icon-' + name + ' />';\n",
        );
        let play = project.write("libs/ui/src/icons/assets/play.svg", "<svg id=\"play\"/>");
        project.write("libs/ui/src/icons/assets/skip.svg", "<svg id=\"skip\"/>");
        let view = ExportView {
            name: Some("Sprite".into()),
            implementations: vec![(
                "storybook".into(),
                implementation(&root.join("apps/storybook"), Some("storybook")),
            )],
            pages: vec![(
                "button".into(),
                page("Button", None, vec![("storybook", story)]),
            )],
        };
        let result = create(root, &view, &ExportOptions::default()).expect("export");
        for expected in [
            "apps/storybook/web/main.ts",
            "apps/storybook/web/preview.ts",
            "libs/ui/package.json",
            "libs/ui/vite/index.mjs",
            "libs/ui/src/icons/assets/play.svg",
            "libs/ui/src/icons/assets/skip.svg",
        ] {
            assert!(
                result.report.files.contains(&expected.to_owned()),
                "{expected}: {:?}",
                result.report.files
            );
        }
        assert!(
            result.report.pages[0]
                .files
                .contains(&"libs/ui/src/icons/assets/play.svg".to_owned())
        );
        assert!(
            !result
                .report
                .dependencies
                .iter()
                .any(|dependency| dependency.name == "@acme/ui"
                    || dependency.name == "virtual:svg-icons-register")
        );
        let before = result.report.pages[0].hash.clone();
        fs::write(play, "<svg id=\"play-changed\"/>").expect("write");
        assert_ne!(
            create(root, &view, &ExportOptions::default())
                .expect("export")
                .report
                .pages[0]
                .hash,
            before
        );
    }

    #[test]
    fn a_portable_export_ships_its_viewer_and_sdk_types_and_refuses_paths_outside_browser() {
        let project = Project::new("portable");
        project.write("workbench.yaml", "name: Acme\n");
        let product = Project::new("product");
        product.write(
            "sdk/api.d.ts",
            "export declare function definePreview(): void;",
        );
        let file = |path: &str| PortableFile {
            path: path.into(),
            data: base64::engine::general_purpose::STANDARD.encode("viewer"),
        };
        let portable = PortableExport {
            files: vec![file("browser/index.html")],
            warnings: vec!["Card: build failed".into()],
            ..PortableExport::default()
        };
        let view = ExportView {
            name: Some("Acme".into()),
            ..ExportView::default()
        };
        let result = create(
            &project.0,
            &view,
            &ExportOptions {
                portable: Some(portable.clone()),
                product: Some(product.0.clone()),
                ..ExportOptions::default()
            },
        )
        .expect("export");
        let contents = bodies(&result.download().body);
        assert_eq!(contents["acme-design-system/browser/index.html"], "viewer");
        assert!(
            contents["acme-design-system/workbench-env.d.ts"]
                .contains("declare module \"@canonic2/workbench\"")
        );
        assert!(contents["acme-design-system/README.md"].contains("## Portable browser previews"));
        assert_eq!(
            result
                .report
                .browser
                .as_ref()
                .map(|browser| browser.warnings.clone()),
            Some(vec!["Card: build failed".to_owned()])
        );
        let outside = PortableExport {
            files: vec![file("browser/../escape.js")],
            ..portable
        };
        let error = create(
            &project.0,
            &view,
            &ExportOptions {
                portable: Some(outside),
                product: Some(product.0.clone()),
                ..ExportOptions::default()
            },
        )
        .expect_err("outside browser/");
        assert_eq!(error.to_string(), "Invalid portable export path");
    }

    #[test]
    fn unresolved_references_and_missing_files_are_warnings() {
        let project = Project::new("warnings");
        project.write("workbench.yaml", "name: Acme\n");
        let app = project.write(
            "app.ts",
            "import './missing';\nimport 'https://example.com/x.js';\n",
        );
        let view = ExportView {
            name: Some("Acme".into()),
            implementations: vec![],
            pages: vec![
                ("app".into(), page("App", None, vec![("web", app)])),
                (
                    "gone".into(),
                    page("Gone", Some(project.0.join("gone.html")), vec![]),
                ),
            ],
        };
        let result = create(&project.0, &view, &ExportOptions::default()).expect("export");
        assert_eq!(
            result.report.warnings,
            [
                "Could not resolve “./missing” from app.ts".to_owned(),
                format!(
                    "Missing design: Gone: {}",
                    display(&project.0.join("gone.html"))
                ),
            ]
        );
        let gone = result
            .report
            .pages
            .iter()
            .find(|page| page.id == "gone")
            .expect("page");
        assert_eq!(gone.readme, "workbench-pages/gone/README.md");
    }
}
