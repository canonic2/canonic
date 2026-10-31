//! Checks the bundled skill's sources and embeds the Markdown guides through
//! canonic-mcp-kit's documentation bundle.
use std::{env, fs, io, path::Path};

fn main() -> io::Result<()> {
    let root = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| io::Error::other("missing package root"))?;
    // The selected workflow has one source file. Fail rather than silently
    // omitting new support files or embedding a link outside this bundle.
    println!("cargo:rerun-if-changed=skills");
    let skills = Path::new(&root).join("skills");
    let selected = skills.join("policy-review");
    for path in [&skills, &selected, &selected.join("SKILL.md")] {
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(io::Error::other("bundled skills must not contain symlinks"));
        }
    }
    for entry in fs::read_dir(&selected)? {
        let entry = entry?;
        if entry.file_name() != "SKILL.md" || !entry.file_type()?.is_file() {
            return Err(io::Error::other("unselected file in policy-review bundle"));
        }
    }
    // Declares docs/ for rebuilds and writes the bundle to OUT_DIR.
    canonic_mcp_kit::docs::build::embed().map_err(io::Error::other)?;
    Ok(())
}
