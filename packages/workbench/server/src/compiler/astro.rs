//! The built-in Astro adapter: each scenario renders in the Node evaluator with
//! the project's own Astro, and the rendered HTML and its resources (styles
//! and client scripts) become generated modules the compiler builds through
//! the HTML loader, with shared chunks and hot updates. An edit to a file a
//! render read renders it again. An input edit renders once more and answers
//! with its own assets, served from memory.
use super::Profile;
use super::client::EntryKind;
use super::docs;
use super::entries::{
    AdapterImport, DocsEntry, DocsExamples, PreviewEntry, SourceImport, digest, entry_slug,
    preview_entry_source, quote,
};
use super::host::Module;
use super::previews::{Entry, Previews};
use super::project::{self, ProjectConfig};
use anyhow::{Context as _, Result, anyhow};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static PUBLIC_URL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\b(src|href|poster)=(?:"(/[^"?#]+)|'(/[^'?#]+))"#).expect("valid pattern")
});

fn is_css(id: &str) -> bool {
    Path::new(id)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("css"))
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A render as modules: each resource Astro emitted as its own file beside
/// the component it came from, the HTML naming them with `wbfile:`, and the
/// module that exports `html` and `inputs`. Root-relative URLs name files in
/// the project's public folder. Returns the module and every generated file.
pub(crate) fn render_modules(
    virtual_dir: &Path,
    key: &str,
    component: &Path,
    html: &str,
    resources: &Map<String, Value>,
    inputs: &Value,
    public_dir: Option<&Path>,
) -> (String, Vec<Module>) {
    let mut modules = vec![];
    let mut html = html.to_owned();
    for (id, resource) in resources {
        let folder = resource["resolveDir"]
            .as_str()
            .map_or_else(|| virtual_dir.join("astro"), PathBuf::from);
        let extension = if is_css(id) { ".css" } else { ".ts" };
        let file = folder.join(format!(".workbench-astro-{}{extension}", digest(id)));
        modules.push(Module {
            file: text(&file),
            source: resource["contents"].as_str().unwrap_or_default().to_owned(),
        });
        html = html.replace(id.as_str(), &format!("wbfile:{}", text(&file)));
    }
    if let Some(public_dir) = public_dir {
        html = PUBLIC_URL
            .replace_all(&html, |found: &regex::Captures<'_>| {
                let quote_mark = if found.get(2).is_some() { '"' } else { '\'' };
                let url = found
                    .get(2)
                    .or_else(|| found.get(3))
                    .map_or("", |url| url.as_str());
                let target = public_dir.join(url.trim_start_matches('/'));
                if target.exists() {
                    format!("{}={quote_mark}wbfile:{}", &found[1], text(&target))
                } else {
                    found[0].to_owned()
                }
            })
            .into_owned();
    }
    // Beside the component, so relative URLs in it resolve as the author wrote them.
    let html_file = component
        .parent()
        .unwrap_or(virtual_dir)
        .join(format!(".workbench-astro-{}.html", digest(key)));
    modules.push(Module {
        file: text(&html_file),
        source: html,
    });
    let module = virtual_dir.join("astro").join(format!("{key}.js"));
    modules.push(Module {
        file: text(&module),
        source: format!(
            "export {{ default as html }} from {};\nexport const inputs = {};\n",
            quote(&text(&html_file)),
            serde_json::to_string(inputs).unwrap_or_else(|_| "{}".into())
        ),
    });
    (text(&module), modules)
}

/// A render's module name: the slug and its source key, with an artboard or
/// iteration key's `+` and `/` made file-name safe. IDs are kebab-case, so
/// the replacements cannot collide with another key's name.
pub(crate) fn render_key(slug: &str, key: &str) -> String {
    format!("{slug}-{}", key.replace('+', "_").replace('/', "."))
}

fn dependencies(answer: &Value) -> Vec<PathBuf> {
    answer["dependencies"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect()
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// The render keys of one preview: `default`, each scenario, each declared
/// artboard that changes the server render (`<scenario>+<artboard>/`), and
/// each iteration that does, under its source key (`<scenario>+<iteration>`
/// or `<scenario>+<artboard>/<iteration>`).
fn rendered_keys(definition: &Value, answer: &Value) -> Vec<String> {
    let mut keys = vec!["default".to_owned()];
    if let Some(map) = definition["scenarios"].as_object() {
        keys.extend(map.keys().cloned());
    }
    keys.extend(
        answer["scenarios"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, _)| key)
            .filter(|key| key.contains('+'))
            .cloned(),
    );
    keys
}

pub(super) async fn preview_entry(
    previews: &Previews,
    relative: &str,
    file: &Path,
    definition: &Value,
    config: &ProjectConfig,
) -> Result<Entry> {
    let id = definition["id"].as_str().context("definition id")?;
    let slug = entry_slug(id);
    let answer = previews
        .evaluator()
        .call(
            "astro",
            json!({ "file": relative, "options": previews.options().await }),
        )
        .await
        .with_context(|| format!("{relative}: Astro render"))?;
    let public_dir = answer["publicDir"].as_str().map(PathBuf::from);
    let empty = Map::new();
    let resources = answer["resources"].as_object().unwrap_or(&empty);
    let mut sources = vec![];
    let mut modules = vec![];
    let render_url = format!("/_workbench/previews/render?file={}", encode(relative));
    for scenario in rendered_keys(definition, &answer) {
        let rendered = &answer["scenarios"][&scenario];
        let component = rendered["component"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_default();
        let (module, written) = render_modules(
            previews.virtual_dir(),
            &render_key(&slug, &scenario),
            &component,
            rendered["html"].as_str().unwrap_or_default(),
            resources,
            &rendered["inputs"],
            public_dir.as_deref(),
        );
        modules.extend(written);
        sources.push(SourceImport::Astro {
            scenario,
            module,
            render_url: Some(render_url.clone()),
        });
    }
    previews.watch_render(
        &slug,
        dependencies(&answer),
        super::previews::Render::Preview(relative.to_owned()),
    );
    let adapter = project::adapter_import(
        config,
        previews.root(),
        &previews.package().preview_dir,
        "astro",
    )
    .map_err(|error| anyhow!(error))?;
    let project_environment = project::project_environment(config, previews.root(), "astro")
        .map_err(|error| anyhow!(error))?
        .map(|file| text(&file));
    let directory = file.parent().unwrap_or(previews.root());
    let own_environment = definition["environment"]
        .as_str()
        .map(|environment| text(&crate::paths::resolve(directory, environment)));
    let styles: Vec<String> = definition["styles"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(|style| text(&crate::paths::resolve(directory, style)))
        .collect();
    let source = preview_entry_source(&PreviewEntry {
        slug: &slug,
        definition: &text(file),
        adapter: &adapter,
        sources: &sources,
        project_environment: project_environment.as_deref(),
        own_environment: own_environment.as_deref(),
        styles: &styles,
        portable: None,
    });
    Ok(Entry {
        file: text(&previews.virtual_dir().join(format!("{slug}.js"))),
        slug,
        kind: EntryKind::Preview,
        profile: Profile::Web,
        title: definition["title"].as_str().unwrap_or(id).to_owned(),
        source,
        examples: vec![],
        problems: vec![],
        modules,
    })
}

pub(super) async fn docs_entry(
    previews: &Previews,
    raw: &Value,
    slug: &str,
    request: &docs::Request,
    listed: &docs::Listed,
    config: &ProjectConfig,
) -> Result<Entry> {
    let mut examples = vec![];
    let mut modules = vec![];
    let mut read = vec![];
    for example in &listed.examples {
        let answer = previews
            .evaluator()
            .call(
                "astroExample",
                json!({ "file": text(&example.file), "options": previews.options().await }),
            )
            .await
            .with_context(|| format!("{}: Astro render", example.file.display()))?;
        let empty = Map::new();
        let (module, written) = render_modules(
            previews.virtual_dir(),
            &format!("{slug}-{}-default", example.id),
            &example.file,
            answer["html"].as_str().unwrap_or_default(),
            answer["resources"].as_object().unwrap_or(&empty),
            &json!({}),
            answer["publicDir"].as_str().map(Path::new),
        );
        modules.extend(written);
        read.extend(dependencies(&answer));
        examples.push((example.id.clone(), module));
    }
    previews.watch_render(slug, read, super::previews::Render::Docs(raw.clone()));
    let adapter: AdapterImport = project::adapter_import(
        config,
        previews.root(),
        &previews.package().preview_dir,
        "astro",
    )
    .map_err(|error| anyhow!(error))?;
    let project_environment = project::project_environment(config, previews.root(), "astro")
        .map_err(|error| anyhow!(error))?
        .map(|file| text(&file));
    let lens_environment = request.environment.as_deref().map(text);
    let styles: Vec<String> = request.styles.iter().map(|style| text(style)).collect();
    let source = super::entries::docs_entry_source(&DocsEntry {
        slug,
        adapter: &adapter,
        project_environment: project_environment.as_deref(),
        lens_environment: lens_environment.as_deref(),
        styles: &styles,
        examples: &DocsExamples::Astro(examples),
        portable: None,
    });
    Ok(Entry {
        file: text(&previews.virtual_dir().join(format!("{slug}.js"))),
        slug: slug.to_owned(),
        kind: EntryKind::Docs,
        profile: Profile::Web,
        title: slug.to_owned(),
        source,
        examples: previews.listed_examples(listed),
        problems: listed.problems.clone(),
        modules,
    })
}

/// One input edit's render: the HTML with its styles served from memory
/// under `/_workbench/previews/preview/<variant>/`. Client scripts already
/// ran with the scenario's own render, so the variant leaves them out.
pub(crate) struct Variant {
    pub(crate) html: String,
    pub(crate) assets: HashMap<String, Vec<u8>>,
}

pub(super) async fn render_variant(
    previews: &Previews,
    relative: &str,
    scenario: &str,
    inputs: &Value,
) -> Result<Variant> {
    let answer = previews
        .evaluator()
        .call(
            "astro",
            json!({ "file": relative, "scenario": scenario, "inputs": inputs, "options": previews.options().await }),
        )
        .await?;
    let rendered = &answer["scenarios"][scenario];
    let mut html = rendered["html"].as_str().unwrap_or_default().to_owned();
    let mut assets = HashMap::new();
    if let Some(resources) = answer["resources"].as_object() {
        for (id, resource) in resources {
            if is_css(id) {
                let name = format!("assets/{}.css", digest(id));
                assets.insert(
                    name.clone(),
                    resource["contents"]
                        .as_str()
                        .unwrap_or_default()
                        .as_bytes()
                        .to_vec(),
                );
                html = html.replace(id.as_str(), &format!("./{name}"));
            } else {
                let script = regex::Regex::new(&format!(
                    r#"(?s)<script\b[^>]*>\s*import\s*["']{}["'];?\s*</script>"#,
                    regex::escape(id)
                ))
                .expect("valid pattern");
                html = script.replace_all(&html, "").into_owned();
            }
        }
    }
    Ok(Variant { html, assets })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteration_renders_get_file_name_safe_modules() {
        assert_eq!(render_key("card-1", "default"), "card-1-default");
        assert_eq!(
            render_key("card-1", "empty+illustrated"),
            "card-1-empty_illustrated"
        );
        assert_eq!(
            render_key("card-1", "default+warning/bold"),
            "card-1-default_warning.bold"
        );
        assert_eq!(
            render_key("card-1", "default+warning/"),
            "card-1-default_warning."
        );
        let keys = [
            "default",
            "empty",
            "empty+illustrated",
            "default+warning/",
            "default+warning/bold",
            "default+info/",
        ];
        let names: std::collections::HashSet<String> =
            keys.iter().map(|key| render_key("card-1", key)).collect();
        assert_eq!(names.len(), keys.len());
    }

    #[test]
    fn declared_artboard_and_iteration_renders_are_rendered_keys() {
        let definition = json!({ "scenarios": { "empty": {}, "feedback": {} } });
        let answer = json!({ "scenarios": {
            "default": {},
            "empty": {},
            "feedback": {},
            "feedback+warning/": {},
            "feedback+warning/bold": {},
            "empty+illustrated": {},
        } });
        let mut keys = rendered_keys(&definition, &answer);
        keys.sort();
        assert_eq!(
            keys,
            [
                "default",
                "empty",
                "empty+illustrated",
                "feedback",
                "feedback+warning/",
                "feedback+warning/bold",
            ]
        );
    }

    #[test]
    fn a_render_becomes_modules_naming_its_resources_and_public_files() {
        let dir = std::env::temp_dir().join(format!("acme-astro-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::write(dir.join("public/logo.svg"), "<svg/>").unwrap();
        let resources = json!({
            "workbench:astro-resource:abc.css": { "contents": ".card{}", "resolveDir": dir.join("src").to_string_lossy() },
            "workbench:astro-resource:def.js": { "contents": "console.log(1)", "resolveDir": null },
        });
        let html = "<link rel=\"stylesheet\" href=\"workbench:astro-resource:abc.css\"><img src=\"/logo.svg\"><img src=\"/missing.svg\"><script type=\"module\">import \"workbench:astro-resource:def.js\";</script>";
        let (module, modules) = render_modules(
            &dir.join(".workbench-rspack"),
            "card-default",
            &dir.join("src/Card.astro"),
            html,
            resources.as_object().unwrap(),
            &json!({ "label": "Hi" }),
            Some(&dir.join("public")),
        );
        assert_eq!(
            module,
            text(&dir.join(".workbench-rspack/astro/card-default.js"))
        );
        let page = modules
            .iter()
            .find(|module| {
                Path::new(&module.file)
                    .extension()
                    .is_some_and(|extension| extension == "html")
            })
            .unwrap();
        assert!(
            page.file
                .starts_with(&text(&dir.join("src/.workbench-astro-")))
        );
        assert!(page.source.contains(&format!(
            "href=\"wbfile:{}",
            text(&dir.join("src/.workbench-astro-"))
        )));
        assert!(page.source.contains(&format!(
            "src=\"wbfile:{}\"",
            text(&dir.join("public/logo.svg"))
        )));
        assert!(page.source.contains("src=\"/missing.svg\""));
        assert!(page.source.contains(&format!(
            "import \"wbfile:{}",
            text(&dir.join(".workbench-rspack/astro/.workbench-astro-"))
        )));
        let exported = modules.last().unwrap();
        assert!(
            exported
                .source
                .contains("export const inputs = {\"label\":\"Hi\"};")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
