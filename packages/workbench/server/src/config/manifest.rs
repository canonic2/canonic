//! The config rules of `contracts/manifest.js` that the server reads a
//! space with: spaces and local merging, implementations, previews, the
//! space's mark, and a page's lenses, Markdown and code. Every problem keeps
//! the Node reader's wording. Pure.
use super::js::{self, label_of};
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

type Problems = Vec<String>;

/// `text` of manifest.js: a number as its text, a string trimmed, else empty.
pub(super) fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::Number(number)) => js::number_text(number),
        Some(Value::String(text)) => js::trim(text).to_owned(),
        _ => String::new(),
    }
}

/// Whether a value is absent, null or the empty string, as the rules test it.
fn blank(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null)) || value.and_then(Value::as_str) == Some("")
}

fn regex(cell: &'static OnceLock<regex::Regex>, pattern: &str) -> &'static regex::Regex {
    cell.get_or_init(|| regex::Regex::new(pattern).expect("manifest expression"))
}

pub(super) fn is_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_http(text: &str) -> bool {
    static CELL: OnceLock<regex::Regex> = OnceLock::new();
    regex(&CELL, r"(?i)^https?://[^/]+").is_match(text)
}

fn has_scheme(text: &str) -> bool {
    static CELL: OnceLock<regex::Regex> = OnceLock::new();
    regex(&CELL, r"(?i)^[a-z][a-z0-9+.-]*://").is_match(text)
}

/// A page whose src is Markdown: its docs are its own view.
pub(super) fn is_docs(src: &str) -> bool {
    js::trim(src).to_ascii_lowercase().ends_with(".md")
}

/// A path inside the project, relative to its root.
fn project_path(value: Option<&Value>) -> Option<String> {
    static DRIVE: OnceLock<regex::Regex> = OnceLock::new();
    let path = text(value);
    if path.is_empty()
        || path.starts_with('/')
        || regex(&DRIVE, r"(?i)^[a-z]:[\\/]").is_match(&path)
        || has_scheme(&path)
    {
        return None;
    }
    if path.split(['\\', '/']).any(|part| part == "..") {
        return None;
    }
    Some(path)
}

/// Why a src can't be used, or `None` when it can.
pub(super) fn src_problem(src: &str) -> Option<String> {
    let found: Vec<String> = [":", "+", "!", "~"]
        .iter()
        .filter(|mark| src.contains(**mark))
        .map(|mark| format!("“{mark}”"))
        .collect();
    if found.is_empty() {
        return None;
    }
    Some(format!(
        "src “{src}” can’t contain {} — “:”, “+”, “!”, and “~” mark the scenario, the iteration, the example, and the lens in the address.",
        found.join(" or ")
    ))
}

/// The problem of a space with no page and nothing to import.
pub(super) const NOTHING_TO_SHOW: &str =
    "nothing to show — a config needs at least one collection with one page in it.";

/// A page whose src is a preview definition, which gives it its scenarios.
pub(super) fn is_preview_src(src: &str) -> bool {
    let src = js::trim(src);
    src.ends_with(".workbench.ts") || src.ends_with(".workbench.tsx")
}

/// A scenario, artboard or iteration id: kebab-case words, starting with a letter.
pub(super) fn is_id(id: &str) -> bool {
    id.split('-').enumerate().all(|(at, word)| {
        !word.is_empty()
            && word
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && (at > 0 || word.starts_with(|first: char| first.is_ascii_lowercase()))
    })
}

/// A scenario entry's iterations in workbench.yaml, as `[{ id, label }]`
/// with labels defaulting from the id, or `None` when none are valid. Invalid
/// and repeated entries are reported and left out, as `scenarioIterations`
/// does. `where_` names the page and `scenario` its scenario.
pub(super) fn scenario_iterations(
    raw: Option<&Value>,
    where_: &str,
    scenario: &str,
    problems: &mut Problems,
) -> Option<Value> {
    let raw = raw.filter(|raw| !raw.is_null())?;
    let at = format!("{where_} › scenario {scenario}");
    let Some(entries) = raw.as_array() else {
        problems.push(format!(
            "{at}: iterations must be a list of entries with an id and an optional label."
        ));
        return None;
    };
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let id = text(entry.get("id"));
        if id.is_empty() {
            problems.push(format!("{at}: iteration {} needs an id.", index + 1));
            continue;
        }
        if !is_id(&id) {
            problems.push(format!(
                "{at}: iteration id “{id}” must be kebab-case — it travels in a URL."
            ));
            continue;
        }
        if seen.contains(&id) {
            problems.push(format!("{at}: iteration id “{id}” is declared twice."));
            continue;
        }
        let mut label = text(entry.get("label"));
        if label.is_empty() {
            label = label_of(&id);
        }
        out.push(json!({ "id": id, "label": label }));
        seen.push(id);
    }
    (!out.is_empty()).then_some(Value::Array(out))
}

/// A string, trimmed, else empty: the config readers' `text`, for which
/// numbers are not text.
fn string_text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| js::trim(text).to_owned())
        .unwrap_or_default()
}

/// A page's scenarios in workbench.yaml, as `pageScenarios` reads them:
/// every valid scenario's id in order, which its lens path maps are checked
/// against, and the list the page keeps — only when there are two or more,
/// or one lists iterations. Labels default from the id; invalid and repeated
/// entries are reported and left out. `where_` names the page.
pub(super) fn page_scenarios(
    raw: Option<&Value>,
    where_: &str,
    problems: &mut Problems,
) -> (Vec<String>, Option<Value>) {
    let mut ids: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut iterated = false;
    let entries = match raw {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(entries)) => entries.as_slice(),
        Some(_) => {
            problems.push(format!(
                "{where_}: scenarios must be a list of entries with an id and an optional label."
            ));
            return (ids, None);
        }
    };
    for (index, scenario) in entries.iter().enumerate() {
        let id = string_text(scenario.get("id"));
        if id.is_empty() {
            problems.push(format!("{where_}: scenario {} needs an id.", index + 1));
            continue;
        }
        if !is_id(&id) {
            problems.push(format!(
                "{where_}: scenario id “{id}” must be kebab-case — it travels in a URL."
            ));
            continue;
        }
        if ids.contains(&id) {
            problems.push(format!("{where_}: scenario id “{id}” is declared twice."));
            continue;
        }
        let mut label = string_text(scenario.get("label"));
        if label.is_empty() {
            label = label_of(&id);
        }
        let mut entry = json!({ "id": id, "label": label });
        if let Some(iterations) =
            scenario_iterations(scenario.get("iterations"), where_, &id, problems)
        {
            entry["iterations"] = iterations;
            iterated = true;
        }
        out.push(entry);
        ids.push(id);
    }
    let kept = (out.len() > 1 || iterated).then_some(Value::Array(out));
    (ids, kept)
}

/// workbench.local.yaml over workbench.yaml: top-level keys replace, except
/// `spaces`, `implementations` and `sizes`, which merge by name.
pub(super) fn merge(base: &Value, local: &Value) -> Value {
    let empty = Map::new();
    let base = base.as_object().unwrap_or(&empty);
    let local = local.as_object().unwrap_or(&empty);
    let mut out = base.clone();
    for (key, value) in local {
        if key == "spaces"
            && let (Some(base_spaces), Some(local_spaces)) = (
                base.get("spaces").and_then(Value::as_object),
                value.as_object(),
            )
        {
            let mut spaces = base_spaces.clone();
            for (id, space) in local_spaces {
                let merged = match (spaces.get(id), space) {
                    (Some(existing @ Value::Object(_)), Value::Object(_)) => merge(existing, space),
                    _ => space.clone(),
                };
                spaces.insert(id.clone(), merged);
            }
            out.insert(key.clone(), Value::Object(js::ordered(spaces)));
            continue;
        }
        let by_name = key == "implementations" || key == "sizes";
        match (base.get(key).and_then(Value::as_object), value.as_object()) {
            (Some(base_named), Some(local_named)) if by_name => {
                let mut named = base_named.clone();
                for (name, entry) in local_named {
                    let merged = match (named.get(name), entry) {
                        (Some(Value::Object(existing)), Value::Object(update)) => {
                            let mut combined = existing.clone();
                            combined.extend(update.clone());
                            Value::Object(js::ordered(combined))
                        }
                        _ => entry.clone(),
                    };
                    named.insert(name.clone(), merged);
                }
                out.insert(key.clone(), Value::Object(js::ordered(named)));
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(js::ordered(out))
}

/// The ids of the spaces a file lists, reporting malformed ones.
pub(super) fn space_keys(raw: &Value, problems: &mut Problems) -> Vec<String> {
    let Some(spaces) = raw
        .as_object()
        .and_then(|raw| raw.get("spaces"))
        .filter(|spaces| !spaces.is_null())
    else {
        return Vec::new();
    };
    let Some(spaces) = spaces.as_object() else {
        problems.push("spaces: must be a map of space ids to spaces.".to_owned());
        return Vec::new();
    };
    spaces
        .iter()
        .filter(|(key, value)| {
            if !is_key(key) {
                problems.push(format!("spaces: “{key}” must be a kebab-case id."));
                return false;
            }
            if !value.is_object() {
                problems.push(format!("spaces › {key}: must be a map."));
                return false;
            }
            true
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// The top-level keys a space never inherits.
const OWN: [&str; 5] = ["name", "color", "icon", "root", "sizes"];

/// The config one space sees: `(raw, key, root)`.
pub(super) fn select_space(
    raw: &Value,
    key: Option<&str>,
    problems: &mut Problems,
) -> (Value, Option<String>, Option<String>) {
    let keys = space_keys(raw, problems);
    let empty = Map::new();
    let file = raw.as_object().unwrap_or(&empty);
    if keys.is_empty() {
        let whole: Map<String, Value> = file
            .iter()
            .filter(|(name, _)| *name != "spaces")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        return (Value::Object(whole), None, None);
    }
    let picked = key
        .filter(|key| keys.iter().any(|candidate| candidate == key))
        .map_or_else(|| keys[0].clone(), str::to_owned);
    if let Some(key) = key.filter(|key| *key != picked) {
        problems.push(format!("spaces: there is no space “{key}”."));
    }
    let shared: Map<String, Value> = file
        .iter()
        .filter(|(name, _)| *name != "spaces" && !OWN.contains(&name.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let entry = file["spaces"][&picked]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let own: Map<String, Value> = entry
        .iter()
        .filter(|(name, _)| *name != "root")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut effective = merge(&Value::Object(shared), &Value::Object(own));
    if text(effective.get("name")).is_empty() {
        effective["name"] = Value::String(label_of(&picked));
    }
    let mut root = if blank(entry.get("root")) {
        None
    } else {
        Some(text(entry.get("root")))
    };
    if !blank(entry.get("root"))
        && root
            .as_deref()
            .is_none_or(|root| root.is_empty() || has_scheme(root))
    {
        problems.push(format!("spaces › {picked}: root must be a folder path."));
        root = None;
    }
    (effective, Some(picked), root)
}

/// How the space is marked: `{ color, icon, image }`.
pub(super) fn space_mark(raw: &Value, problems: &mut Problems) -> Value {
    const COLORS: [&str; 9] = [
        "blue", "green", "orange", "purple", "pink", "teal", "red", "yellow", "gray",
    ];
    static HEX: OnceLock<regex::Regex> = OnceLock::new();
    static IMAGE: OnceLock<regex::Regex> = OnceLock::new();
    let mut out = json!({ "color": null, "icon": null, "image": null });
    let empty = Value::Object(Map::new());
    let raw = if raw.is_object() { raw } else { &empty };
    if !blank(raw.get("color")) {
        let color = text(raw.get("color")).to_lowercase();
        if COLORS.contains(&color.as_str())
            || regex(&HEX, r"(?i)^#(?:[0-9a-f]{3}|[0-9a-f]{6})$").is_match(&color)
        {
            out["color"] = Value::String(color);
        } else {
            problems.push(format!(
                "color: must be one of {}, or a hex colour such as #2f7d55.",
                COLORS.join(", ")
            ));
        }
    }
    if !blank(raw.get("icon")) {
        let icon = text(raw.get("icon"));
        if is_key(&icon) {
            out["icon"] = Value::String(icon);
        } else if regex(&IMAGE, r"(?i)\.(?:svg|png|jpe?g|webp|gif)$").is_match(&icon)
            && !icon.starts_with('/')
            && !icon.contains("..")
            && !has_scheme(&icon)
            && !icon.contains('\\')
        {
            out["image"] = Value::String(icon);
        } else {
            problems.push("icon: must be a kebab-case Lucide icon name, or a project-relative .svg, .png, .jpg, .webp, or .gif file.".to_owned());
        }
    }
    out
}

fn read_lens_label(raw: Option<&Value>, where_: &str, problems: &mut Problems) -> Option<String> {
    let raw = raw?;
    match raw.as_str().map(js::trim) {
        Some(label) if !label.is_empty() => Some(label.to_owned()),
        _ => {
            problems.push(format!("{where_}: must be a nonempty string."));
            None
        }
    }
}

/// `previews`: `false`, or the settings map.
pub(super) fn previews(raw: Option<&Value>, problems: &mut Problems) -> Value {
    let Some(raw) = raw.filter(|raw| !raw.is_null()) else {
        return json!({});
    };
    if raw == &Value::Bool(false) {
        return Value::Bool(false);
    }
    let Some(settings) = raw.as_object() else {
        problems.push(
            "previews: must be false or a map with include, config, and icon settings.".to_owned(),
        );
        return Value::Bool(false);
    };
    let mut out = Map::new();
    if let Some(label) = read_lens_label(settings.get("lensLabel"), "previews.lensLabel", problems)
    {
        out.insert("lensLabel".to_owned(), Value::String(label));
    }
    if settings.contains_key("icon") {
        let icon = text(settings.get("icon"));
        if is_key(&icon) {
            out.insert("icon".to_owned(), Value::String(icon));
        } else {
            problems.push("previews.icon: must be a kebab-case Lucide icon name.".to_owned());
        }
    }
    if let Some(icons) = settings.get("icons") {
        let mut mapped = Map::new();
        if let Some(icons) = icons.as_object() {
            for (prefix, icon) in icons {
                let normalized = js::trim(prefix).trim_end_matches('/').to_owned();
                let icon = text(Some(icon));
                if normalized.is_empty() || !is_key(&icon) {
                    problems.push(format!(
                        "previews.icons: icon for “{prefix}” must be a kebab-case Lucide icon name."
                    ));
                } else {
                    mapped.insert(normalized, Value::String(icon));
                }
            }
        } else {
            problems
                .push("previews.icons: must map title prefixes to Lucide icon names.".to_owned());
        }
        out.insert("icons".to_owned(), Value::Object(js::ordered(mapped)));
    }
    if let Some(include) = settings.get("include") {
        let valid = include.as_array().is_some_and(|patterns| {
            !patterns.is_empty()
                && patterns.iter().all(|pattern| {
                    pattern.as_str().is_some_and(|pattern| {
                        !js::trim(pattern).is_empty()
                            && !pattern.starts_with('/')
                            && !pattern.contains("..")
                    })
                })
        });
        if !valid {
            problems.push(
                "previews.include: must be a nonempty list of project-relative glob patterns."
                    .to_owned(),
            );
            return Value::Bool(false);
        }
        out.insert("include".to_owned(), include.clone());
    }
    if settings.contains_key("config") {
        let file = text(settings.get("config"));
        if file.is_empty() || file.starts_with('/') || file.contains("..") || has_scheme(&file) {
            problems.push("previews.config: must be a project-relative config file.".to_owned());
            return Value::Bool(false);
        }
        out.insert("config".to_owned(), Value::String(file));
    }
    Value::Object(out)
}

/// A catalog's collection icon.
enum CatalogIcon {
    /// The built-in default, which ranks below an authored default.
    BuiltIn,
    /// An icon configured for the catalog's collection.
    Configured(String),
}

/// A catalog, when the implementation declares one, with its icon.
fn catalog(raw: Option<&Value>, where_: &str, problems: &mut Problems) -> Option<CatalogIcon> {
    match raw {
        None | Some(Value::Null | Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(CatalogIcon::BuiltIn),
        Some(Value::Object(settings)) => {
            let icon = text(settings.get("icon"));
            if icon.is_empty() {
                return Some(CatalogIcon::BuiltIn);
            }
            if !is_key(&icon) {
                problems.push(format!(
                    "{where_}: catalog icon “{icon}” must be a kebab-case Lucide icon name."
                ));
                return Some(CatalogIcon::BuiltIn);
            }
            Some(CatalogIcon::Configured(icon))
        }
        Some(_) => {
            problems.push(format!(
                "{where_}: catalog must be true, false, or a map with an icon."
            ));
            None
        }
    }
}

fn startup_probe(raw: Option<&Value>, where_: &str, problems: &mut Problems) -> Option<Value> {
    let Some(probe) = raw.and_then(Value::as_object) else {
        problems.push(format!("{where_} must contain a port or URL."));
        return None;
    };
    let port = js::to_number(probe.get("port"));
    if probe.contains_key("port")
        && !probe.contains_key("url")
        && js::is_integer(port)
        && (1.0..=65535.0).contains(&port)
    {
        let host = text(probe.get("host"));
        return Some(
            json!({ "port": js::number(port), "host": if host.is_empty() { "127.0.0.1".to_owned() } else { host } }),
        );
    }
    if probe.contains_key("url") && !probe.contains_key("port") && is_http(&text(probe.get("url")))
    {
        return Some(json!({ "url": text(probe.get("url")) }));
    }
    problems.push(format!(
        "{where_} needs exactly one valid port (1–65535) or http(s) URL."
    ));
    None
}

fn startup(raw: Option<&Value>, where_: &str, problems: &mut Problems) -> Value {
    let Some(raw) = raw.filter(|raw| !raw.is_null()) else {
        return Value::Null;
    };
    let Some(spec) = raw.as_object() else {
        problems.push(format!(
            "{where_}: start must be a map with command and check."
        ));
        return Value::Null;
    };
    let command = text(spec.get("command"));
    if command.is_empty() {
        problems.push(format!(
            "{where_}: start.command must be a nonempty shell command."
        ));
    }
    let probe = startup_probe(
        spec.get("check"),
        &format!("{where_}: start.check"),
        problems,
    );
    let ready = if spec.contains_key("ready") {
        startup_probe(
            spec.get("ready"),
            &format!("{where_}: start.ready"),
            problems,
        )
    } else {
        None
    };
    let mut timeout = spec.get("timeout").map(|value| js::to_number(Some(value)));
    if let Some(seconds) = timeout
        && (!js::is_integer(seconds) || !(1.0..=300.0).contains(&seconds))
    {
        problems.push(format!("{where_}: start.timeout must be 1–300 seconds."));
        timeout = None;
    }
    let Some(probe) =
        probe.filter(|_| !command.is_empty() && (!spec.contains_key("ready") || ready.is_some()))
    else {
        return Value::Null;
    };
    let cwd = text(spec.get("cwd"));
    let mut out = json!({
        "command": command,
        "cwd": if cwd.is_empty() { ".".to_owned() } else { cwd },
        "check": probe,
        "timeout": js::number(timeout.unwrap_or(60.0)),
    });
    if let Some(ready) = ready {
        out["ready"] = ready;
    }
    out
}

/// The declared implementations by name; malformed ones are reported and left out.
#[expect(
    clippy::too_many_lines,
    reason = "one rule per kind, kept in the Node reader's order so problems match it"
)]
pub(super) fn implementations(raw: Option<&Value>, problems: &mut Problems) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(raw) = raw.filter(|raw| !blank(Some(raw))) else {
        return out;
    };
    let Some(declared) = raw.as_object() else {
        problems.push("implementations: must be a map of names to implementations.".to_owned());
        return out;
    };
    for (key, spec) in declared {
        let where_ = format!("implementations › {key}");
        if !is_key(key) {
            problems.push(format!(
                "implementations: “{key}” must be kebab-case — it travels in a URL."
            ));
            continue;
        }
        if key == "design" {
            problems.push(
                "implementations: “design” is reserved for a page’s own lens; choose another name."
                    .to_owned(),
            );
            continue;
        }
        let Some(spec) = spec.as_object() else {
            problems.push(format!("{where_}: needs a kind, and a url or base."));
            continue;
        };
        let kind = text(spec.get("kind"));
        if ![
            "url",
            "storybook",
            "workbench",
            "docs",
            "ios-simulator",
            "window",
        ]
        .contains(&kind.as_str())
        {
            problems.push(format!(
                "{where_}: kind must be url, storybook, workbench, docs, ios-simulator, or window."
            ));
            continue;
        }
        let label = text(spec.get("label"));
        let mut implementation = json!({ "key": key, "label": if label.is_empty() { label_of(key) } else { label }, "kind": kind, "root": null });
        let mut owned = false;
        match kind.as_str() {
            "workbench" => implementation["root"] = json!("."),
            "docs" => {
                let adapter = text(spec.get("adapter"));
                if !is_key(&adapter) {
                    problems.push(format!("{where_}: needs an adapter — html, react, vue, react-native-web, or one registered in workbench.config.ts."));
                    continue;
                }
                implementation["root"] = json!(".");
                implementation["adapter"] = json!(adapter);
                let styles = match spec.get("styles") {
                    value if blank(value) => Vec::new(),
                    Some(Value::Array(items)) => items.clone(),
                    Some(other) => vec![other.clone()],
                    None => Vec::new(),
                };
                let mut kept = Vec::new();
                for style in &styles {
                    match project_path(Some(style)) {
                        Some(path) => kept.push(Value::String(path)),
                        None => problems.push(format!("{where_}: styles must be paths inside the project, relative to its root.")),
                    }
                }
                implementation["styles"] = Value::Array(kept);
                if !blank(spec.get("environment")) {
                    match project_path(spec.get("environment")) {
                        Some(path) => implementation["environment"] = Value::String(path),
                        None => problems.push(format!("{where_}: environment must be a path inside the project, relative to its root.")),
                    }
                }
                for field in ["base", "url"] {
                    if spec.contains_key(field) {
                        problems.push(format!("{where_}: {field} doesn’t apply to docs, whose examples Workbench compiles itself."));
                    }
                }
            }
            "ios-simulator" => {
                let device = text(spec.get("device"));
                implementation["device"] = json!(if device.is_empty() {
                    "booted".to_owned()
                } else {
                    device
                });
            }
            "window" => {
                let app = text(spec.get("app"));
                if app.is_empty()
                    || !app
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
                {
                    problems.push(format!("{where_}: needs an app — the application’s bundle ID, or part of it, such as com.example.app."));
                    continue;
                }
                implementation["app"] = json!(app);
            }
            _ => {
                let field = if kind == "url" { "base" } else { "url" };
                let address = text(spec.get(field));
                if kind == "storybook" && spec.contains_key("base") {
                    problems.push(format!("{where_}: Storybook takes url, not base; leave both out to have Workbench run Storybook."));
                    continue;
                } else if kind == "storybook" && address.is_empty() {
                    owned = true;
                    implementation["owned"] = json!(true);
                    implementation["url"] = Value::Null;
                    implementation["root"] = json!(".");
                    if spec.contains_key("start") {
                        problems.push(format!("{where_}: start is for a Storybook you run at url; without url, Workbench runs Storybook itself."));
                    }
                } else if kind == "storybook" && address == "auto" {
                    implementation["url"] = json!("auto");
                    implementation["auto"] = json!(true);
                } else if kind == "url" && crate::gate::target(&address).is_some() {
                    implementation["base"] = json!(address.trim_end_matches('/'));
                } else if !is_http(&address) {
                    let or = if kind == "storybook" {
                        ", or url: auto."
                    } else {
                        "."
                    };
                    problems.push(format!(
                        "{where_}: needs a {field} starting with http:// or https://{or}"
                    ));
                    continue;
                } else {
                    implementation[field] = json!(address.trim_end_matches('/'));
                }
            }
        }
        if let Some(icon) = catalog(spec.get("catalog"), &where_, problems) {
            if kind == "ios-simulator" {
                implementation["catalog"] = json!(true);
                if let CatalogIcon::Configured(icon) = icon {
                    implementation["catalogIcon"] = json!(icon);
                }
            } else {
                problems.push(format!(
                    "{where_}: catalog is only available for iOS Simulator implementations."
                ));
            }
        }
        if spec.contains_key("render") {
            problems.push(format!("{where_}: render is no longer used; remove it. URL and Storybook implementations use iframes."));
        }
        if spec.contains_key("start") && !owned {
            if implementation["base"]
                .as_str()
                .is_some_and(|base| base.starts_with("gate://"))
            {
                problems.push(format!("{where_}: Gate owns service startup; omit start and run gate run TARGET from the task checkout."));
            } else if ["ios-simulator", "window", "workbench", "docs"].contains(&kind.as_str()) {
                problems.push(format!(
                    "{where_}: start is available for URL and Storybook implementations."
                ));
            } else {
                implementation["start"] = startup(spec.get("start"), &where_, problems);
            }
        }
        let root = text(spec.get("root"));
        if !root.is_empty() {
            if (kind == "workbench" || kind == "docs") && root != "." {
                let what = if kind == "workbench" {
                    "Workbench previews"
                } else {
                    "Docs examples"
                };
                problems.push(format!("{where_}: {what} use this project root."));
                continue;
            }
            if has_scheme(&root) {
                problems.push(format!("{where_}: root must be a folder path, not a URL."));
            } else {
                implementation["root"] = json!(root);
            }
        }
        out.insert(key.clone(), implementation);
    }
    js::ordered(out)
}

/// A page's own lens label, when it gives one.
pub(super) fn page_lens_label(
    raw: Option<&Value>,
    item: &mut Map<String, Value>,
    where_: &str,
    problems: &mut Problems,
) {
    let Some(label) = read_lens_label(raw, &format!("{where_}: lensLabel"), problems) else {
        return;
    };
    if is_docs(item["src"].as_str().unwrap_or_default()) {
        problems.push(format!("{where_}: lensLabel is only available for authored pages; docs lenses use implementation labels."));
        return;
    }
    item.insert("lensLabel".to_owned(), Value::String(label));
}

/// The page's Markdown: its src for a Markdown page, or `docs`.
pub(super) fn page_markdown(
    raw: &Value,
    item: &mut Map<String, Value>,
    where_: &str,
    problems: &mut Problems,
) {
    let given = !blank(raw.get("docs"));
    let src = item["src"].as_str().unwrap_or_default().to_owned();
    if is_docs(&src) {
        item.insert("markdown".to_owned(), Value::String(src));
        if given {
            problems.push(format!(
                "{where_}: docs doesn’t apply to a Markdown page, which is its own docs."
            ));
        }
        return;
    }
    if !given {
        return;
    }
    match project_path(raw.get("docs")) {
        Some(file) if is_docs(&file) && src_problem(&file).is_none() => {
            item.insert("markdown".to_owned(), Value::String(file));
        }
        _ => problems.push(format!(
            "{where_}: docs must be a .md file inside the project, relative to its root."
        )),
    }
}

fn address(implementation: &Value) -> String {
    for field in ["base", "url"] {
        if let Some(text) = implementation
            .get(field)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            return text.to_owned();
        }
    }
    String::new()
}

fn path_of(
    value: &Value,
    key: &str,
    implementation: &Value,
    where_: &str,
    problems: &mut Problems,
) -> Option<String> {
    let path = text(Some(value));
    if path.starts_with('/') {
        return Some(path);
    }
    problems.push(format!(
        "{where_}: implementation “{key}” paths must start with / — they’re appended to {}.",
        address(implementation)
    ));
    None
}

/// A lens's path map, from scenario id to a checked path, against the page's
/// scenario ids: `{ path, scenarios }`, or `None` when the first scenario
/// has no path. The first scenario's own id and `default` both name it.
fn scenario_paths(
    paths: &Map<String, Value>,
    key: &str,
    ids: &[String],
    where_: &str,
    problems: &mut Problems,
) -> Option<Value> {
    let first = ids.first().map_or("default", String::as_str);
    let mut path = Value::Null;
    let mut mapped = Map::new();
    for (scenario, found) in paths {
        if scenario == first || scenario == "default" {
            path = found.clone();
        } else if ids.contains(scenario) {
            mapped.insert(scenario.clone(), found.clone());
        } else {
            problems.push(format!("{where_}: implementation “{key}” maps scenario “{scenario}”, which this page doesn’t declare."));
        }
    }
    if path.is_null() {
        problems.push(format!(
            "{where_}: implementation “{key}” needs a path for the default scenario (“{first}”)."
        ));
        return None;
    }
    Some(json!({ "path": path, "scenarios": js::ordered(mapped) }))
}

/// `settleLenses`: a page that lists a preview definition keeps its lens
/// path maps as `{ pending }` until it has the definition's scenarios; this
/// checks them against `ids`, the scenarios it has now, reporting at
/// `where_`, and drops the lenses that don't survive.
pub(crate) fn settle_lenses(
    item: &mut Value,
    ids: &[String],
    where_: &str,
    problems: &mut Vec<String>,
) {
    let Some(lenses) = item.get("implementations").and_then(Value::as_object) else {
        return;
    };
    if !lenses.values().any(|lens| lens.get("pending").is_some()) {
        return;
    }
    let mut out = Map::new();
    for (key, lens) in lenses {
        let settled = match lens.get("pending").and_then(Value::as_object) {
            Some(paths) => scenario_paths(paths, key, ids, where_, problems),
            None => Some(lens.clone()),
        };
        if let Some(settled) = settled {
            out.insert(key.clone(), settled);
        }
    }
    let Some(item) = item.as_object_mut() else {
        return;
    };
    if out.is_empty() {
        item.remove("implementations");
    } else {
        item.insert("implementations".to_owned(), Value::Object(out));
    }
}

/// The lenses one page has, or `None` when none survived. `ids` are the
/// page's valid scenario ids, or `None` when its scenarios come later from
/// its preview definition, which leaves its path maps pending.
pub(super) fn page_lenses(
    raw: Option<&Value>,
    item: &Map<String, Value>,
    implementations: &Map<String, Value>,
    where_: &str,
    problems: &mut Problems,
    ids: Option<&[String]>,
) -> Option<Value> {
    let raw = raw.filter(|raw| !blank(Some(raw)))?;
    let Some(mapped) = raw.as_object() else {
        problems.push(format!(
            "{where_}: implementations must be a map of implementation names to paths."
        ));
        return None;
    };
    let mut out = Map::new();
    for (key, value) in mapped {
        let Some(implementation) = implementations.get(key) else {
            problems.push(format!(
                "{where_}: implementation “{key}” isn’t declared under implementations."
            ));
            continue;
        };
        let kind = implementation["kind"].as_str().unwrap_or_default();
        let single = |name: &str, problem: String, problems: &mut Problems| {
            let found = text(Some(value));
            if found.is_empty() {
                problems.push(problem);
                None
            } else {
                Some(json!({ name: found }))
            }
        };
        let lens = match kind {
            "docs" => {
                if !item.contains_key("markdown") {
                    problems.push(format!("{where_}: docs lens “{key}” needs the page’s Markdown — a .md src, or docs: with a .md file."));
                    None
                } else if let Some(source) = project_path(Some(value)) {
                    Some(json!({ "examples": source }))
                } else {
                    problems.push(format!("{where_}: docs lens “{key}” needs an example folder (ending in /) or file, relative to the project root."));
                    None
                }
            }
            "storybook" => single(
                "title",
                format!("{where_}: implementation “{key}” needs a story title."),
                problems,
            ),
            "workbench" => {
                let preview = text(Some(value));
                if !preview.is_empty() && preview.split('/').all(is_key) {
                    Some(json!({ "preview": preview }))
                } else {
                    problems.push(format!("{where_}: Workbench lens needs a preview ID."));
                    None
                }
            }
            "ios-simulator" => single(
                "device",
                format!("{where_}: implementation “{key}” needs a simulator device name or UDID."),
                problems,
            ),
            "window" => single(
                "window",
                format!(
                    "{where_}: implementation “{key}” needs the window’s title, or part of it."
                ),
                problems,
            ),
            _ => match value {
                Value::String(_) | Value::Number(_) => {
                    path_of(value, key, implementation, where_, problems)
                        .map(|path| json!({ "path": path }))
                }
                Value::Object(by_scenario) => {
                    let mut paths = Map::new();
                    for (scenario, entry) in by_scenario {
                        if let Some(found) = path_of(entry, key, implementation, where_, problems) {
                            paths.insert(scenario.clone(), Value::String(found));
                        }
                    }
                    let paths = js::ordered(paths);
                    match ids {
                        None => Some(json!({ "pending": paths })),
                        Some(ids) => scenario_paths(&paths, key, ids, where_, problems),
                    }
                }
                _ => {
                    problems.push(format!("{where_}: implementation “{key}” needs a path, or a map of this page’s scenario ids to paths."));
                    None
                }
            },
        };
        if let Some(lens) = lens {
            out.insert(key.clone(), lens);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(js::ordered(out)))
    }
}

/// A Markdown page's lens and its rules; the built-in Docs lens's key.
pub(super) fn docs_page_entry(
    raw: &Value,
    item: &mut Map<String, Value>,
    implementations: &Map<String, Value>,
    where_: &str,
    problems: &mut Problems,
) {
    let lens = text(raw.get("lens"));
    let mapped: Vec<String> = item
        .get("implementations")
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    let docs_keys: Vec<String> = mapped
        .iter()
        .filter(|key| {
            implementations
                .get(key.as_str())
                .is_some_and(|implementation| implementation["kind"] == "docs")
        })
        .cloned()
        .collect();
    if item.contains_key("markdown")
        && docs_keys.is_empty()
        && mapped.iter().any(|key| key == "docs")
    {
        problems.push(format!("{where_}: “docs” names this page’s built-in Docs lens; give implementation “docs” another name."));
    }
    if !is_docs(item["src"].as_str().unwrap_or_default()) {
        if !lens.is_empty() {
            problems.push(format!(
                "{where_}: lens is only for Markdown pages (a .md src)."
            ));
        }
        return;
    }
    if !blank(raw.get("sizes")) {
        problems.push(format!(
            "{where_}: sizes don’t apply to a Markdown page, which uses the whole canvas."
        ));
    }
    if !lens.is_empty() && !docs_keys.contains(&lens) {
        let list = if docs_keys.is_empty() {
            ".".to_owned()
        } else {
            format!(" ({}).", docs_keys.join(", "))
        };
        problems.push(format!(
            "{where_}: lens “{lens}” isn’t one of this page’s docs lenses{list}"
        ));
    } else if !lens.is_empty() {
        item.insert("lens".to_owned(), Value::String(lens));
    }
    if !item.contains_key("lens")
        && let Some(first) = docs_keys.first()
    {
        item.insert("lens".to_owned(), Value::String(first.clone()));
    }
}

/// Where a page's implementation code lives: `[{ implementation, path }]`.
pub(super) fn page_code(
    raw: Option<&Value>,
    implementations: &Map<String, Value>,
    where_: &str,
    problems: &mut Problems,
) -> Option<Value> {
    let raw = raw.filter(|raw| !blank(Some(raw)))?;
    let Some(by_implementation) = raw.as_object() else {
        problems.push(format!(
            "{where_}: code must be a map of implementation names to paths."
        ));
        return None;
    };
    let mut out = Vec::new();
    for (key, value) in by_implementation {
        if !implementations.contains_key(key) {
            problems.push(format!("{where_}: code names implementation “{key}”, which isn’t declared under implementations."));
            continue;
        }
        let values = match value {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        let mut found = 0;
        for value in &values {
            let path = text(Some(value));
            if !path.is_empty() {
                out.push(json!({ "implementation": key, "path": path }));
                found += 1;
            }
        }
        if found == 0 {
            problems.push(format!(
                "{where_}: code for “{key}” needs a path relative to that implementation’s root."
            ));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Array(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_src_may_not_contain_the_iteration_mark() {
        assert_eq!(src_problem("pages/sign-in.html"), None);
        assert_eq!(
            src_problem("routes/+page.html").as_deref(),
            Some(
                "src “routes/+page.html” can’t contain “+” — “:”, “+”, “!”, and “~” mark the scenario, the iteration, the example, and the lens in the address."
            )
        );
        assert!(
            src_problem("a:b+c.html")
                .is_some_and(|problem| problem.contains("can’t contain “:” or “+”"))
        );
    }

    #[test]
    fn scenario_iterations_are_kebab_case_unique_and_labelled_from_their_id() {
        let mut problems = Vec::new();
        let raw = json!([
            { "id": "inline-hint" },
            { "id": "compact", "label": "  Tight  " },
            { "id": "Bold" },
            { "id": "2-up" },
            { "label": "No id" },
            { "id": "compact", "label": "Again" },
            "just-text",
        ]);
        let found = scenario_iterations(Some(&raw), "Pages › Sign in", "error", &mut problems);
        assert_eq!(
            found,
            Some(json!([
                { "id": "inline-hint", "label": "Inline hint" },
                { "id": "compact", "label": "Tight" },
            ]))
        );
        assert_eq!(
            problems,
            [
                "Pages › Sign in › scenario error: iteration id “Bold” must be kebab-case — it travels in a URL.",
                "Pages › Sign in › scenario error: iteration id “2-up” must be kebab-case — it travels in a URL.",
                "Pages › Sign in › scenario error: iteration 5 needs an id.",
                "Pages › Sign in › scenario error: iteration id “compact” is declared twice.",
                "Pages › Sign in › scenario error: iteration 7 needs an id.",
            ]
        );
    }

    #[test]
    fn page_scenarios_share_the_id_rule_default_their_labels_and_report_repeats() {
        let mut problems = Vec::new();
        let raw = json!([
            { "id": "default", "label": "Default" },
            { "id": "compact-table" },
            { "id": "2-up", "label": "Two up" },
            { "id": "a--b", "label": "Odd" },
            { "label": "No id" },
            { "id": 7, "label": "Number" },
            { "id": "compact-table", "label": "Again" },
        ]);
        let (ids, kept) = page_scenarios(Some(&raw), "Pages › Sign in", &mut problems);
        assert_eq!(ids, ["default", "compact-table"]);
        assert_eq!(
            kept,
            Some(json!([
                { "id": "default", "label": "Default" },
                { "id": "compact-table", "label": "Compact table" },
            ]))
        );
        assert_eq!(
            problems,
            [
                "Pages › Sign in: scenario id “2-up” must be kebab-case — it travels in a URL.",
                "Pages › Sign in: scenario id “a--b” must be kebab-case — it travels in a URL.",
                "Pages › Sign in: scenario 5 needs an id.",
                "Pages › Sign in: scenario 6 needs an id.",
                "Pages › Sign in: scenario id “compact-table” is declared twice.",
            ]
        );
        // One scenario is no choice, but its id still names the page's first.
        let (ids, kept) = page_scenarios(
            Some(&json!([{ "id": "filled" }])),
            "Pages › Library",
            &mut problems,
        );
        assert_eq!((ids, kept), (vec!["filled".to_owned()], None));
    }

    #[test]
    fn a_lone_scenario_names_the_default_path_by_its_own_id_or_default() {
        let implementations = implementations(
            Some(&json!({ "dev": { "kind": "url", "base": "http://localhost:3000" } })),
            &mut Vec::new(),
        );
        let item = Map::from_iter([("src".to_owned(), json!("library.html"))]);
        let ids = ["filled".to_owned()];
        for key in ["filled", "default"] {
            let mut problems = Vec::new();
            let raw = json!({ "dev": { key: "/library" } });
            assert_eq!(
                page_lenses(
                    Some(&raw),
                    &item,
                    &implementations,
                    "App › Library",
                    &mut problems,
                    Some(&ids[..])
                ),
                Some(json!({ "dev": { "path": "/library", "scenarios": {} } }))
            );
            assert!(problems.is_empty());
        }
        // A preview definition's page leaves its path maps for later.
        let mut problems = Vec::new();
        let raw = json!({ "dev": { "empty": "/library?empty", "bad": "relative" } });
        assert_eq!(
            page_lenses(
                Some(&raw),
                &item,
                &implementations,
                "App › Library",
                &mut problems,
                None
            ),
            Some(json!({ "dev": { "pending": { "empty": "/library?empty" } } }))
        );
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn only_a_configured_catalog_icon_is_kept() {
        let mut problems = Vec::new();
        let found = implementations(
            Some(&json!({
                "plain": { "kind": "ios-simulator", "catalog": true },
                "tablets": { "kind": "ios-simulator", "catalog": { "icon": "tablet" } },
                "odd": { "kind": "ios-simulator", "catalog": { "icon": "Not Kebab" } },
            })),
            &mut problems,
        );
        assert_eq!(found["plain"].get("catalogIcon"), None);
        assert_eq!(found["tablets"]["catalogIcon"], "tablet");
        assert_eq!(found["odd"].get("catalogIcon"), None);
        assert_eq!(found["odd"]["catalog"], true);
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn absent_or_empty_iterations_are_none_and_a_non_list_is_a_problem() {
        let mut problems = Vec::new();
        assert_eq!(
            scenario_iterations(None, "Pages › Sign in", "error", &mut problems),
            None
        );
        assert_eq!(
            scenario_iterations(
                Some(&Value::Null),
                "Pages › Sign in",
                "error",
                &mut problems
            ),
            None
        );
        assert_eq!(
            scenario_iterations(Some(&json!([])), "Pages › Sign in", "error", &mut problems),
            None
        );
        assert!(problems.is_empty());
        assert_eq!(
            scenario_iterations(
                Some(&json!("bold")),
                "Pages › Sign in",
                "error",
                &mut problems
            ),
            None
        );
        assert_eq!(
            problems,
            [
                "Pages › Sign in › scenario error: iterations must be a list of entries with an id and an optional label."
            ]
        );
    }
}
