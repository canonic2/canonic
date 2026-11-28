//! Build input selection and successful artifact reuse, independent of Cargo I/O.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn inputs(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    fn visit(path: &Path, extensions: &[&str], files: &mut Vec<PathBuf>) {
        let name = path.file_name().unwrap().to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "xcuserdata" | "build") {
            return;
        }
        if path.is_dir() {
            for entry in std::fs::read_dir(path).expect("Read build inputs") {
                visit(&entry.expect("Build input").path(), extensions, files);
            }
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
        {
            files.push(path.into());
        }
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).expect("Read build input directory") {
        visit(&entry.expect("Build input").path(), extensions, &mut files);
    }
    files.sort();
    files
}

/// Local path dependencies compiled into `package`, followed transitively.
/// Development-only dependencies never reach the binary and are excluded.
pub fn local_dependencies(package: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut pending = vec![package.to_path_buf()];
    while let Some(root) = pending.pop() {
        let manifest =
            std::fs::read_to_string(root.join("Cargo.toml")).expect("Read package manifest");
        for path in dependency_paths(&manifest) {
            let dependency = root.join(path);
            if !found.contains(&dependency) {
                found.push(dependency.clone());
                pending.push(dependency);
            }
        }
    }
    found.sort();
    found
}

/// A path dependency's compiled inputs. Cargo.lock records no checksum for
/// path dependencies, so their sources must enter the identity directly.
pub fn dependency_inputs(dependency: &Path) -> Vec<PathBuf> {
    let mut files = vec![dependency.join("Cargo.toml")];
    if dependency.join("build.rs").is_file() {
        files.push(dependency.join("build.rs"));
    }
    files.extend(inputs(&dependency.join("src"), &["rs"]));
    files
}

fn dependency_paths(manifest: &str) -> Vec<String> {
    #[derive(PartialEq)]
    enum Section {
        Other,
        List,
        Single,
    }
    let compiled =
        |table: &str| table.ends_with("dependencies") && !table.ends_with("dev-dependencies");
    let value = |text: &str| text.trim().trim_matches('"').to_owned();
    let mut section = Section::Other;
    let mut paths = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            let table = line.trim_matches(['[', ']']);
            section = if compiled(table) {
                Section::List
            } else if table
                .rsplit_once('.')
                .is_some_and(|(head, _)| compiled(head))
            {
                Section::Single
            } else {
                Section::Other
            };
        } else if section == Section::Single
            && let Some((key, path)) = line.split_once('=')
            && key.trim() == "path"
        {
            paths.push(value(path));
        } else if section == Section::List
            && let Some((_, table)) = line.split_once('{')
        {
            for pair in table.trim_end_matches('}').split(',') {
                if let Some((key, path)) = pair.split_once('=')
                    && key.trim() == "path"
                {
                    paths.push(value(path));
                }
            }
        }
    }
    paths
}

pub fn fingerprint(files: &[PathBuf], configuration: &[String]) -> String {
    let mut digest = Sha256::new();
    for path in files {
        for bytes in [
            path.to_str().expect("Build input name").as_bytes().to_vec(),
            std::fs::read(path).expect("Read build input"),
        ] {
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
        }
    }
    for value in configuration {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub struct ArtifactCache {
    artifact: PathBuf,
    record: PathBuf,
}
impl ArtifactCache {
    pub fn new(out: &Path, artifact: &str) -> Self {
        Self {
            artifact: out.join(artifact),
            record: out.join(format!("{artifact}.fingerprint")),
        }
    }
    pub fn current(&self, key: &str) -> bool {
        self.artifact.is_file()
            && std::fs::read_to_string(&self.record).is_ok_and(|saved| saved == key)
    }
    pub fn invalidate(&self) {
        match std::fs::remove_file(&self.record) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("Invalidate build artifact: {error}"),
        }
    }
    pub fn commit(&self, key: &str) {
        assert!(self.artifact.is_file(), "Missing compiled artifact");
        std::fs::write(&self.record, key).expect("Record successful build");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_changes_invalidate_identity_but_editor_and_generated_files_do_not() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("main.rs");
        std::fs::write(&source, "fn main() {}").unwrap();
        let identity = || fingerprint(&inputs(root.path(), &["rs", "hex"]), &[]);
        let original = identity();
        for name in [".DS_Store", "main.rs~", "notes.txt"] {
            std::fs::write(root.path().join(name), "editor state").unwrap();
        }
        for name in [".editor", "build", "xcuserdata"] {
            let directory = root.path().join(name);
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join("generated.rs"), "ignored").unwrap();
        }
        assert_eq!(identity(), original);
        std::fs::write(&source, "fn main() { panic!(); }").unwrap();
        let changed = identity();
        assert_ne!(changed, original);
        std::fs::write(root.path().join("new.rs"), "mod new {}").unwrap();
        let added = identity();
        assert_ne!(added, changed);
        std::fs::write(root.path().join("tone.hex"), "00ff").unwrap();
        assert_ne!(identity(), added);
        std::fs::remove_file(root.path().join("tone.hex")).unwrap();
        assert_eq!(identity(), added);
        assert_ne!(
            fingerprint(&inputs(root.path(), &["rs", "hex"]), &["release".into()]),
            identity()
        );
    }

    #[test]
    fn shared_path_dependency_sources_change_identity_but_dev_dependencies_do_not() {
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "acme/Cargo.toml",
            "[package]\nname = \"acme\"\n\n[[bin]]\npath = \"src/main.rs\"\n\n[dependencies]\nacme-mcp = { path = \"../mcp\" }\nserde = \"1\"\n\n[target.'cfg(unix)'.dependencies.acme-tui]\npath = \"../tui\"\n\n[dev-dependencies]\nacme-fixture = { path = \"../fixture\" }\n",
        );
        write("acme/src/main.rs", "fn main() {}");
        write(
            "mcp/Cargo.toml",
            "[dependencies]\nacme-json = { version = \"0.1\", path = \"../json\" }\n",
        );
        write("mcp/src/lib.rs", "pub fn validate() {}");
        write("tui/Cargo.toml", "[package]\nname = \"acme-tui\"\n");
        write("tui/src/lib.rs", "");
        write("json/Cargo.toml", "[package]\nname = \"acme-json\"\n");
        write("json/src/lib.rs", "");
        write("fixture/Cargo.toml", "[package]\nname = \"acme-fixture\"\n");
        write("fixture/src/lib.rs", "");
        let package = root.path().join("acme");
        let dependencies = local_dependencies(&package);
        let mut resolved: Vec<_> = dependencies
            .iter()
            .map(|path| path.canonicalize().unwrap())
            .collect();
        resolved.sort();
        let expected: Vec<_> = ["json", "mcp", "tui"]
            .iter()
            .map(|name| root.path().join(name).canonicalize().unwrap())
            .collect();
        assert_eq!(resolved, expected);
        let identity = || {
            let files: Vec<_> = dependencies
                .iter()
                .flat_map(|dependency| dependency_inputs(dependency))
                .collect();
            fingerprint(&files, &[])
        };
        let original = identity();
        write("fixture/src/lib.rs", "pub fn changed() {}");
        assert_eq!(identity(), original);
        write("mcp/src/lib.rs", "pub fn validate() { todo!() }");
        let edited = identity();
        assert_ne!(edited, original);
        write("json/src/added.rs", "");
        assert_ne!(identity(), edited);
    }

    #[test]
    fn reuse_requires_matching_inputs_and_a_successfully_published_artifact() {
        let root = tempfile::tempdir().unwrap();
        let cache = ArtifactCache::new(root.path(), "classes.dex");
        assert!(!cache.current("one"));
        std::fs::write(&cache.artifact, "compiled").unwrap();
        cache.commit("one");
        assert!(cache.current("one"));
        assert!(!cache.current("two"));
        cache.invalidate();
        // A failed compiler may leave a partial artifact, but no reusable record.
        std::fs::write(&cache.artifact, "partial").unwrap();
        assert!(!cache.current("one"));
        cache.commit("two");
        assert!(cache.current("two"));
        std::fs::remove_file(&cache.artifact).unwrap();
        assert!(!cache.current("two"));
    }
}
