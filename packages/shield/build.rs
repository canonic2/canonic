//! Checks the bundled skills' sources and embeds the Markdown guides through
//! canonic-mcp-kit's documentation bundle.
use std::{env, fs, io, path::Path};

fn main() -> io::Result<()> {
    let root = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| io::Error::other("missing package root"))?;
    // Each skill folder holds one SKILL.md. Fail rather than silently omitting
    // new support files or embedding a link outside the bundle.
    println!("cargo:rerun-if-changed=skills");
    let skills = Path::new(&root).join("skills");
    if skills.exists() {
        check_skills(&skills)?;
    }
    // Declares docs/ for rebuilds and writes the bundle to OUT_DIR.
    canonic_mcp_kit::docs::build::embed().map_err(io::Error::other)?;
    Ok(())
}

fn check_skills(skills: &Path) -> io::Result<()> {
    if fs::symlink_metadata(skills)?.file_type().is_symlink() {
        return Err(io::Error::other("bundled skills must not contain symlinks"));
    }
    for skill in fs::read_dir(skills)? {
        let skill = skill?;
        if !skill.file_type()?.is_dir() {
            return Err(io::Error::other("bundled skills must be folders"));
        }
        for entry in fs::read_dir(skill.path())? {
            let entry = entry?;
            if entry.file_name() != "SKILL.md" || !entry.file_type()?.is_file() {
                return Err(io::Error::other("unselected file in a bundled skill"));
            }
        }
    }
    Ok(())
}
