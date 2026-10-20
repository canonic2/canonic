//! Workbench's own Rspack loaders, registered under `builtin:workbench-*`:
//! the HTML page and HTML part loaders, the import glob transform, Vue
//! single-file components and the bridge that runs a project's JavaScript
//! loaders in the Node evaluator.
//!
//! Loaders are restored from Rspack's persistent cache by type, so they hold
//! only their identifier and options; the Node evaluator they reach is looked
//! up by project root through [`crate::node::for_root`].
#![expect(
    clippy::unused_async,
    reason = "Rspack's hook macros need async functions, whether or not they await"
)]
use super::html::{html_part, transform_html};
use rspack_cacheable::{cacheable, cacheable_dyn};
use rspack_core::{
    BoxLoader, Context, ModuleRuleUseLoader, NormalModuleFactoryResolveLoader, Plugin, Resolver,
    RunnerContext,
};
use rspack_error::{Result, error};
use rspack_hook::{plugin, plugin_hook};
use rspack_loader_runner::{Identifier, Loader, LoaderContext};
use std::path::Path;
use std::sync::Arc;

pub(crate) const HTML: &str = "builtin:workbench-html-loader";
pub(crate) const HTML_PART: &str = "builtin:workbench-html-part-loader";
pub(crate) const GLOB: &str = "builtin:workbench-glob-loader";
pub(crate) const SPRITE: &str = "builtin:workbench-svg-sprite-loader";
pub(crate) const NODE: &str = "builtin:workbench-node-loader";
pub(crate) const VUE: &str = "builtin:workbench-vue-loader";
pub(crate) const VUE_STYLE: &str = "builtin:workbench-vue-style-loader";

fn source_of(context: &mut LoaderContext<RunnerContext>) -> Result<Option<String>> {
    match context.take_content() {
        Some(input) => Ok(Some(input.try_into_string()?)),
        None => Ok(None),
    }
}

fn root_of(context: &LoaderContext<RunnerContext>) -> String {
    context.context.options.context.as_str().to_owned()
}

/// An HTML page or rendered Astro HTML as a module exporting its HTML.
#[cacheable]
pub(crate) struct HtmlLoader {
    identifier: Identifier,
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for HtmlLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let Some(source) = source_of(context)? else {
            return Ok(());
        };
        let resource = context
            .resource_path()
            .map(|path| path.as_str().to_owned())
            .unwrap_or_default();
        let root = root_of(context);
        let module = transform_html(&source, &resource, Path::new(&root));
        context.finish_with((module, None, None));
        Ok(())
    }
}

/// One inline style or script of an HTML page, or a script file made runnable.
#[cacheable]
pub(crate) struct HtmlPartLoader {
    identifier: Identifier,
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for HtmlPartLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let Some(source) = source_of(context)? else {
            return Ok(());
        };
        let query = context.resource_query().unwrap_or_default().to_owned();
        let part = html_part(&source, &query).map_err(|message| error!("{message}"))?;
        context.finish_with((part, None, None));
        Ok(())
    }
}

/// Resolves `builtin:workbench-*` loader requests to Workbench's loaders.
#[plugin]
#[derive(Debug, Default)]
pub(crate) struct LoadersPlugin;

impl LoadersPlugin {
    pub(crate) fn create() -> Self {
        Self::new_inner()
    }
}

#[plugin_hook(NormalModuleFactoryResolveLoader for LoadersPlugin)]
async fn resolve_loader(
    &self,
    _context: &Context,
    _resolver: &Resolver,
    loader: &ModuleRuleUseLoader,
) -> Result<Option<BoxLoader>> {
    let request = loader.loader.as_str();
    let identifier: Identifier = request.into();
    let options = loader.options.as_deref().unwrap_or("null");
    let found: Option<BoxLoader> = if request.starts_with(HTML_PART) {
        Some(Arc::new(HtmlPartLoader { identifier }))
    } else if request.starts_with(HTML) {
        Some(Arc::new(HtmlLoader { identifier }))
    } else if request.starts_with(GLOB) {
        Some(Arc::new(super::plugins::glob::GlobLoader::new(identifier)))
    } else if request.starts_with(SPRITE) {
        Some(Arc::new(super::plugins::svg::SpriteLoader::new(
            identifier, options,
        )))
    } else if request.starts_with(VUE_STYLE) {
        Some(Arc::new(super::vue::VueStyleLoader::new(identifier)))
    } else if request.starts_with(VUE) {
        Some(Arc::new(super::vue::VueLoader::new(identifier)))
    } else if request.starts_with(NODE) {
        Some(Arc::new(super::node_loader::NodeLoader::new(
            identifier, options,
        )))
    } else {
        None
    };
    Ok(found)
}

impl Plugin for LoadersPlugin {
    fn name(&self) -> &'static str {
        "WorkbenchLoadersPlugin"
    }

    fn apply(&self, context: &mut rspack_core::ApplyContext<'_>) -> Result<()> {
        context
            .normal_module_factory_hooks
            .resolve_loader
            .tap(resolve_loader::new(self));
        Ok(())
    }
}
