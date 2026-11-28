//! Stateless discovery pages and bounded directory sort keys.
use super::*;

type Pending = std::collections::BTreeMap<Vec<u8>, (String, PathBuf, usize)>;

/// Digest components give a bounded preorder key: a parent sorts before every
/// descendant, so traversal can prune whole subtrees before the cursor. Six
/// levels use at most 103 bytes. Explicit selections use full identity digests.
fn directory_key(relative: &str, explicit: bool) -> Vec<u8> {
    if explicit {
        return canonic_mcp_kit::page::digest(relative.as_bytes()).to_vec();
    }
    let mut key = Vec::new();
    for component in relative
        .split('/')
        .filter(|component| *component != "." && !component.is_empty())
    {
        key.push(1);
        key.extend_from_slice(&canonic_mcp_kit::page::digest(component.as_bytes())[..16]);
    }
    key.push(0);
    key
}
fn selected_directories(root: &Path, paths: &[PathBuf]) -> Result<Pending> {
    let mut pending = std::collections::BTreeMap::new();
    for path in if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    } {
        let path = root
            .join(path)
            .canonicalize()
            .map_err(|_| Error::new("CONFIG_PATH", "Selected directory is unavailable"))?;
        if !path.starts_with(root) || !path.is_dir() {
            return Err(Error::invalid("Select directories inside root"));
        }
        let relative = path
            .strip_prefix(root)
            .expect("inside root")
            .to_string_lossy()
            .replace('\\', "/");
        let relative = if relative.is_empty() {
            ".".to_owned()
        } else {
            relative
        };
        pending.insert(
            directory_key(&relative, !paths.is_empty()),
            (relative, path, 0),
        );
    }
    Ok(pending)
}
fn validate_key(key: &[u8], explicit: bool) -> Result<()> {
    let valid = if explicit {
        key.len() == 32
    } else {
        key.last() == Some(&0)
            && key.len() <= 103
            && (key.len() - 1).is_multiple_of(17)
            && key[..key.len() - 1]
                .as_chunks::<17>()
                .0
                .iter()
                .all(|component| component[0] == 1)
    };
    if valid {
        Ok(())
    } else {
        Err(canonic_mcp_kit::page::Error::InvalidCursor.into())
    }
}
pub(in crate::configuration::onboarding) fn page(
    root: &Path,
    paths: &[PathBuf],
    args: &Value,
) -> Result<Value> {
    use canonic_mcp_kit::page::{Builder, Limits, Position};
    let root = root
        .canonicalize()
        .map_err(|_| Error::new("CONFIG_PATH", "Project root is unavailable"))?;
    let scope = crate::pagination::scope(
        "project_discover",
        &(&root, paths, crate::pagination::filters(args)),
    )?;
    let (request, after) = crate::pagination::request(args, &scope)?;
    let mut pending = selected_directories(&root, paths)?;
    let explicit = !paths.is_empty();
    if let Some(key) = &after {
        validate_key(key, explicit)?;
    }
    let mut scan = Scan::new(root.clone());
    let mut builder = Builder::new(
        &scope,
        request.limit,
        Limits::STANDARD,
        crate::protocol::MAX_REPLY,
        "project_discover with an explicit path",
    )?;
    while !builder.is_full() {
        let Some((key, (relative, path, depth))) = pending.pop_first() else {
            break;
        };
        if after.as_ref().is_some_and(|after| {
            key <= *after && (explicit || !after.starts_with(&key[..key.len() - 1]))
        }) {
            continue;
        }
        let entries = std::fs::read_dir(&path)
            .map_err(|_| Error::new("CONFIG_IO", "Cannot inspect project directory"))?;
        let mut names = BTreeSet::new();
        for entry in entries {
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
            if !explicit
                && kind.is_dir()
                && !name.starts_with('.')
                && !SKIP.contains(&name.as_str())
                && !name.ends_with(".xcodeproj")
                && !name.ends_with(".xcworkspace")
            {
                if depth < 6 {
                    let child = if relative == "." {
                        name
                    } else {
                        format!("{relative}/{name}")
                    };
                    pending.insert(
                        directory_key(&child, false),
                        (child, entry.path(), depth + 1),
                    );
                } else {
                    scan.result.truncated = true;
                }
            }
        }
        if after.as_ref().is_some_and(|after| key <= *after) {
            continue;
        }
        scan.bytes = 0;
        let candidate = scan.classify(&path, &names).or_else(|| {
            explicit.then(|| Candidate {
                path: relative.clone(),
                kind: "custom".into(),
                platforms: Vec::new(),
                evidence: Vec::new(),
            })
        });
        if let Some(candidate) = candidate {
            if !explicit && scan.generated(&candidate) {
                continue;
            }
            if !builder.push(json!(candidate), &Position::Keyset(key))? {
                break;
            }
        }
    }
    let page = builder.finish(!pending.is_empty());
    let warnings_total = scan.result.warnings.len();
    scan.result.warnings.truncate(16);
    Ok(
        json!({"root":root,"candidates":page.items,"nextCursor":page.next_cursor,"warnings":scan.result.warnings,"warnings_total":warnings_total,"scanDepth":6,"deeperProjects":{"tool":"project_discover","arguments":{"root":root,"paths":["relative/deeper/directory"]}},"incompleteManifests":scan.result.truncated}),
    )
}
