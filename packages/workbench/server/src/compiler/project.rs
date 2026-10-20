//! The project's `workbench.config.ts`, as the Node evaluator serializes it:
//! declarative settings, module rules and native plugin capabilities. Regular
//! expressions arrive as `{"$regexp": {"source", "flags"}}`; anything the
//! evaluator could not serialize, such as a function, is a problem.
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ProjectConfig {
    /// Exact aliases, in the order the config gives them.
    #[serde(deserialize_with = "ordered")]
    pub(crate) aliases: Vec<(String, String)>,
    pub(crate) dedupe: Vec<String>,
    pub(crate) resolve_extensions: Option<Vec<String>>,
    /// Defines: each value is code, as in Rspack's `DefinePlugin`.
    pub(crate) define: Map<String, Value>,
    pub(crate) environment: Option<Value>,
    pub(crate) plugins: Vec<Value>,
    pub(crate) rules: Vec<Value>,
    pub(crate) adapters: Map<String, Value>,
    /// What the evaluator could not carry over, such as functions.
    pub(crate) problems: Vec<String>,
}

fn ordered<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<(String, String)>, D::Error> {
    let map = Map::<String, Value>::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key, value.to_owned())))
        .collect())
}

/// The built-in adapters' browser mounts, in Workbench's `ui/preview/`.
const BUILTINS: [(&str, &str); 5] = [
    ("html", "html.js"),
    ("react", "react.js"),
    ("vue", "vue.js"),
    ("react-native-web", "react.js"),
    ("astro", "astro.js"),
];

/// Which module mounts an adapter: the project's registered runtime, or a
/// built-in one. React Native Web mounts natively with its host styles.
pub(crate) fn adapter_import(
    config: &ProjectConfig,
    root: &Path,
    preview_dir: &Path,
    adapter: &str,
) -> Result<super::entries::AdapterImport, String> {
    if let Some(runtime) = config
        .adapters
        .get(adapter)
        .and_then(|addon| addon.get("runtime"))
        .and_then(Value::as_str)
    {
        return Ok(super::entries::AdapterImport {
            file: crate::paths::resolve(root, runtime)
                .to_string_lossy()
                .into_owned(),
            native: false,
            native_styles: None,
        });
    }
    let Some((_, file)) = BUILTINS.iter().find(|(name, _)| *name == adapter) else {
        return Err(format!(
            "Unknown adapter “{adapter}”. Register it in workbench.config.ts."
        ));
    };
    let file = preview_dir.join(file).to_string_lossy().into_owned();
    Ok(if adapter == "react-native-web" {
        super::entries::AdapterImport {
            file,
            native: true,
            native_styles: Some(
                preview_dir
                    .join("react-native-web.css")
                    .to_string_lossy()
                    .into_owned(),
            ),
        }
    } else {
        super::entries::AdapterImport {
            file,
            native: false,
            native_styles: None,
        }
    })
}

/// One environment for every preview, or one per adapter name.
pub(crate) fn project_environment(
    config: &ProjectConfig,
    root: &Path,
    adapter: &str,
) -> Result<Option<PathBuf>, String> {
    let value = match &config.environment {
        Some(Value::String(file)) => Some(file.as_str()),
        Some(Value::Object(map)) => map.get(adapter).and_then(Value::as_str),
        _ => None,
    };
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let target = crate::paths::resolve(root, value);
    if !target.exists() {
        return Err(format!(
            "The preview config environment must be a file inside the project: {value}"
        ));
    }
    Ok(Some(target))
}

/// The built-in Astro adapter renders in the Node evaluator; a registered
/// `astro` adapter replaces it.
pub(crate) fn built_in_astro(config: &ProjectConfig, adapter: &str) -> bool {
    adapter == "astro" && !config.adapters.contains_key("astro")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_config_keeps_alias_order_and_resolves_adapters() {
        let config: ProjectConfig = serde_json::from_value(json!({
            "aliases": { "b": "./b.ts", "a": "a-package" },
            "adapters": { "acme-canvas": { "runtime": "./canvas.ts" } },
            "environment": { "react": "./missing.tsx" },
        }))
        .unwrap();
        assert_eq!(config.aliases[0].0, "b");
        let root = Path::new("/acme");
        let preview = Path::new("/workbench/preview");
        let canvas = adapter_import(&config, root, preview, "acme-canvas").unwrap();
        assert_eq!(canvas.file, "/acme/canvas.ts");
        let native = adapter_import(&config, root, preview, "react-native-web").unwrap();
        assert!(native.native);
        assert_eq!(
            native.native_styles.as_deref(),
            Some("/workbench/preview/react-native-web.css")
        );
        assert!(
            adapter_import(&config, root, preview, "acme-unknown")
                .unwrap_err()
                .contains("Unknown adapter")
        );
        assert!(
            project_environment(&config, root, "react")
                .unwrap_err()
                .contains("must be a file inside the project")
        );
        assert_eq!(project_environment(&config, root, "html"), Ok(None));
        assert!(built_in_astro(&config, "astro"));
    }
}
