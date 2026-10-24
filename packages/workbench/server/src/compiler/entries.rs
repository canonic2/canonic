//! The generated entry modules: one per preview and one per docs lens. Each
//! imports its definition, adapter, sources, environments and styles, and
//! registers what the preview module mounts. Pure string building.
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The first 12 hexadecimal digits of the SHA-256 of `value`.
pub(crate) fn digest(value: &str) -> String {
    use std::fmt::Write as _;
    Sha256::digest(value.as_bytes())
        .iter()
        .take(6)
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Entry names are file-safe and stay unique when IDs differ only by `/` and `--`.
pub(crate) fn entry_slug(id: &str) -> String {
    format!("{}-{}", id.replace('/', "--"), digest(id))
}

/// A docs lens's entry name, from its request as JSON text.
pub(crate) fn docs_slug(request: &Value) -> String {
    format!("docs-{}", digest(&js_json(request)))
}

/// The adapter's mount: the built-in React Native Web runtime mounts natively.
#[derive(Clone, Debug)]
pub(crate) struct AdapterImport {
    pub(crate) file: String,
    pub(crate) native: bool,
    pub(crate) native_styles: Option<String>,
}

/// One source of a preview entry's `sources()` map. `scenario` is its key: a
/// scenario ID, or an iteration's key from [`iteration_sources`].
#[derive(Clone, Debug)]
pub(crate) enum SourceImport {
    Module {
        scenario: String,
        file: String,
        export_name: String,
    },
    Astro {
        scenario: String,
        module: String,
        render_url: Option<String>,
    },
}

/// A portable entry bundles Workbench's runtime and request mocks from their
/// files and exports what the viewer or docs page imports. `note` is appended
/// to a preview's docs, with its controls removed.
#[derive(Clone, Debug)]
pub(crate) struct PortableEntry {
    pub(crate) runtime: String,
    pub(crate) requests: String,
    pub(crate) note: Option<String>,
}

pub(crate) struct PreviewEntry<'a> {
    pub(crate) slug: &'a str,
    pub(crate) definition: &'a str,
    pub(crate) adapter: &'a AdapterImport,
    pub(crate) sources: &'a [SourceImport],
    pub(crate) project_environment: Option<&'a str>,
    pub(crate) own_environment: Option<&'a str>,
    pub(crate) styles: &'a [String],
    pub(crate) portable: Option<&'a PortableEntry>,
}

const LIVE_RUNTIME: &str = "/_workbench/preview-runtime.js";

/// Each iteration of a validated scenario that declares its own `source`,
/// keyed `<scenario>+<iteration>` for the scenario's implicit artboard and
/// `<scenario>+<artboard>/<iteration>` for a declared artboard. The runtime
/// looks an iteration's key up before its scenario's.
pub(crate) fn iteration_sources<'a>(scenario: &str, spec: &'a Value) -> Vec<(String, &'a Value)> {
    let mut found = vec![];
    let mut add = |prefix: String, iterations: &'a Value| {
        for (id, iteration) in iterations.as_object().into_iter().flatten() {
            if iteration["source"].is_object() {
                found.push((format!("{prefix}{id}"), &iteration["source"]));
            }
        }
    };
    add(format!("{scenario}+"), &spec["iterations"]);
    for (artboard, board) in spec["artboards"].as_object().into_iter().flatten() {
        add(format!("{scenario}+{artboard}/"), &board["iterations"]);
    }
    found
}

/// JSON text as JavaScript's `JSON.stringify` writes it.
pub(crate) fn js_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// A string literal as `JSON.stringify` writes it.
pub(crate) fn quote(text: &str) -> String {
    js_json(&Value::String(text.to_owned()))
}

fn adapter_lines(adapter: &AdapterImport) -> Vec<String> {
    if adapter.native {
        let mut lines = vec![];
        if let Some(styles) = &adapter.native_styles {
            lines.push(format!("import {};", quote(styles)));
        }
        lines.push(format!(
            "import {{ mountNative }} from {};",
            quote(&adapter.file)
        ));
        lines.push("const adapter = { mount: mountNative };".to_owned());
        return lines;
    }
    vec![format!(
        "import * as adapter from {};",
        quote(&adapter.file)
    )]
}

fn environment_lines(project: Option<&str>, own: Option<&str>) -> (Vec<String>, String) {
    let mut lines = vec![];
    let mut names = vec![];
    if let Some(project) = project {
        lines.push(format!(
            "import * as projectEnvironment from {};",
            quote(project)
        ));
        names.push("projectEnvironment");
    }
    if let Some(own) = own {
        lines.push(format!("import * as ownEnvironment from {};", quote(own)));
        names.push("ownEnvironment");
    }
    let expression = match names.as_slice() {
        [one, two] => format!("combine({one}, {two})"),
        [one] => (*one).to_owned(),
        _ => "{}".to_owned(),
    };
    (lines, expression)
}

/// A preview's entry. Sources are read when `mount` runs, so a hot update to a
/// source re-renders with the new module. Sources that refresh themselves
/// (React components) stop an update there; anything else bubbles to the
/// entry, which re-renders the selected scenario in place.
pub(crate) fn preview_entry_source(input: &PreviewEntry<'_>) -> String {
    let portable = input.portable;
    let mut lines = vec![];
    // Request mocks first: they replace fetch before any project module runs.
    if let Some(portable) = portable {
        lines.push(format!("import {};", quote(&portable.requests)));
    }
    lines.push(format!(
        "import definition from {};",
        quote(input.definition)
    ));
    lines.extend(adapter_lines(input.adapter));
    lines.push(format!(
        "import {{ boot, combine }} from {};",
        quote(portable.map_or(LIVE_RUNTIME, |portable| portable.runtime.as_str()))
    ));
    let mut accepted = vec![];
    let mut sources = vec![];
    for (index, source) in input.sources.iter().enumerate() {
        let name = format!("source{index}");
        match source {
            SourceImport::Astro {
                scenario,
                module,
                render_url,
            } => {
                lines.push(format!("import * as {name} from {};", quote(module)));
                accepted.push(Value::String(module.clone()));
                sources.push(format!(
                    "{}: {{ html: {name}.html, inputs: {name}.inputs, renderUrl: {} }}",
                    quote(scenario),
                    render_url.as_deref().map_or("null".to_owned(), quote)
                ));
            }
            SourceImport::Module {
                scenario,
                file,
                export_name,
            } => {
                lines.push(format!("import * as {name} from {};", quote(file)));
                accepted.push(Value::String(file.clone()));
                sources.push(format!(
                    "{}: {name}[{}]",
                    quote(scenario),
                    quote(export_name)
                ));
            }
        }
    }
    let (environment, expression) =
        environment_lines(input.project_environment, input.own_environment);
    lines.extend(environment);
    for style in input.styles {
        lines.push(format!("import {};", quote(style)));
    }
    let shown = match portable.and_then(|portable| portable.note.as_deref()) {
        Some(note) if !note.is_empty() => format!(
            "{{ ...definition, controls: {{}}, docs: (definition.docs || \"\") + \"\\n\" + {} }}",
            quote(note)
        ),
        _ => "definition".to_owned(),
    };
    lines.push(format!(
        "const sources = () => ({{{}}});",
        sources.join(", ")
    ));
    lines.push(format!("const environment = {expression};"));
    if portable.is_some() {
        lines.push(format!("export function mount(options) {{ return boot({shown}, sources(), adapter, environment, options); }}"));
        lines.push("if (!window.__workbenchHost) mount(window.__workbenchOptions);".to_owned());
        return lines.join("\n") + "\n";
    }
    lines.push("function mount(options) { return boot(definition, sources(), adapter, environment, options); }".to_owned());
    lines.push(format!(
        "(window.__wbRspack = window.__wbRspack || {{}})[{}] = {{ mount, moduleId: module.id }};",
        quote(input.slug)
    ));
    lines.push(format!(
        "if (module.hot) module.hot.accept({}, () => {{ if (window.__workbenchRerender) window.__workbenchRerender(sources()); }});",
        js_json(&Value::Array(accepted))
    ));
    lines.join("\n") + "\n"
}

#[derive(Clone, Debug)]
pub(crate) enum DocsExamples {
    Folder(Vec<(String, String)>),
    Exports {
        file: String,
        examples: Vec<(String, String)>,
    },
    Astro(Vec<(String, String)>),
}

pub(crate) struct DocsEntry<'a> {
    pub(crate) slug: &'a str,
    pub(crate) adapter: &'a AdapterImport,
    pub(crate) project_environment: Option<&'a str>,
    pub(crate) lens_environment: Option<&'a str>,
    pub(crate) styles: &'a [String],
    pub(crate) examples: &'a DocsExamples,
    pub(crate) portable: Option<&'a PortableEntry>,
}

/// A docs lens's entry: its examples by ID, with the lens's adapter and environment.
pub(crate) fn docs_entry_source(input: &DocsEntry<'_>) -> String {
    let mut lines = adapter_lines(input.adapter);
    lines.push(format!(
        "import {{ combine }} from {};",
        quote(
            input
                .portable
                .map_or(LIVE_RUNTIME, |portable| portable.runtime.as_str())
        )
    ));
    let (environment, expression) =
        environment_lines(input.project_environment, input.lens_environment);
    lines.extend(environment);
    for style in input.styles {
        lines.push(format!("import {};", quote(style)));
    }
    let mut entries = vec![];
    match input.examples {
        DocsExamples::Folder(examples) => {
            for (index, (id, file)) in examples.iter().enumerate() {
                lines.push(format!("import * as example{index} from {};", quote(file)));
                entries.push(format!("{}: example{index}.default", quote(id)));
            }
        }
        DocsExamples::Astro(examples) => {
            // Astro examples render in the Node evaluator; each is a module of rendered HTML.
            for (index, (id, module)) in examples.iter().enumerate() {
                lines.push(format!(
                    "import * as example{index} from {};",
                    quote(module)
                ));
                entries.push(format!(
                    "{}: {{ html: example{index}.html, inputs: {{}}, renderUrl: null }}",
                    quote(id)
                ));
            }
        }
        DocsExamples::Exports { file, examples } => {
            lines.push(format!("import * as source from {};", quote(file)));
            for (id, export_name) in examples {
                entries.push(format!("{}: source[{}]", quote(id), quote(export_name)));
            }
        }
    }
    if input.portable.is_some() {
        lines.push("export { adapter };".to_owned());
        lines.push(format!("export const environment = {expression};"));
        lines.push(format!(
            "export const examples = {{{}}};",
            entries.join(", ")
        ));
        return lines.join("\n") + "\n";
    }
    lines.push(format!("const environment = {expression};"));
    lines.push(format!(
        "(window.__wbRspack = window.__wbRspack || {{}})[{}] = {{ adapter, environment, examples: {{{}}}, moduleId: module.id }};",
        quote(input.slug),
        entries.join(", ")
    ));
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn react() -> AdapterImport {
        AdapterImport {
            file: "/workbench/react.js".into(),
            native: false,
            native_styles: None,
        }
    }

    #[test]
    fn digests_and_slugs_match_the_javascript_pipeline() {
        // sha256("acme") starts with 4e7b…; the slug keeps IDs that differ
        // only by `/` and `--` apart.
        assert_eq!(digest("").len(), 12);
        assert_eq!(digest(""), "e3b0c44298fc");
        assert_ne!(entry_slug("a/b"), entry_slug("a--b"));
        assert!(entry_slug("pages/landing").starts_with("pages--landing-"));
        assert_eq!(
            docs_slug(&serde_json::json!({"adapter": "react"})),
            format!("docs-{}", digest(r#"{"adapter":"react"}"#))
        );
    }

    #[test]
    fn iterations_with_their_own_source_get_their_own_keys() {
        let spec = serde_json::json!({
            "iterations": { "illustrated": { "source": { "entry": "./empty.tsx" } }, "plain": {} },
            "artboards": {
                "info": { "width": 480, "height": 320 },
                "warning": { "width": 480, "height": 320, "iterations": {
                    "bold": { "source": { "entry": "./bold.tsx", "export": "Bold" } },
                    "calm": { "inputs": { "tone": "calm" } }
                } }
            }
        });
        let found = iteration_sources("default", &spec);
        let keys: Vec<&str> = found.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["default+illustrated", "default+warning/bold"]);
        assert_eq!(found[1].1["export"], "Bold");
        let adapter = react();
        let sources: Vec<SourceImport> = found
            .iter()
            .map(|(key, source)| SourceImport::Module {
                scenario: key.clone(),
                file: format!("/acme/{}", source["entry"].as_str().unwrap()),
                export_name: source["export"].as_str().unwrap_or("default").to_owned(),
            })
            .collect();
        let entry = preview_entry_source(&PreviewEntry {
            slug: "feedback-1",
            definition: "/acme/feedback.workbench.ts",
            adapter: &adapter,
            sources: &sources,
            project_environment: None,
            own_environment: None,
            styles: &[],
            portable: None,
        });
        assert!(entry.contains("\"default+illustrated\": source0[\"default\"]"));
        assert!(entry.contains("\"default+warning/bold\": source1[\"Bold\"]"));
    }

    #[test]
    fn preview_entries_register_live_and_export_portable() {
        let adapter = react();
        let sources = [SourceImport::Module {
            scenario: "default".into(),
            file: "/acme/Landing.tsx".into(),
            export_name: "default".into(),
        }];
        let styles = ["/acme/theme.css".to_owned()];
        let mut input = PreviewEntry {
            slug: "pages--landing-1",
            definition: "/acme/landing.workbench.ts",
            adapter: &adapter,
            sources: &sources,
            project_environment: Some("/acme/environment.tsx"),
            own_environment: Some("/acme/landing-environment.tsx"),
            styles: &styles,
            portable: None,
        };
        let live = preview_entry_source(&input);
        assert!(live.contains("from \"/_workbench/preview-runtime.js\""));
        assert!(live.contains("combine(projectEnvironment, ownEnvironment)"));
        assert!(live.contains("window.__wbRspack"));
        assert!(live.contains("module.hot.accept([\"/acme/Landing.tsx\"]"));
        let portable = PortableEntry {
            runtime: "/workbench/browser.js".into(),
            requests: "/workbench/requests.js".into(),
            note: Some("Static.".into()),
        };
        input.portable = Some(&portable);
        let built = preview_entry_source(&input);
        assert!(
            built.starts_with("import \"/workbench/requests.js\";"),
            "request mocks load before any project module"
        );
        assert!(built.contains("export function mount"));
        assert!(
            built.contains("controls: {}, docs: (definition.docs || \"\") + \"\\n\" + \"Static.\"")
        );
        assert!(!built.contains("__wbRspack") && !built.contains("module.hot"));
        assert!(!built.contains("preview-runtime"));
    }

    #[test]
    fn docs_entries_list_examples_by_id_for_each_source_kind() {
        let native = AdapterImport {
            file: "/workbench/react.js".into(),
            native: true,
            native_styles: Some("/workbench/react-native-web.css".into()),
        };
        let folder = DocsExamples::Folder(vec![("primary".into(), "/acme/primary.tsx".into())]);
        let base = |examples| DocsEntry {
            slug: "docs-1",
            adapter: &native,
            project_environment: None,
            lens_environment: None,
            styles: &[],
            examples,
            portable: None,
        };
        let source = docs_entry_source(&base(&folder));
        assert!(source.contains("import \"/workbench/react-native-web.css\";"));
        assert!(source.contains("const adapter = { mount: mountNative };"));
        assert!(source.contains("\"primary\": example0.default"));
        let portable = PortableEntry {
            runtime: "/workbench/browser.js".into(),
            requests: "/workbench/requests.js".into(),
            note: None,
        };
        let exports = DocsExamples::Exports {
            file: "/acme/card.examples.ts".into(),
            examples: vec![("with-icon".into(), "withIcon".into())],
        };
        let mut input = base(&exports);
        input.portable = Some(&portable);
        assert!(
            docs_entry_source(&input)
                .contains("export const examples = {\"with-icon\": source[\"withIcon\"]};")
        );
        let astro = DocsExamples::Astro(vec![("card".into(), "/acme/.astro/card.js".into())]);
        assert!(
            docs_entry_source(&base(&astro))
                .contains("\"card\": { html: example0.html, inputs: {}, renderUrl: null }")
        );
    }
}
