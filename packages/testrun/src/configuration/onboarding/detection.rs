//! Static manifest inspection; discovery never invokes build tools or project code.
use super::*;
mod paging;
pub(super) use paging::page;
const SKIP: &[&str] = &[
    "node_modules",
    "Pods",
    "build",
    "dist",
    "target",
    "vendor",
    "coverage",
];
struct Scan {
    entries: usize,
    bytes: usize,
    result: Discovery,
}
impl Scan {
    fn new(root: PathBuf) -> Self {
        Self {
            entries: 0,
            bytes: 0,
            result: Discovery {
                root,
                candidates: Vec::new(),
                warnings: Vec::new(),
                truncated: false,
            },
        }
    }

    /// A native/web directory below a framework's platform output belongs to
    /// that framework. Check only ancestors of a candidate admitted to this
    /// page, so continuation does not need to retain or rescan prior results.
    fn generated(&mut self, candidate: &Candidate) -> bool {
        let platforms = match candidate.kind.as_str() {
            "xcode" => &["ios", "macos"][..],
            "gradle" | "android-studio" => &["android"][..],
            "web" => &["web"][..],
            _ => return false,
        };
        for ancestor in Path::new(&candidate.path).ancestors() {
            if ancestor
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| platforms.contains(&name))
            {
                let parent = self
                    .result
                    .root
                    .join(ancestor.parent().unwrap_or_else(|| Path::new("")));
                if self
                    .classify(&parent, &BTreeSet::new())
                    .is_some_and(|parent| {
                        ["expo", "react-native", "flutter"].contains(&parent.kind.as_str())
                    })
                {
                    return true;
                }
            }
        }
        false
    }
    fn manifest(&mut self, path: &Path) -> Option<String> {
        let meta = fs::symlink_metadata(path).ok()?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return None;
        }
        if meta.len() > 262_144 || self.bytes.saturating_add(meta.len() as usize) > 4_194_304 {
            self.result.truncated = true;
            self.result.warnings.push(format!(
                "Manifest read limit: {}",
                path.strip_prefix(&self.result.root)
                    .unwrap_or(path)
                    .display()
            ));
            return None;
        }
        let mut text = String::new();
        fs::File::open(path)
            .ok()?
            .take(262_145)
            .read_to_string(&mut text)
            .ok()?;
        self.bytes += text.len();
        if text.len() > 262_144 {
            self.result.truncated = true;
            return None;
        }
        Some(text)
    }
    fn classify_package_json(
        &mut self,
        path: &Path,
        names: &BTreeSet<String>,
        candidate: &mut Candidate,
    ) {
        if let Some(text) = self.manifest(&path.join("package.json")) {
            match serde_json::from_str::<Value>(&text) {
                Ok(package) => {
                    let has = |name: &str| {
                        ["dependencies", "devDependencies", "peerDependencies"]
                            .iter()
                            .any(|section| package[*section].get(name).is_some())
                    };
                    candidate.kind = if has("expo") {
                        "expo"
                    } else if has("react-native") {
                        "react-native"
                    } else if has("react") {
                        "react"
                    } else if has("vue") {
                        "vue"
                    } else if has("vite") {
                        "vite"
                    } else if names.contains("index.html") {
                        "web"
                    } else {
                        ""
                    }
                    .into();
                    if !candidate.kind.is_empty() {
                        candidate.evidence.push("package.json".into());
                    }
                }
                Err(_) => self
                    .result
                    .warnings
                    .push(format!("Invalid package.json: {}", candidate.path)),
            }
        }
    }

    fn classify(&mut self, path: &Path, names: &BTreeSet<String>) -> Option<Candidate> {
        let relative = path.strip_prefix(&self.result.root).ok()?;
        let root = if relative.as_os_str().is_empty() {
            ".".into()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        };
        let mut candidate = Candidate {
            path: root,
            kind: String::new(),
            platforms: Vec::new(),
            evidence: Vec::new(),
        };
        self.classify_package_json(path, names, &mut candidate);
        if candidate.kind.is_empty()
            && let Some(text) = self.manifest(&path.join("pubspec.yaml"))
        {
            let options = serde_saphyr::options! {duplicate_keys:serde_saphyr::DuplicateKeyPolicy::Error,merge_keys:serde_saphyr::MergeKeyPolicy::Error,reject_unsupported_tags:true,budget:serde_saphyr::budget!{max_depth:32,max_nodes:10000,max_aliases:0,max_anchors:0,max_total_scalar_bytes:262144}};
            match serde_saphyr::from_str_with_options::<Value>(&text, options) {
                Ok(pubspec) if pubspec["dependencies"]["flutter"]["sdk"] == "flutter" => {
                    candidate.kind = "flutter".into();
                    candidate.evidence.push("pubspec.yaml".into());
                }
                Err(_) => self
                    .result
                    .warnings
                    .push(format!("Invalid pubspec.yaml: {}", candidate.path)),
                _ => {}
            }
        }
        if candidate.kind.is_empty() {
            if let Some(marker) = names
                .iter()
                .find(|name| name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace"))
            {
                candidate.kind = "xcode".into();
                candidate.evidence.push(marker.clone());
            } else if [
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
                "gradlew",
                "gradlew.bat",
            ]
            .iter()
            .any(|name| names.contains(*name))
            {
                candidate.kind = "gradle".into();
                candidate.evidence.extend(
                    names
                        .iter()
                        .filter(|name| {
                            [
                                "build.gradle",
                                "build.gradle.kts",
                                "settings.gradle",
                                "settings.gradle.kts",
                                "gradlew",
                                "gradlew.bat",
                            ]
                            .contains(&name.as_str())
                        })
                        .cloned(),
                );
                let manifest = path.join("src/main/AndroidManifest.xml").is_file()
                    || path.join("app/src/main/AndroidManifest.xml").is_file();
                if manifest {
                    candidate.kind = "android-studio".into();
                    candidate.platforms.push("android".into());
                }
            }
        }
        if candidate.kind.is_empty() && names.contains("index.html") {
            candidate.kind = "web".into();
            candidate.evidence.push("index.html".into());
        }
        if candidate.kind.is_empty() {
            return None;
        }
        if ["react", "vue", "vite", "web"].contains(&candidate.kind.as_str()) {
            candidate.platforms.push("web".into());
        }
        if ["expo", "react-native", "flutter"].contains(&candidate.kind.as_str()) {
            for platform in ["ios", "android", "macos", "windows", "linux", "web"] {
                if path.join(platform).is_dir() {
                    candidate.platforms.push(platform.into());
                }
            }
        }
        Some(candidate)
    }
    fn directory(&mut self, path: &Path, depth: usize, recurse: bool) -> Result<()> {
        if self.entries >= 4096 || self.result.candidates.len() >= 128 {
            self.result.truncated = true;
            return Ok(());
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) if depth > 0 => {
                self.result.warnings.push(format!(
                    "Directory unreadable: {}",
                    path.strip_prefix(&self.result.root)
                        .unwrap_or(path)
                        .display()
                ));
                return Ok(());
            }
            Err(_) => return Err(Error::new("CONFIG_IO", "Cannot inspect project directory")),
        };
        let mut children = Vec::new();
        let mut names = BTreeSet::new();
        for entry in entries {
            self.entries += 1;
            if self.entries > 4096 {
                self.result.truncated = true;
                break;
            }
            let entry =
                entry.map_err(|_| Error::new("CONFIG_IO", "Cannot inspect project entry"))?;
            let kind = entry
                .file_type()
                .map_err(|_| Error::new("CONFIG_IO", "Cannot inspect project entry type"))?;
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            names.insert(name.clone());
            if kind.is_dir()
                && !name.starts_with('.')
                && !SKIP.contains(&name.as_str())
                && !name.ends_with(".xcodeproj")
                && !name.ends_with(".xcworkspace")
            {
                children.push(entry.path());
            }
        }
        if let Some(candidate) = self.classify(path, &names) {
            self.result.candidates.push(candidate);
        }
        children.sort();
        if recurse {
            if depth == 6 {
                if !children.is_empty() {
                    self.result.truncated = true;
                }
            } else {
                for child in children {
                    self.directory(&child, depth + 1, true)?;
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn discover(root: &Path, paths: &[PathBuf]) -> Result<Discovery> {
    let root = root
        .canonicalize()
        .map_err(|_| Error::new("CONFIG_PATH", "Project root is unavailable"))?;
    let mut scan = Scan::new(root.clone());
    if paths.is_empty() {
        scan.directory(&root, 0, true)?;
    } else {
        let mut seen = BTreeSet::new();
        for path in paths {
            let path = root.join(path).canonicalize().map_err(|_| {
                Error::new(
                    "CONFIG_PATH",
                    "Selected application directory is unavailable",
                )
            })?;
            if !path.starts_with(&root) || !path.is_dir() {
                return Err(Error::invalid(
                    "Application paths must be directories inside the selected root",
                ));
            }
            if !seen.insert(path.clone()) {
                continue;
            }
            let before = scan.result.candidates.len();
            scan.directory(&path, 0, false)?;
            if scan.result.candidates.len() == before {
                scan.result.candidates.push(Candidate {
                    path: if path == root {
                        ".".into()
                    } else {
                        path.strip_prefix(&root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    },
                    kind: "custom".into(),
                    platforms: Vec::new(),
                    evidence: Vec::new(),
                });
            }
        }
    }
    let parents = scan.result.candidates.clone();
    if paths.is_empty() {
        scan.result.candidates.retain(|candidate| {
            !parents.iter().any(|parent| {
                ["expo", "react-native", "flutter"].contains(&parent.kind.as_str())
                    && ["xcode", "gradle", "android-studio", "web"]
                        .contains(&candidate.kind.as_str())
                    && candidate.path != parent.path
                    && (if candidate.kind == "xcode" {
                        &["ios", "macos"][..]
                    } else if candidate.kind == "web" {
                        &["web"][..]
                    } else {
                        &["android"][..]
                    })
                    .iter()
                    .any(|platform| {
                        Path::new(&candidate.path).starts_with(
                            Path::new(if parent.path == "." { "" } else { &parent.path })
                                .join(platform),
                        )
                    })
            })
        });
    }
    scan.result.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(scan.result)
}
