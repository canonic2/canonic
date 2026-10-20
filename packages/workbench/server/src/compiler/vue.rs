//! Vue single-file components: the Node evaluator compiles each file with
//! Workbench's bundled Vue compiler, and these loaders make its script a
//! module that imports each style block as native CSS (scoped, or a CSS
//! module for `<style module>`) and reloads in place through Vue's own hot
//! reload runtime.
use rspack_cacheable::{cacheable, cacheable_dyn};
use rspack_core::RunnerContext;
use rspack_error::{Result, error};
use rspack_loader_runner::{Identifier, Loader, LoaderContext};
use serde_json::{Value, json};
use std::path::Path;

async fn compiled(context: &mut LoaderContext<RunnerContext>) -> Result<(String, Value)> {
    let Some(input) = context.take_content() else {
        return Err(error!("a Vue component has no source"));
    };
    let source = input.try_into_string()?;
    let file = context
        .resource_path()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_default();
    let root = context.context.options.context.as_str().to_owned();
    let answer = crate::node::for_root(Path::new(&root))
        .call(
            "vue",
            json!({ "file": file, "source": source, "hot": context.hot }),
        )
        .await
        .map_err(|problem| error!("{file}: {problem:#}"))?;
    // `<style src>` files: an edit to one rebuilds the component and its styles.
    for dependency in answer["dependencies"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
    {
        context.add_file_dependency(std::path::PathBuf::from(dependency));
    }
    Ok((file, answer))
}

/// The component as a module: its script, style imports and hot reload.
pub(crate) fn component_module(file: &str, compiled: &Value, hot: bool) -> String {
    let quote = super::entries::quote;
    let id = compiled["id"].as_str().unwrap_or_default();
    let mut lines = vec![compiled["script"].as_str().unwrap_or_default().to_owned()];
    let mut modules = vec![];
    for (index, style) in compiled["styles"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        if style["module"] == Value::Bool(true) {
            lines.push(format!(
                "import __workbench_style_{index} from {};",
                quote(&format!("{file}?workbench-vue-style={index}&module"))
            ));
            modules.push(format!("__workbench_style_{index}"));
        } else {
            lines.push(format!(
                "import {};",
                quote(&format!("{file}?workbench-vue-style={index}"))
            ));
        }
    }
    if let Some(first) = modules.first() {
        lines.push(format!("__component.__cssModules = {{ $style: {first} }};"));
    }
    if compiled["scoped"] == Value::Bool(true) {
        lines.push(format!(
            "__component.__scopeId = {};",
            quote(&format!("data-v-{id}"))
        ));
    }
    if hot {
        // As Vue Loader's hot reload: when only the template changed, the
        // new render function re-renders every mounted instance and keeps
        // its state; otherwise the new definition re-creates them.
        lines.push(format!(
            "if (module.hot) {{\n  __component.__hmrId = {id};\n  const api = __VUE_HMR_RUNTIME__;\n  module.hot.accept();\n  module.hot.dispose((data) => {{ data.workbenchScript = {key}; }});\n  if (!api.createRecord({id}, __component)) {{\n    if (module.hot.data && module.hot.data.workbenchScript === {key}) api.rerender({id}, __component.render);\n    else api.reload({id}, __component);\n  }}\n}}",
            id = quote(id),
            key = quote(compiled["scriptKey"].as_str().unwrap_or_default())
        ));
    }
    lines.push("export default __component;".to_owned());
    lines.join("\n") + "\n"
}

#[cacheable]
pub(crate) struct VueLoader {
    identifier: Identifier,
}

impl VueLoader {
    pub(crate) fn new(identifier: Identifier) -> Self {
        Self { identifier }
    }
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for VueLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let hot = context.hot;
        let (file, answer) = compiled(context).await?;
        context.finish_with((component_module(&file, &answer, hot), None, None));
        Ok(())
    }
}

/// One style block of a component, by its index in the resource query.
#[cacheable]
pub(crate) struct VueStyleLoader {
    identifier: Identifier,
}

impl VueStyleLoader {
    pub(crate) fn new(identifier: Identifier) -> Self {
        Self { identifier }
    }
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for VueStyleLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let query = context.resource_query().unwrap_or_default().to_owned();
        let index = query
            .trim_start_matches('?')
            .split('&')
            .find_map(|pair| pair.strip_prefix("workbench-vue-style="))
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| error!("Unknown Vue style: {query}"))?;
        let (_, answer) = compiled(context).await?;
        let css = answer["styles"][index]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        context.finish_with((css, None, None));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_component_imports_its_styles_and_reloads_in_place() {
        let compiled = json!({
            "id": "1a2b3c4d", "scoped": true, "lang": "ts", "scriptKey": "s1",
            "script": "const __component = {};",
            "styles": [{ "code": ".a{}", "module": false }, { "code": ".b{}", "module": true }],
        });
        let module = component_module("/acme/Card.vue", &compiled, true);
        assert!(module.starts_with(
            "const __component = {};\nimport \"/acme/Card.vue?workbench-vue-style=0\";"
        ));
        assert!(module.contains(
            "import __workbench_style_1 from \"/acme/Card.vue?workbench-vue-style=1&module\";"
        ));
        assert!(module.contains("__component.__cssModules = { $style: __workbench_style_1 };"));
        assert!(module.contains("__component.__scopeId = \"data-v-1a2b3c4d\";"));
        assert!(module.contains("api.createRecord(\"1a2b3c4d\", __component)"));
        // The script key the update leaves behind decides: same, re-render.
        assert!(module.contains("data.workbenchScript = \"s1\";"));
        assert!(module.contains(
            "if (module.hot.data && module.hot.data.workbenchScript === \"s1\") api.rerender(\"1a2b3c4d\", __component.render);\n    else api.reload(\"1a2b3c4d\", __component);"
        ));
        assert!(module.ends_with("export default __component;\n"));
        assert!(!component_module("/acme/Card.vue", &compiled, false).contains("module.hot"));
    }
}
