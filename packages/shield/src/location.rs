//! Request-scoped filesystem observations for explicit resource predicates.
//! Policy passes this observer explicitly; no observation authorizes execution.
use crate::policy::{Error, normalized::Input, patterns::Provider, selectors};
use serde_json::Value;
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

mod node;
mod targets;
mod text;
mod working_files;
mod writers;
pub(crate) use working_files::WorkingFiles;

/// The single source file read by a supported Node syntax check.
pub(crate) fn node_check_file<'a>(executable: &str, arguments: &[&'a str]) -> Option<&'a str> {
    node::check_file(executable, arguments)
}

/// Whether the submitted fixed text-filter syntax has only read effects.
/// Every file a modeled text filter names, with whether it writes it, such as
/// sed's in-place inputs and the files its script writes or reads.
pub(crate) fn text_files(executable: &str, arguments: &[&str]) -> Option<Vec<(bool, String)>> {
    text::files(
        executable,
        &arguments
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>(),
    )
}

/// Whether a configuration or credentials file that the client loads by
/// default exists in the home folder. Any entry counts, including a dangling link.
pub(crate) fn http_client_config(
    home: &Path,
    client: crate::http_client::Client,
    netrc: bool,
) -> bool {
    let names: &[&str] = match client {
        crate::http_client::Client::Curl => &[".curlrc", ".config/.curlrc", ".config/curlrc"],
        crate::http_client::Client::Wget => &[".wgetrc"],
    };
    names
        .iter()
        .chain(netrc.then_some(&".netrc"))
        .any(|name| fs::symlink_metadata(home.join(name)).is_ok())
}

pub(crate) fn text_read_only(executable: &str, arguments: &[&str]) -> bool {
    text::read_only(
        executable,
        &arguments
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[cfg(unix)]
type EntryIdentity = Result<Option<(u64, u64)>, ()>;

#[derive(Clone)]
struct Target {
    action: &'static str,
    role: &'static str,
    path: PathBuf,
    recursive: bool,
    descendants: Descendants,
    follow: bool,
}
/// Which descendants of a recursive target its operation can reach.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Descendants {
    /// Every path below the target, including paths created later.
    All,
    /// Only descendants without a hidden name component.
    Visible,
    /// Only entries that exist now, as in a walk that lists without creating.
    Present,
}
struct Targets {
    items: Vec<Target>,
    complete: bool,
}

pub(crate) struct Observer {
    input: Input,
    project: PathBuf,
    cwd: PathBuf,
    executable: Option<PathBuf>,
    access_paths: crate::configuration::AccessPaths,
    working_files: Option<WorkingFiles>,
    cache: RefCell<BTreeMap<(PathBuf, bool), Option<PathBuf>>>,
    directories: RefCell<BTreeMap<PathBuf, bool>>,
    #[cfg(unix)]
    entries: RefCell<BTreeMap<PathBuf, EntryIdentity>>,
    targets: OnceCell<Targets>,
    observations: Cell<usize>,
    unverified: Cell<bool>,
}
impl Observer {
    /// Inspect a bounded, physical directory footprint when ignore/glob rules
    /// could expose hidden descendants. No file contents or links are followed.
    pub(crate) fn search_entries(&self, mut visit: impl FnMut(&Path) -> bool) -> Option<()> {
        let Input::Argv { arguments, .. } = &self.input else {
            return None;
        };
        let (targets, listing) = targets::rg_model(arguments, &self.cwd);
        if !targets.complete {
            return None;
        }
        if listing {
            return Some(());
        }
        let mut pending = Vec::new();
        for target in targets.items {
            if target.recursive && matches!(target.descendants, Descendants::Present) {
                pending.push((target.path, 0));
            }
        }
        let mut remaining = 1024;
        while let Some((directory, depth)) = pending.pop() {
            if depth >= 32 {
                return None;
            }
            for entry in fs::read_dir(directory).ok()? {
                if remaining == 0 {
                    return None;
                }
                remaining -= 1;
                let entry = entry.ok()?;
                let path = entry.path();
                if !visit(&path) {
                    return Some(());
                }
                if entry.file_type().ok()?.is_dir() {
                    pending.push((path, depth + 1));
                }
            }
        }
        Some(())
    }

    /// Whether a modeled ripgrep content search names only its pattern, so it
    /// reads its standard input when that is a pipe.
    pub(crate) fn search_reads_input(&self) -> bool {
        let Input::Argv { arguments, .. } = &self.input else {
            return false;
        };
        targets::rg_pattern_only(arguments, &self.cwd)
    }

    /// Conservative content footprint for modeled ripgrep requests. Listings do
    /// not read file contents; ignore files and globs may expose hidden entries.
    pub(crate) fn search_reaches(&self, excluded: &Path) -> Option<bool> {
        let Input::Argv { arguments, .. } = &self.input else {
            return None;
        };
        let (targets, listing) = targets::rg_model(arguments, &self.cwd);
        if !targets.complete {
            return None;
        }
        if listing {
            return Some(false);
        }
        for target in targets.items {
            let path = self.observed(&target.path, target.follow)?;
            let excluded = self.observed(excluded, target.follow)?;
            let descendant = target.recursive && excluded.starts_with(&path)
                && match target.descendants {
                    Descendants::All => true,
                    Descendants::Visible => !excluded.strip_prefix(&path).ok()?.components().any(|part| {
                        matches!(part, Component::Normal(name) if name.as_encoded_bytes().starts_with(b"."))
                    }),
                    Descendants::Present => self.present(&excluded).ok()?,
                };
            if path.starts_with(&excluded)
                || descendant
                || self.same_entry(&path, &excluded).ok()?
            {
                return Some(true);
            }
        }
        Some(false)
    }

    pub(crate) fn readable_roots(
        project: &Path,
        cwd: &Path,
        paths: &crate::configuration::AccessPaths,
    ) -> Option<Vec<PathBuf>> {
        let input = Input::File {
            action: "read".into(),
            sources: vec![],
            destination: None,
            recursive: false,
            original_input: serde_json::json!({}),
        };
        Self::new(&input, project.to_str()?, cwd.to_str()?, None)
            .with_access_paths(paths)
            .reference(&serde_json::json!({"root":"read"}), true)
    }

    pub(crate) fn new(input: &Input, project: &str, cwd: &str, executable: Option<&str>) -> Self {
        Self {
            input: input.clone(),
            project: project.into(),
            cwd: cwd.into(),
            executable: executable.map(PathBuf::from),
            access_paths: crate::configuration::AccessPaths::default(),
            working_files: None,
            cache: RefCell::new(BTreeMap::new()),
            directories: RefCell::new(BTreeMap::new()),
            #[cfg(unix)]
            entries: RefCell::new(BTreeMap::new()),
            targets: OnceCell::new(),
            observations: Cell::new(0),
            unverified: Cell::new(false),
        }
    }
    pub(crate) fn with_access_paths(mut self, paths: &crate::configuration::AccessPaths) -> Self {
        self.access_paths = paths.clone();
        self
    }

    pub(crate) fn with_working_files(mut self, scope: Option<&WorkingFiles>) -> Self {
        self.working_files = scope.cloned();
        self
    }
    fn observed(&self, path: &Path, follow: bool) -> Option<PathBuf> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        let key = (path.clone(), follow);
        if let Some(value) = self.cache.borrow().get(&key) {
            return value.clone();
        }
        let value = resolve(&path, follow, &mut 0, &self.observations);
        if value.is_none() {
            self.unverified.set(true);
        }
        self.cache.borrow_mut().insert(key, value.clone());
        value
    }
    fn reference(&self, reference: &Value, follow: bool) -> Option<Vec<PathBuf>> {
        let suffix = reference["path"].as_str().unwrap_or(".");
        let mut paths = match reference["root"].as_str()? {
            "absolute" => vec![PathBuf::from(reference["path"].as_str()?)],
            "project" => vec![self.project.join(suffix)],
            "read" | "write" => {
                let mut roots = vec![self.project.join(suffix)];
                if let Some(scope) = &self.working_files {
                    for root in scope.roots(reference["root"] == "read") {
                        let base = self.observed(root, true)?;
                        let path = self.observed(&root.join(suffix), follow)?;
                        if base != *root || !self.directory(&base) || !path.starts_with(root) {
                            self.unverified.set(true);
                            return None;
                        }
                        roots.push(path);
                    }
                }
                let additional = self.access_paths.write.iter().chain(
                    self.access_paths
                        .read
                        .iter()
                        .filter(|_| reference["root"] == "read"),
                );
                for path in additional {
                    let Some(base) = self
                        .observed(Path::new(path), true)
                        .filter(|p| self.directory(p))
                    else {
                        self.unverified.set(true);
                        return None;
                    };
                    roots.push(base.join(suffix));
                }
                roots
            }
            _ => return None,
        };
        // Verify all configured roots before testing their union. An unavailable
        // root cannot prove the complement or silently weaken an exclusion.
        for path in &mut paths {
            *path = self.observed(path, follow)?;
        }
        paths.sort();
        paths.dedup();
        Some(paths)
    }
    fn directory(&self, path: &Path) -> bool {
        if let Some(directory) = self.directories.borrow().get(path) {
            return *directory;
        }
        self.observations
            .set(self.observations.get().saturating_add(1));
        let directory = self.observations.get() <= 8192 && path.is_dir();
        if !directory {
            self.unverified.set(true);
        }
        self.directories.borrow_mut().insert(path.into(), directory);
        directory
    }
    pub(crate) fn executable(&self, reference: &Value) -> Option<&'static str> {
        let path = self.executable.clone().or_else(|| match &self.input {
            Input::Argv { executable, .. } if executable.contains(std::path::MAIN_SEPARATOR) => {
                self.observed(Path::new(executable), true)
            }
            _ => None,
        });
        let Some(path) = path else {
            self.unverified.set(true);
            return Some("evidence_unavailable");
        };
        let Some(expected) = self.reference(reference, true) else {
            return Some("evidence_unavailable");
        };
        if !fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            self.unverified.set(true);
            return Some("evidence_unavailable");
        }
        (!expected.contains(&path)).then_some("predicate_mismatch")
    }
    pub(crate) fn cwd(
        &self,
        predicate: &Value,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        let Some(path) = self.observed(&self.cwd, true) else {
            return Ok(Some("evidence_unavailable"));
        };
        if !path.is_dir() {
            self.unverified.set(true);
            return Ok(Some("evidence_unavailable"));
        }
        self.path(
            predicate,
            &Target {
                action: "read",
                role: "cwd",
                path,
                recursive: false,
                descendants: Descendants::All,
                follow: true,
            },
            patterns,
        )
    }
    fn path(
        &self,
        predicate: &Value,
        target: &Target,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        let Some(path) = self.observed(&target.path, target.follow) else {
            return Ok(Some("evidence_unavailable"));
        };
        let (kind, reference) = ["exact", "within", "outside"]
            .iter()
            .find_map(|key| predicate.get(*key).map(|v| (*key, v)))
            .ok_or(Error::Syntax)?;
        let Some(roots) = self.reference(reference, kind != "exact" || target.follow) else {
            return Ok(Some("evidence_unavailable"));
        };
        let inside = roots.iter().any(|root| path.starts_with(root));
        let matched = match kind {
            "exact" => roots.contains(&path),
            "within" => inside,
            "outside" => !inside,
            _ => false,
        };
        if !matched {
            return Ok(Some("location_outside"));
        }
        for reference in predicate["exclude"].as_array().into_iter().flatten() {
            let Some(exclusions) = self.reference(reference, target.follow) else {
                return Ok(Some("evidence_unavailable"));
            };
            for excluded in exclusions {
                let reached = target.recursive
                    && excluded.starts_with(&path)
                    && match target.descendants {
                        Descendants::All => true,
                        Descendants::Visible => !excluded.strip_prefix(&path).is_ok_and(|relative| {
                            relative.components().any(|part| matches!(part, Component::Normal(name) if name.as_encoded_bytes().starts_with(b".")))
                        }),
                        Descendants::Present => match self.present(&excluded) {
                            Ok(present) => present,
                            Err(()) => return Ok(Some("evidence_unavailable")),
                        },
                    };
                if path.starts_with(&excluded) || reached {
                    return Ok(Some("location_excluded"));
                }
                match self.same_entry(&path, &excluded) {
                    Ok(true) => return Ok(Some("location_excluded")),
                    Err(()) => return Ok(Some("evidence_unavailable")),
                    Ok(false) => {}
                }
            }
        }
        if let Some(selector) = predicate.get("match") {
            let mut selected = false;
            for root in &roots {
                if let Some(relative) = path.strip_prefix(root).ok().and_then(Path::to_str) {
                    selected |= selectors::matches(
                        selector,
                        &relative.replace(std::path::MAIN_SEPARATOR, "/"),
                        patterns,
                    )?;
                }
            }
            if !selected {
                return Ok(Some("predicate_mismatch"));
            }
        }
        Ok(None)
    }
    fn present(&self, path: &Path) -> Result<bool, ()> {
        #[cfg(unix)]
        {
            Ok(self.entry(path)?.is_some())
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(())
        }
    }
    fn same_entry(&self, a: &Path, b: &Path) -> Result<bool, ()> {
        #[cfg(unix)]
        {
            let (a, b) = (self.entry(a)?, self.entry(b)?);
            Ok(a.is_some() && a == b)
        }
        #[cfg(not(unix))]
        {
            let _ = (a, b);
            Ok(false)
        }
    }
    #[cfg(unix)]
    fn entry(&self, path: &Path) -> EntryIdentity {
        use std::os::unix::fs::MetadataExt;
        if let Some(identity) = self.entries.borrow().get(path) {
            return *identity;
        }
        self.observations
            .set(self.observations.get().saturating_add(1));
        let identity = if self.observations.get() > 8192 {
            Err(())
        } else {
            match fs::symlink_metadata(path) {
                Ok(metadata) => Ok(Some((metadata.dev(), metadata.ino()))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(()),
            }
        };
        if identity.is_err() {
            self.unverified.set(true);
        }
        self.entries
            .borrow_mut()
            .insert(path.to_path_buf(), identity);
        identity
    }
    pub(crate) fn predicate(
        &self,
        key: &str,
        predicate: &Value,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        let targets = self.targets.get_or_init(|| self.collect());
        if !targets.complete || targets.items.is_empty() {
            self.unverified.set(true);
        }
        if key == "targets" {
            let predicates: Vec<_> = predicate
                .as_array()
                .map_or_else(|| vec![predicate], |v| v.iter().collect());
            for item in predicates {
                let action = item["action"].as_str().ok_or(Error::Syntax)?;
                if let Some(failure) = self.quantified(
                    item,
                    targets.items.iter().filter(|t| t.action == action),
                    targets.complete,
                    patterns,
                )? {
                    return Ok(Some(failure));
                }
            }
            Ok(None)
        } else if key == "sources" {
            self.quantified(
                predicate,
                targets.items.iter().filter(|t| t.role == "source"),
                targets.complete,
                patterns,
            )
        } else {
            let destinations: Vec<_> = targets
                .items
                .iter()
                .filter(|t| t.role == "destination" || t.role == "replacement")
                .collect();
            if !targets.complete || destinations.is_empty() {
                return Ok(Some("evidence_unavailable"));
            }
            for destination in destinations {
                if let Some(failure) = self.path(predicate, destination, patterns)? {
                    return Ok(Some(failure));
                }
            }
            Ok(None)
        }
    }
    fn quantified<'a>(
        &self,
        predicate: &Value,
        items: impl Iterator<Item = &'a Target>,
        complete: bool,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        let all = predicate.get("all").is_some();
        let selector = predicate
            .get(if all { "all" } else { "any" })
            .ok_or(Error::Syntax)?;
        let items: Vec<_> = items.collect();
        if items.is_empty() {
            return Ok(Some("evidence_unavailable"));
        }
        let mut unknown = !complete;
        let mut failure = None;
        for item in items {
            match self.path(selector, item, patterns)? {
                None if !all => return Ok(None),
                Some("evidence_unavailable") => unknown = true,
                Some(code) => failure = Some(code),
                _ => {}
            }
        }
        if all && failure.is_some() {
            return Ok(failure);
        }
        if unknown {
            return Ok(Some("evidence_unavailable"));
        }
        Ok(if all {
            None
        } else {
            Some("predicate_mismatch")
        })
    }
    fn collect(&self) -> Targets {
        let alone = self
            .working_files
            .as_ref()
            .is_some_and(|scope| scope.sole_operation);
        targets::collect(&self.input, &self.cwd, alone)
    }

    pub(crate) fn ensure_budget(&self) -> Result<(), Error> {
        if self.observations.get() > 8192 {
            Err(Error::Limit)
        } else {
            Ok(())
        }
    }
    pub(crate) fn complete(&self) -> bool {
        !self.unverified.get()
    }

    pub(crate) fn coverage(&self, predicates: &[&Value]) -> bool {
        let targets = self.targets.get_or_init(|| self.collect());
        targets.complete
            && !targets.items.is_empty()
            && targets.items.iter().all(|item| {
                predicates.iter().any(|p| {
                    let selected = &p["targets"];
                    std::iter::once(selected)
                        .chain(selected.as_array().into_iter().flatten())
                        .any(|t| t["action"] == item.action && t.get("all").is_some())
                })
            })
    }
}

impl crate::policy::resources::Resources for Observer {
    fn argument_path(
        &self,
        path: &str,
        predicate: &Value,
        recursive: bool,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        if path.is_empty() || path == "-" || path.contains('\0') {
            return Ok(Some("evidence_unavailable"));
        }
        let Some(path) = self.observed(Path::new(path), true) else {
            return Ok(Some("evidence_unavailable"));
        };
        self.path(
            predicate,
            &Target {
                action: "read",
                role: "argument",
                path,
                recursive,
                descendants: Descendants::All,
                follow: true,
            },
            patterns,
        )
    }
    fn executable(&self, reference: &Value) -> Option<&'static str> {
        Self::executable(self, reference)
    }
    fn cwd(
        &self,
        predicate: &Value,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        Self::cwd(self, predicate, patterns)
    }
    fn predicate(
        &self,
        key: &str,
        predicate: &Value,
        patterns: &dyn Provider,
    ) -> Result<Option<&'static str>, Error> {
        Self::predicate(self, key, predicate, patterns)
    }
    fn coverage(&self, predicates: &[&Value]) -> bool {
        Self::coverage(self, predicates)
    }
    fn ensure_budget(&self) -> Result<(), Error> {
        Self::ensure_budget(self)
    }
}

// Follow each existing parent before processing '..'. New suffixes remain
// anchored to their last observed ancestor; failures never become lexical proof.
fn resolve(
    path: &Path,
    follow_leaf: bool,
    links: &mut usize,
    observations: &Cell<usize>,
) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let components: Vec<_> = path.components().collect();
    let mut result = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        observations.set(observations.get().saturating_add(1));
        if observations.get() > 8192 {
            return None;
        }
        match component {
            Component::Prefix(_) | Component::RootDir => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => {
                result.push(part);
                match fs::symlink_metadata(&result) {
                    Ok(metadata)
                        if metadata.file_type().is_symlink()
                            && (follow_leaf || index + 1 < components.len()) =>
                    {
                        *links += 1;
                        if *links > 40 {
                            return None;
                        }
                        let target = fs::read_link(&result).ok()?;
                        let target = if target.is_absolute() {
                            target
                        } else {
                            result.parent()?.join(target)
                        };
                        result = resolve(&target, true, links, observations)?;
                        // A dangling link is not a verified new destination.
                        fs::symlink_metadata(&result).ok()?;
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return None,
                }
            }
        }
    }
    match fs::symlink_metadata(&result) {
        Ok(metadata) if !follow_leaf && metadata.file_type().is_symlink() => Some(
            fs::canonicalize(result.parent()?)
                .ok()?
                .join(result.file_name()?),
        ),
        Ok(_) => fs::canonicalize(result).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut ancestor = result.as_path();
            while fs::symlink_metadata(ancestor).is_err() {
                ancestor = ancestor.parent()?;
            }
            Some(
                fs::canonicalize(ancestor)
                    .ok()?
                    .join(result.strip_prefix(ancestor).ok()?),
            )
        }
        Err(_) => None,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn observer(root: &Path) -> Observer {
        let root_text = root.to_str().expect("UTF-8 temporary fixture path");
        Observer::new(
            &Input::File {
                action: "read".into(),
                sources: vec!["target".into()],
                destination: None,
                recursive: false,
                original_input: serde_json::json!({}),
            },
            root_text,
            root_text,
            None,
        )
    }

    #[test]
    fn repeated_alias_checks_share_observations_only_inside_one_request() {
        let root = assert_fs::TempDir::new().unwrap();
        let target = root.path().join("target");
        let excluded = root.path().join("excluded");
        fs::write(&target, "Acme").unwrap();
        fs::hard_link(&target, &excluded).unwrap();
        let request = observer(root.path());
        for _ in 0..1000 {
            assert_eq!(request.same_entry(&target, &excluded), Ok(true));
        }
        assert_eq!(request.observations.get(), 2);
        fs::remove_file(&excluded).unwrap();
        fs::write(&excluded, "Acme replacement").unwrap();
        assert_eq!(request.same_entry(&target, &excluded), Ok(true));
        assert_eq!(
            observer(root.path()).same_entry(&target, &excluded),
            Ok(false)
        );
    }

    #[test]
    fn alias_observation_budget_exhaustion_is_unknown_and_fails_evaluation_budget() {
        let root = assert_fs::TempDir::new().unwrap();
        let request = observer(root.path());
        request.observations.set(8192);
        assert_eq!(request.same_entry(root.path(), root.path()), Err(()));
        assert!(!request.complete());
        assert!(matches!(request.ensure_budget(), Err(Error::Limit)));
    }
}
