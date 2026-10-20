//! SVG sprite modules: every icon in the configured folders becomes a
//! `<symbol>` in one sprite that a registration module inserts into the
//! document, and a names module exports the symbol IDs. Symbols are made as
//! `vite-plugin-svg-icons` makes them: comments, `<title>`, `<desc>` and
//! `<metadata>` removed, the first stroke color made `currentColor`, a view
//! box from the icon's size when it has none, IDs inside an icon prefixed
//! with its symbol ID (with the references to them), and only the root's
//! presentation attributes kept. The folders and icons are dependencies of
//! both modules, so adding, editing or deleting an icon rebuilds them.
use regex::{Captures, Regex};
use rspack_cacheable::{cacheable, cacheable_dyn};
use rspack_core::RunnerContext;
use rspack_error::{Result, error};
use rspack_loader_runner::{Identifier, Loader, LoaderContext};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sprite {
    pub(crate) dirs: Vec<PathBuf>,
    pub(crate) symbol_id: String,
    pub(crate) inject: String,
    pub(crate) dom_id: String,
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("valid SVG pattern")
}

static REMOVED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?is)<\?xml.*?\?>|<!DOCTYPE[^>]*>|<!--.*?-->|<title\b[^>]*>.*?</title>|<desc\b[^>]*>.*?</desc>|<metadata\b[^>]*>.*?</metadata>",
    )
});
static STROKE: LazyLock<Regex> = LazyLock::new(|| pattern(r#"stroke="[a-zA-Z#0-9]*""#));
static ROOT: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?is)<svg\b([^>]*)>(.*)</svg\s*>"));
static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'))?"#));
static ID: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(\sid\s*=\s*)(?:"([^"]*)"|'([^']*)')"#));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r#"url\(\s*(["']?)#([^)"']+)(["']?)\s*\)|((?:xlink:)?href\s*=\s*["'])#([^"']+)"#)
});

/// svg-baker's preserved root attributes: `viewBox`, `preserveAspectRatio`,
/// `class`, `overflow`, `stroke*`, `fill*`, `xmlns*`, `role`, `aria-*`.
fn preserved(name: &str) -> bool {
    matches!(
        name,
        "viewBox" | "preserveAspectRatio" | "class" | "overflow" | "role"
    ) || name == "stroke"
        || name.starts_with("stroke-")
        || name == "fill"
        || name.starts_with("fill-")
        || name == "xmlns"
        || name.starts_with("xmlns:")
        || name.starts_with("aria-")
}

/// The symbol ID for an icon at `relative` inside its folder.
pub(crate) fn symbol_id(template: &str, relative: &str) -> String {
    let (dir, file) = relative
        .rsplit_once('/')
        .map_or((String::new(), relative), |(dir, file)| {
            (dir.replace('/', "-"), file)
        });
    let mut id = template.to_owned();
    if id.contains("[dir]") {
        id = id.replace("[dir]", &dir);
        if dir.is_empty() {
            id = id.replacen("--", "-", 1);
        }
    }
    id = id.replace("[name]", file);
    match id.rfind('.') {
        Some(at) if !id[at..].contains('/') => {
            let extension = id[at..].to_owned();
            id.replacen(&extension, "", 1)
        }
        _ => id,
    }
}

fn number(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches("px");
    trimmed.parse::<f64>().ok().map(|_| trimmed.to_owned())
}

/// One icon's markup as a `<symbol>` with the given ID.
pub(crate) fn symbol(content: &str, id: &str) -> Result<String, String> {
    let content = REMOVED.replace_all(content, "");
    let content = STROKE.replace(&content, "stroke=\"currentColor\"");
    let root = ROOT
        .captures(&content)
        .ok_or_else(|| "an icon must have an <svg> root".to_owned())?;
    let mut attributes: Vec<(String, Option<String>)> = ATTRIBUTE
        .captures_iter(&root[1])
        .map(|found| {
            (
                found[1].to_owned(),
                found
                    .get(2)
                    .or_else(|| found.get(3))
                    .map(|value| value.as_str().to_owned()),
            )
        })
        .collect();
    let value = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| value.clone())
    };
    if value("viewBox").is_none()
        && let (Some(width), Some(height)) = (
            value("width").as_deref().and_then(number),
            value("height").as_deref().and_then(number),
        )
    {
        attributes.push(("viewBox".to_owned(), Some(format!("0 0 {width} {height}"))));
    }
    attributes.retain(|(name, _)| preserved(name));
    let inner = ID.replace_all(&root[2], |found: &Captures<'_>| {
        let original = found
            .get(2)
            .or_else(|| found.get(3))
            .map_or("", |value| value.as_str());
        format!("{}\"{id}_{original}\"", &found[1])
    });
    let inner = REFERENCE.replace_all(&inner, |found: &Captures<'_>| {
        if let Some(target) = found.get(2) {
            format!("url({}#{id}_{}{})", &found[1], target.as_str(), &found[3])
        } else {
            format!("{}#{id}_{}", &found[4], &found[5])
        }
    });
    let mut out = format!("<symbol id=\"{id}\"");
    for (name, value) in attributes {
        match value {
            Some(value) => {
                let _ = write!(out, " {name}=\"{value}\"");
            }
            None => {
                let _ = write!(out, " {name}");
            }
        }
    }
    out.push('>');
    out.push_str(&inner);
    out.push_str("</symbol>");
    Ok(out)
}

fn icons(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => icons(&path, files),
            Ok(kind)
                if kind.is_file()
                    && path.extension().is_some_and(|extension| extension == "svg") =>
            {
                files.push(path);
            }
            _ => {}
        }
    }
}

/// The sprite's symbols and IDs, with every file read.
pub(crate) fn compile(sprite: &Sprite) -> Result<(String, Vec<String>, Vec<PathBuf>), String> {
    let mut html = String::new();
    let mut ids: Vec<String> = vec![];
    let mut read = vec![];
    for folder in &sprite.dirs {
        let mut files = vec![];
        icons(folder, &mut files);
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(folder)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let id = symbol_id(&sprite.symbol_id, &relative);
            let content = std::fs::read_to_string(&file)
                .map_err(|error| format!("{}: {error}", file.display()))?;
            html.push_str(
                &symbol(&content, &id).map_err(|error| format!("{}: {error}", file.display()))?,
            );
            if !ids.contains(&id) {
                ids.push(id);
            }
            read.push(file);
        }
    }
    let html = html
        .replace("xmlns=\"http://www.w3.org/2000/svg\"", "")
        .replace("xmlns:xlink=\"http://www.w3.org/1999/xlink\"", "");
    Ok((html, ids, read))
}

/// The registration module: inserts the sprite once the body exists.
pub(crate) fn register_source(sprite: &Sprite, html: &str) -> String {
    let inject = if sprite.inject == "body-first" {
        "body.insertBefore(svgDom, body.firstChild);"
    } else {
        "body.insertBefore(svgDom, body.lastChild);"
    };
    let dom_id = super::super::entries::quote(&sprite.dom_id);
    format!(
        "if (typeof window !== 'undefined') {{\n  function loadSvg() {{\n    var body = document.body;\n    var svgDom = document.getElementById({dom_id});\n    if (!svgDom) {{\n      svgDom = document.createElementNS('http://www.w3.org/2000/svg', 'svg');\n      svgDom.style.position = 'absolute';\n      svgDom.style.width = '0';\n      svgDom.style.height = '0';\n      svgDom.id = {dom_id};\n      svgDom.setAttribute('xmlns', 'http://www.w3.org/2000/svg');\n      svgDom.setAttribute('xmlns:link', 'http://www.w3.org/1999/xlink');\n    }}\n    svgDom.innerHTML = {};\n    {inject}\n  }}\n  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', loadSvg);\n  else loadSvg();\n}}\nexport default {{}};\n",
        super::super::entries::quote(html)
    )
}

#[derive(Serialize, Deserialize)]
struct Options {
    sprite: Sprite,
    /// `register` or `names`.
    module: String,
}

/// Generates a sprite's registration or names module over its placeholder.
#[cacheable]
pub(crate) struct SpriteLoader {
    identifier: Identifier,
    options: String,
}

impl SpriteLoader {
    pub(crate) fn new(identifier: Identifier, options: &str) -> Self {
        Self {
            identifier,
            options: options.to_owned(),
        }
    }
}

/// The loader options for one of a sprite's modules.
pub(crate) fn loader_options(sprite: &Sprite, module: &str) -> serde_json::Value {
    serde_json::to_value(Options {
        sprite: sprite.clone(),
        module: module.to_owned(),
    })
    .unwrap_or_default()
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for SpriteLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let options: Options = serde_json::from_str(&self.options)
            .map_err(|problem| error!("SVG sprite options: {problem}"))?;
        let _placeholder = context.take_content();
        let (html, ids, read) =
            compile(&options.sprite).map_err(|problem| error!("SVG sprite: {problem}"))?;
        for folder in &options.sprite.dirs {
            context.add_context_dependency(folder.clone());
        }
        for file in read {
            context.add_file_dependency(file);
        }
        let source = if options.module == "names" {
            format!(
                "export default {};\n",
                serde_json::to_string(&ids).unwrap_or_default()
            )
        } else {
            register_source(&options.sprite, &html)
        };
        context.finish_with((source, None, None));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_ids_follow_the_template() {
        assert_eq!(
            symbol_id("icon-[name]", "arrow-left.svg"),
            "icon-arrow-left"
        );
        assert_eq!(
            symbol_id("icon-[dir]-[name]", "arrow-left.svg"),
            "icon-arrow-left"
        );
        assert_eq!(
            symbol_id("icon-[dir]-[name]", "nav/arrows/up.svg"),
            "icon-nav-arrows-up"
        );
    }

    #[test]
    fn an_icon_becomes_a_symbol_with_its_own_ids_and_view_box() {
        let icon = concat!(
            "<?xml version=\"1.0\"?><!-- drawn by Acme -->",
            "<svg width=\"32\" height=\"24\" fill=\"none\" class=\"acme\" data-x=\"1\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">",
            "<title>apps</title><defs><linearGradient id=\"a\"/><path id='b'/></defs>",
            "<path stroke=\"#111\" fill=\"url(#a)\"/><use xlink:href=\"#b\"/><path stroke=\"#222\"/></svg>"
        );
        let symbol = symbol(icon, "icon-apps").unwrap();
        assert!(symbol.starts_with("<symbol id=\"icon-apps\" fill=\"none\" class=\"acme\" xmlns="));
        assert!(symbol.contains(" viewBox=\"0 0 32 24\">"));
        assert!(
            !symbol.contains("width=") && !symbol.contains("data-x") && !symbol.contains("<title>")
        );
        assert!(symbol.contains("<linearGradient id=\"icon-apps_a\"/>"));
        assert!(symbol.contains("<path id=\"icon-apps_b\"/>"));
        assert!(symbol.contains("fill=\"url(#icon-apps_a)\""));
        assert!(symbol.contains("xlink:href=\"#icon-apps_b\""));
        assert!(symbol.contains("<path stroke=\"currentColor\""));
        assert!(
            symbol.contains("<path stroke=\"#222\"/>"),
            "only the first stroke changes"
        );
        assert!(symbol.ends_with("</symbol>"));
        assert!(super::symbol("<g/>", "x").is_err());
    }

    #[test]
    fn a_sprite_lists_every_icon_and_registers_its_symbols() {
        let dir = std::env::temp_dir().join(format!("acme-sprite-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nav")).unwrap();
        std::fs::write(dir.join("b.svg"), "<svg viewBox=\"0 0 1 1\"><path/></svg>").unwrap();
        std::fs::write(dir.join("a.svg"), "<svg viewBox=\"0 0 2 2\"><path/></svg>").unwrap();
        std::fs::write(
            dir.join("nav/up.svg"),
            "<svg viewBox=\"0 0 3 3\"/>".replace("/>", "></svg>"),
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not an icon").unwrap();
        let sprite = Sprite {
            dirs: vec![dir.clone()],
            symbol_id: "icon-[dir]-[name]".into(),
            inject: "body-last".into(),
            dom_id: "__svg__icons__dom__".into(),
        };
        let (html, ids, read) = compile(&sprite).unwrap();
        assert_eq!(ids, ["icon-a", "icon-b", "icon-nav-up"]);
        assert_eq!(read.len(), 3);
        assert!(html.starts_with("<symbol id=\"icon-a\" viewBox=\"0 0 2 2\">"));
        let module = register_source(&sprite, &html);
        assert!(module.contains("document.getElementById(\"__svg__icons__dom__\")"));
        assert!(module.contains("body.insertBefore(svgDom, body.lastChild);"));
        assert!(module.ends_with("export default {};\n"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
