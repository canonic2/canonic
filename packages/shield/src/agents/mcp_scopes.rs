//! Bounded read-only observations for adapter-owned MCP scope checks.
use crate::review::Error;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) fn read(path: &Path) -> Result<Option<String>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::Invalid("cannot inspect MCP configuration scope")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(Error::Invalid(
            "MCP configuration scope must be a bounded regular file",
        ));
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_string(&mut text))
        .map_err(|_| Error::Invalid("cannot read MCP configuration scope"))?;
    if text.len() > 1024 * 1024 {
        return Err(Error::Invalid("MCP configuration scope exceeds limit"));
    }
    Ok(Some(text))
}

pub(super) fn configuration(override_name: &str, default: &str) -> Result<PathBuf, Error> {
    let path = super::working_files::configuration(override_name, default)
        .filter(|path| path.is_absolute())
        .ok_or(Error::Invalid(
            "cannot resolve absolute MCP configuration scope",
        ))?;
    Ok(path)
}

pub(super) fn project_layers(root: &Path, cwd: &Path) -> Result<Vec<PathBuf>, Error> {
    let root = root.canonicalize()?;
    let cwd = cwd.canonicalize()?;
    let relative = cwd
        .strip_prefix(&root)
        .map_err(|_| Error::Invalid("MCP working directory is outside the bound project"))?;
    let mut layers = vec![root.clone()];
    let mut directory = root;
    for part in relative.components() {
        if layers.len() >= 64 {
            return Err(Error::Invalid(
                "MCP configuration scope depth exceeds limit",
            ));
        }
        directory.push(part);
        layers.push(directory.clone());
    }
    Ok(layers)
}

pub(super) fn no_alias<'a>(
    alias: &str,
    aliases: impl Iterator<Item = &'a str>,
) -> Result<(), Error> {
    let aliases = aliases.collect::<Vec<_>>();
    if aliases.contains(&alias) {
        return Err(Error::Invalid(
            "bound MCP alias is shadowed by another host scope; resolve the conflict before setup",
        ));
    }
    super::unambiguous_mcp_alias(alias, aliases.into_iter())
}
