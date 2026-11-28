//! Build-time bundle of the explicitly selected MCP skill files; output stays in OUT_DIR.
use std::{
    env,
    fmt::Write as _,
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Every published skill and its complete file set, relative to `skills/<name>/`.
/// The source directory must contain exactly these files.
pub const SELECTED: &[(&str, &[&str])] = &[("ui-verification", &["SKILL.md"])];

pub fn embed() -> io::Result<()> {
    let root = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| io::Error::other("missing package root"))?;
    let out = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing build output"))?;
    println!("cargo:rerun-if-changed=skills");
    generate(&Path::new(&root).join("skills"), Path::new(&out))
}

pub fn generate(skills: &Path, out: &Path) -> io::Result<()> {
    let bundle = out.join("testrun_skills");
    if bundle.exists() {
        fs::remove_dir_all(&bundle)?;
    }
    fs::create_dir_all(&bundle)?;
    let mut source = String::from("const SKILL_FILES: &[(&str, &str, &[u8])] = &[\n");
    for (index, (skill, path, bytes)) in selected(skills)?.into_iter().enumerate() {
        let copy = format!("{index}.bin");
        fs::write(bundle.join(&copy), bytes)?;
        writeln!(
            source,
            "    ({skill:?}, {path:?}, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/testrun_skills/{copy}\"))),"
        )
        .map_err(io::Error::other)?;
    }
    source.push_str("];\n");
    fs::write(out.join("testrun_skills.rs"), source)
}

/// The selected files' checked paths, for embedding and runtime identity.
pub fn sources(skills: &Path) -> Vec<PathBuf> {
    SELECTED
        .iter()
        .flat_map(|(skill, paths)| paths.iter().map(move |path| skills.join(skill).join(path)))
        .collect()
}

fn selected(skills: &Path) -> io::Result<Vec<(&'static str, &'static str, Vec<u8>)>> {
    let mut found = Vec::new();
    collect(skills, skills, &mut found)?;
    found.sort();
    let mut expected = SELECTED
        .iter()
        .flat_map(|(skill, paths)| paths.iter().map(move |path| format!("{skill}/{path}")))
        .collect::<Vec<_>>();
    expected.sort();
    if found != expected {
        return Err(io::Error::other(format!(
            "skills/ must contain exactly the selected files: found {found:?}, selected {expected:?}"
        )));
    }
    let mut files = Vec::new();
    for (skill, paths) in SELECTED {
        for path in *paths {
            let bytes = fs::read(skills.join(skill).join(path))?;
            if path.ends_with(".md") {
                check_links(skill, path, &bytes, paths)?;
            }
            files.push((*skill, *path, bytes));
        }
    }
    Ok(files)
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<String>) -> io::Result<()> {
    if fs::symlink_metadata(directory)?.file_type().is_symlink() {
        return Err(io::Error::other("skills must not contain symlinks"));
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_symlink() {
            return Err(io::Error::other("skills must not contain symlinks"));
        }
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_str()
                .ok_or_else(|| io::Error::other("non-UTF-8 skill path"))?
                .replace('\\', "/");
            files.push(relative);
        }
    }
    Ok(())
}

/// Relative Markdown links must name another selected file of the same skill.
fn check_links(skill: &str, path: &str, bytes: &[u8], bundle: &[&str]) -> io::Result<()> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| io::Error::other(format!("{skill}/{path} is not UTF-8")))?;
    for target in text
        .split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
    {
        let target = target.split('#').next().unwrap_or_default();
        if target.is_empty() || target.contains("://") || target.starts_with("mailto:") {
            continue;
        }
        let base = Path::new(path).parent().unwrap_or_else(|| Path::new(""));
        let mut resolved = Vec::new();
        for component in base.join(target).components() {
            match component {
                Component::Normal(part) => resolved.push(part.to_string_lossy().into_owned()),
                Component::CurDir => {}
                Component::ParentDir if resolved.pop().is_some() => {}
                _ => {
                    return Err(io::Error::other(format!(
                        "{skill}/{path} links outside its skill: {target}"
                    )));
                }
            }
        }
        let resolved = resolved.join("/");
        if !bundle.contains(&resolved.as_str()) {
            return Err(io::Error::other(format!(
                "{skill}/{path} links to an unselected file: {target}"
            )));
        }
    }
    Ok(())
}
