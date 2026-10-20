//! The spaces one window, or one standalone command, switches between, as
//! its switchers draw them. A folder's workbench.yaml without `spaces` is one
//! space served from the folder; one with `spaces` is a space per entry,
//! each served from its own `root`.
//!
//! A space's id is a hash of its file's folder, and of its key when the file
//! lists several, so it is the same in every window, in every run and in the
//! canvas's address, without saying where the folder is. Its default colour
//! comes from the id too, so a space keeps its colour wherever it is shown.
use crate::{config, paths};
use base64::Engine as _;
use serde_json::{Value, json};
use sha1::{Digest as _, Sha1};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The colours a space without one of its own is drawn in.
const COLORS: [&str; 6] = ["blue", "green", "orange", "purple", "pink", "teal"];

/// The largest image icon carried in a space's mark.
const MAX_IMAGE: u64 = 256 * 1024;

/// Lower-case hex digits.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// The id of the space `key` of the workbench.yaml in `dir`.
pub(crate) fn id(dir: &Path, key: Option<&str>) -> String {
    let at = dir.to_string_lossy();
    let named = key.map_or_else(|| at.clone().into_owned(), |key| format!("{at}#{key}"));
    Sha1::digest(named.as_bytes())
        .iter()
        .take(5)
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 15)],
            ]
        })
        .map(char::from)
        .collect()
}

/// The first letter or digit of `name`, upper-cased, or `?`.
fn initial(name: &str) -> String {
    name.chars()
        .find(|character| character.is_alphanumeric())
        .map_or_else(
            || "?".to_owned(),
            |character| character.to_uppercase().collect(),
        )
}

/// An image icon as a data URI. Each space is its own server and folder, and
/// its mark is drawn wherever the switcher is, so the image can't be fetched
/// from where it lives. One that is missing, too big or outside the space's
/// root leaves the letter in its place.
fn image(root: &Path, file: &str) -> Option<String> {
    let full = paths::resolve(root, file);
    if !full.starts_with(root) || full == root {
        return None;
    }
    let extension = full.extension()?.to_string_lossy().to_lowercase();
    let kind = match extension.as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    let meta = std::fs::metadata(&full).ok()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE {
        return None;
    }
    let body = std::fs::read(&full).ok()?;
    Some(format!(
        "data:{kind};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(body)
    ))
}

/// One space as the switchers draw it. A file that doesn't parse still
/// gives the space, under its folder's name with the default mark, so it can
/// be opened and its error read.
pub(crate) fn describe(dir: &Path, key: Option<&str>, root: &Path) -> Value {
    let id = id(dir, key);
    let (name, mark) = config::identity(dir, key).unwrap_or_default();
    let name = if name.is_empty() {
        root.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        name
    };
    let image = mark["image"].as_str().and_then(|file| image(root, file));
    // An image is drawn as it is, on no colour unless the space names one.
    let color = match mark["color"].as_str() {
        Some(color) => json!(color),
        None if image.is_some() => Value::Null,
        None => {
            let seed = u32::from_str_radix(&id[..8], 16).unwrap_or(0);
            json!(COLORS[usize::try_from(seed).unwrap_or(0) % COLORS.len()])
        }
    };
    json!({
        "id": id,
        "key": key,
        "name": name,
        "dir": dir,
        "root": root,
        "initial": initial(&name),
        "color": color,
        "icon": mark["icon"],
        "image": image,
    })
}

/// Every space the workbench.yaml files in `folders` hold, in order. A space
/// listed by two folders' files is the first one's.
pub(crate) fn listed(folders: &[PathBuf]) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for folder in folders {
        for (key, root) in config::spaces_in(folder) {
            if seen.insert(id(folder, key.as_deref())) {
                out.push(describe(folder, key.as_deref(), &root));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn ids_name_the_folder_and_key_without_saying_where_it_is() {
        let dir = Path::new("/projects/acme");
        assert_eq!(id(dir, None), id(dir, None));
        assert_ne!(id(dir, None), id(dir, Some("ui")));
        assert_eq!(id(dir, None).len(), 10);
        assert!(!id(dir, None).contains("acme"));
    }

    #[test]
    fn a_file_with_spaces_is_one_space_per_entry_with_its_own_root_and_mark() {
        let temp = assert_fs::TempDir::new().unwrap();
        let folder = temp.path().to_path_buf();
        std::fs::create_dir(folder.join("site")).unwrap();
        write(&folder.join("site"), "logo.svg", "<svg/>");
        write(
            &folder,
            "workbench.yaml",
            "spaces:\n  ui:\n    name: Acme UI\n    color: teal\n    icon: palette\n  site:\n    root: site\n    icon: logo.svg\n",
        );
        write(
            &folder,
            "workbench.local.yaml",
            "spaces:\n  ui:\n    name: Acme local\n",
        );
        let spaces = listed(&[folder.clone(), folder.clone()]);
        assert_eq!(
            spaces.len(),
            2,
            "a folder listed twice gives its spaces once"
        );
        assert_eq!(spaces[0]["name"], "Acme local");
        assert_eq!(spaces[0]["key"], "ui");
        assert_eq!(spaces[0]["initial"], "A");
        assert_eq!(spaces[0]["color"], "teal");
        assert_eq!(spaces[0]["icon"], "palette");
        assert_eq!(
            spaces[1]["name"], "Site",
            "an unnamed space is labelled from its key"
        );
        assert_eq!(spaces[1]["root"], json!(folder.join("site")));
        assert_eq!(spaces[1]["color"], Value::Null);
        assert!(
            spaces[1]["image"]
                .as_str()
                .unwrap()
                .starts_with("data:image/svg+xml;base64,")
        );
    }

    #[test]
    fn an_unreadable_file_still_lists_its_folder_with_the_default_mark() {
        let temp = assert_fs::TempDir::new().unwrap();
        let folder = temp.path().to_path_buf();
        write(&folder, "workbench.yaml", "spaces: [unclosed\n");
        let spaces = listed(std::slice::from_ref(&folder));
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0]["key"], Value::Null);
        assert_eq!(
            spaces[0]["name"],
            json!(folder.file_name().unwrap().to_string_lossy())
        );
        assert!(COLORS.iter().any(|color| spaces[0]["color"] == *color));
        assert!(listed(&[folder.join("missing")]).is_empty());
    }

    #[test]
    fn an_image_outside_the_root_or_too_big_leaves_the_letter() {
        let temp = assert_fs::TempDir::new().unwrap();
        let folder = temp.path().to_path_buf();
        std::fs::write(folder.join("big.png"), vec![0; 300 * 1024]).unwrap();
        assert_eq!(image(&folder, "big.png"), None);
        assert_eq!(image(&folder.join("site"), "../big.png"), None);
        assert_eq!(image(&folder, "missing.svg"), None);
        assert_eq!(initial("  émile"), "É");
        assert_eq!(initial("—"), "?");
    }
}
