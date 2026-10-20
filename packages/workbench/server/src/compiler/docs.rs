//! Docs lenses: a lens's request (adapter, example source, styles and
//! environment) checked against the project, and the examples it offers,
//! each file of a folder by its name or each named export of a file.
use super::exports::Resolution;
use crate::order::locale_compare;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Declarations and tests are not examples.
static SKIPPED: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\.d\.[cm]?ts$|\.(test|spec)\.[^.]+$").expect("valid pattern")
});
static KEBAB: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("valid pattern"));

/// A lens's request, with every path absolute and inside the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) adapter: String,
    pub(crate) examples: PathBuf,
    pub(crate) folder: bool,
    pub(crate) styles: Vec<PathBuf>,
    pub(crate) environment: Option<PathBuf>,
}

fn inside(root: &Path, target: &Path) -> bool {
    target.starts_with(root)
}

/// The request a lens sends, checked: paths stay inside the project.
pub(crate) fn request(raw: &Value, root: &Path) -> Result<Request, String> {
    let Some(raw) = raw.as_object() else {
        return Err("Docs examples need a request.".into());
    };
    let local = |value: Option<&Value>, what: &str| -> Result<PathBuf, String> {
        let Some(value) = value
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return Err(format!("{what} must be a path inside the project."));
        };
        let target = crate::paths::resolve(root, value);
        if !inside(root, &target) {
            return Err(format!("{what} must be inside the project: {value}"));
        }
        Ok(target)
    };
    let adapter = raw
        .get("adapter")
        .and_then(Value::as_str)
        .filter(|adapter| regex_kebab_name(adapter))
        .ok_or("Examples need a registered kebab-case adapter.")?;
    let examples = local(raw.get("examples"), "The example source")?;
    if !examples.exists() {
        return Err(format!(
            "The example source doesn’t exist: {}",
            raw.get("examples")
                .and_then(Value::as_str)
                .unwrap_or_default()
        ));
    }
    let mut styles = vec![];
    for style in raw
        .get("styles")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let target = local(Some(style), "A style")?;
        if !target.exists() {
            return Err(format!(
                "A style doesn’t exist: {}",
                style.as_str().unwrap_or_default()
            ));
        }
        styles.push(target);
    }
    let environment = match raw.get("environment") {
        Some(Value::String(value)) if !value.is_empty() => {
            Some(local(raw.get("environment"), "The environment")?)
        }
        _ => None,
    };
    Ok(Request {
        adapter: adapter.to_owned(),
        folder: examples.is_dir(),
        examples,
        styles,
        environment,
    })
}

fn regex_kebab_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// `withCustomStyle` → `with-custom-style`; `HTMLButton` → `html-button`.
pub(crate) fn export_name_to_id(name: &str) -> String {
    let characters: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (at, &character) in characters.iter().enumerate() {
        let previous = at.checked_sub(1).map(|before| characters[before]);
        let next = characters.get(at + 1).copied();
        if character.is_ascii_uppercase()
            && let Some(previous) = previous
        {
            let after_lower = previous.is_ascii_lowercase() || previous.is_ascii_digit();
            let acronym_end =
                previous.is_ascii_uppercase() && next.is_some_and(|next| next.is_ascii_lowercase());
            if after_lower || acronym_end {
                out.push('-');
            }
        }
        out.push(character);
    }
    let mut collapsed = String::new();
    let mut underscores = false;
    for character in out.chars() {
        if character == '_' {
            if !underscores {
                collapsed.push('-');
            }
            underscores = true;
        } else {
            underscores = false;
            collapsed.push(character);
        }
    }
    collapsed.to_lowercase()
}

/// The script extensions an adapter compiles as an example file.
fn extensions(adapter: &str) -> &'static [&'static str] {
    match adapter {
        "vue" => &[".vue", ".ts", ".tsx", ".js", ".jsx", ".mjs"],
        "astro" => &[".astro"],
        _ => &[".ts", ".tsx", ".js", ".jsx", ".mjs"],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Example {
    pub(crate) id: String,
    pub(crate) file: PathBuf,
    pub(crate) export: String,
}

/// What a lens offers, and what was wrong with the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) examples: Vec<Example>,
    pub(crate) problems: Vec<String>,
}

/// Each file directly inside a folder whose extension the adapter compiles,
/// in name order; tests, declarations and dot-files are not examples.
fn folder_examples(folder: &Path, adapter: &str) -> Result<Listed, String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("{}: {error}", folder.display()))?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort_by(|a, b| locale_compare(a, b));
    let mut listed = Listed::default();
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if SKIPPED.is_match(&name) {
            continue;
        }
        let Some(extension) = extensions(adapter)
            .iter()
            .find(|extension| name.ends_with(*extension))
        else {
            continue;
        };
        let id = &name[..name.len() - extension.len()];
        if !KEBAB.is_match(id) {
            listed
                .problems
                .push(format!("{name}: example file names must be kebab-case."));
        } else if !seen.insert(id.to_owned()) {
            listed.problems.push(format!(
                "{name}: another file in the folder gives the same example ID."
            ));
        } else {
            listed.examples.push(Example {
                id: id.to_owned(),
                file: folder.join(&name),
                export: "default".into(),
            });
        }
    }
    Ok(listed)
}

/// The examples a lens offers. Astro examples are one component per file.
pub(crate) fn examples(
    request: &Request,
    resolution: &Resolution<'_>,
    astro: bool,
) -> Result<Listed, String> {
    if astro && !request.folder {
        return Err(ASTRO_FOLDER.into());
    }
    if request.folder {
        return folder_examples(&request.examples, &request.adapter);
    }
    let mut listed = Listed::default();
    let mut ids: Vec<(String, String)> = vec![];
    for name in resolution.exports(&request.examples)? {
        if name == "default" {
            continue;
        }
        let id = export_name_to_id(&name);
        if let Some((_, first)) = ids.iter().find(|(seen, _)| *seen == id) {
            listed.problems.push(format!(
                "{name}: gives the same example ID as {first} (“{id}”)."
            ));
            continue;
        }
        ids.push((id.clone(), name.clone()));
        listed.examples.push(Example {
            id,
            file: request.examples.clone(),
            export: name,
        });
    }
    Ok(listed)
}

pub(crate) const ASTRO_FOLDER: &str =
    "Astro examples are one component per file: point the lens at a folder of .astro files.";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn export_names_become_kebab_case_ids() {
        for (name, id) in [
            ("basic", "basic"),
            ("withCustomStyle", "with-custom-style"),
            ("Basic", "basic"),
            ("HTMLButton", "html-button"),
            ("size2x", "size2x"),
            ("with_nested_content", "with-nested-content"),
        ] {
            assert_eq!(export_name_to_id(name), id, "{name}");
        }
    }

    #[test]
    fn a_folder_holds_one_example_per_compilable_file_in_name_order() {
        let dir = std::env::temp_dir().join(format!("acme-docs-folder-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        for name in [
            "with-custom-style.tsx",
            "basic.tsx",
            "basic.test.tsx",
            "types.d.ts",
            "notes.md",
            ".hidden.tsx",
            "WithIcon.tsx",
            "basic.ts",
        ] {
            std::fs::write(dir.join(name), "export default 1;").unwrap();
        }
        let listed = folder_examples(&dir, "react").unwrap();
        let ids: Vec<_> = listed
            .examples
            .iter()
            .map(|example| example.id.as_str())
            .collect();
        assert_eq!(ids, ["basic", "with-custom-style"]);
        assert_eq!(listed.examples[0].file, dir.join("basic.ts"));
        assert_eq!(
            listed.problems,
            [
                "basic.tsx: another file in the folder gives the same example ID.",
                "WithIcon.tsx: example file names must be kebab-case."
            ]
        );
        assert!(folder_examples(&dir, "astro").unwrap().examples.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn requests_stay_inside_the_project_and_say_what_is_wrong() {
        let dir = std::env::temp_dir().join(format!("acme-docs-request-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("card.examples.ts"), "export const basic = 1;").unwrap();
        let ok = request(
            &json!({ "adapter": "react", "examples": "card.examples.ts", "styles": [] }),
            &dir,
        )
        .unwrap();
        assert!(!ok.folder);
        for (raw, message) in [
            (json!(null), "need a request"),
            (
                json!({ "examples": "card.examples.ts" }),
                "kebab-case adapter",
            ),
            (
                json!({ "adapter": "react", "examples": "../outside.ts" }),
                "inside the project",
            ),
            (
                json!({ "adapter": "react", "examples": "missing.ts" }),
                "doesn’t exist",
            ),
            (
                json!({ "adapter": "react", "examples": "card.examples.ts", "styles": ["missing.css"] }),
                "A style doesn’t exist",
            ),
        ] {
            let error = request(&raw, &dir).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
