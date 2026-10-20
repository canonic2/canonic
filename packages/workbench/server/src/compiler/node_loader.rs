//! A project's JavaScript loader, run in the Node evaluator one module at a
//! time; its result and the dependencies it reported return to the build.
use base64::Engine as _;
use rspack_cacheable::{cacheable, cacheable_dyn};
use rspack_core::RunnerContext;
use rspack_error::{Diagnostic, Result, error};
use rspack_loader_runner::{Content, Identifier, Loader, LoaderContext};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[cacheable]
pub(crate) struct NodeLoader {
    identifier: Identifier,
    /// `{"loader": "/absolute/loader.cjs", "options": ...}`.
    options: String,
}

impl NodeLoader {
    pub(crate) fn new(identifier: Identifier, options: &str) -> Self {
        Self {
            identifier,
            options: options.to_owned(),
        }
    }
}

fn paths(value: &Value) -> Vec<PathBuf> {
    value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect()
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for NodeLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let options: Value = serde_json::from_str(&self.options)
            .map_err(|problem| error!("loader options: {problem}"))?;
        let Some(input) = context.take_content() else {
            return Ok(());
        };
        let (source, encoding) = match input {
            Content::String(text) => (text, "utf8"),
            Content::Buffer(bytes) => (
                base64::engine::general_purpose::STANDARD.encode(bytes),
                "base64",
            ),
        };
        let root = PathBuf::from(context.context.options.context.as_str());
        let resource = context
            .resource_path()
            .map(|path| path.as_str().to_owned())
            .unwrap_or_default();
        let request = json!({
            "loader": options["loader"], "options": options["options"],
            "resourcePath": resource, "resourceQuery": context.resource_query().unwrap_or_default(),
            "source": source, "encoding": encoding, "hot": context.hot,
        });
        let answer = crate::node::for_root(Path::new(&root))
            .call("loader", request)
            .await
            .map_err(|problem| {
                error!(
                    "{} failed on {resource}: {problem:#}",
                    options["loader"].as_str().unwrap_or("A project loader")
                )
            })?;
        for file in paths(&answer["dependencies"]) {
            context.add_file_dependency(file);
        }
        for folder in paths(&answer["contextDependencies"]) {
            context.add_context_dependency(folder);
        }
        for file in paths(&answer["missingDependencies"]) {
            context.add_missing_dependency(file);
        }
        if answer["cacheable"] == Value::Bool(false) {
            context.cacheable = false;
        }
        // `this.emitWarning` warns. `this.emitError` fails the module's build,
        // as a thrown error does: an error diagnostic on a module that still
        // builds panics Rspack's module concatenation in production builds.
        // The dependencies above stay recorded, so an edit rebuilds it.
        let loader = options["loader"].as_str().unwrap_or("A project loader");
        let messages = |field: &str| -> Vec<String> {
            answer[field]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .map(|message| format!("{loader} on {resource}: {message}"))
                .collect()
        };
        for message in messages("warnings") {
            context.emit_diagnostic(Diagnostic::warn("ModuleWarning".to_owned(), message));
        }
        let errors = messages("errors");
        if !errors.is_empty() {
            return Err(error!("{}", errors.join("\n")));
        }
        let output = answer["source"].as_str().unwrap_or_default();
        let result = if answer["encoding"] == "base64" {
            Content::Buffer(
                base64::engine::general_purpose::STANDARD
                    .decode(output)
                    .map_err(|problem| error!("loader output: {problem}"))?,
            )
        } else {
            Content::String(output.to_owned())
        };
        context.finish_with((result, None, None));
        Ok(())
    }
}
