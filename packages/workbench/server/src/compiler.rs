//! The preview compiler: one Rspack compiler per space and profile, built
//! with Rspack's Rust crates, that compiles every preview and docs lens as
//! entries of one compilation with shared chunks and hot updates. Output
//! lives in memory, the persistent cache in the system temporary folder, and
//! project JavaScript (computed definitions, `workbench.config.ts`, project
//! loaders, Vue's compiler and Astro rendering) runs only in the Node
//! evaluator. See `specs/preview-pipeline/`.
mod astro;
mod client;
mod docs;
mod entries;
mod exports;
mod host;
mod html;
mod loaders;
mod node_loader;
mod options;
mod overlay;
mod plugins;
mod portable;
mod previews;
mod project;
mod vue;

pub(crate) use client::BASE;
pub(crate) use previews::{Catalog, Previews, Settings, lens_requests};

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

/// A set of resolution rules shared by adapters, with its own compiler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Profile {
    /// React, Vue, HTML, Astro's client side and custom adapters.
    Web,
    /// `react-native` aliased to `react-native-web`, `.web.*` preferred.
    ReactNativeWeb,
}

impl Profile {
    pub(crate) fn of(adapter: &str) -> Self {
        if adapter == "react-native-web" {
            Self::ReactNativeWeb
        } else {
            Self::Web
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::ReactNativeWeb => "react-native-web",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "web" => Some(Self::Web),
            "react-native-web" => Some(Self::ReactNativeWeb),
            _ => None,
        }
    }

    /// The runtime's `uniqueName`, which also tags its stylesheets.
    pub(crate) fn unique_name(self) -> String {
        format!("wb_{}", self.name().replace('-', "_"))
    }
}

/// React Refresh's browser files, from Workbench's own packages.
#[derive(Clone, Debug)]
pub(crate) struct Refresh {
    pub(crate) entry: PathBuf,
    pub(crate) runtime: PathBuf,
    pub(crate) utils: PathBuf,
    pub(crate) runtime_dir: PathBuf,
}

/// Workbench's own files the compiler builds with: the preview adapters, the
/// authoring API and React Refresh's client.
#[derive(Clone, Debug)]
pub(crate) struct Package {
    /// `ui/preview/`: browser adapters and request mocks.
    pub(crate) preview_dir: PathBuf,
    /// `sdk/api.js`, what `@canonic2/workbench` resolves to.
    pub(crate) api: PathBuf,
    pub(crate) refresh: Refresh,
}

impl Package {
    /// Workbench's files under its product folder, which holds `ui/`, `sdk/`
    /// and the hoisted `node_modules/`.
    pub(crate) fn locate(product: &Path) -> Result<Self> {
        let modules = product.join("node_modules");
        let client = modules.join("@rspack/plugin-react-refresh/client");
        let package = Self {
            preview_dir: product.join("ui/preview"),
            api: product.join("sdk/api.js"),
            refresh: Refresh {
                entry: client.join("reactRefreshEntry.js"),
                runtime: client.join("reactRefresh.js"),
                utils: client.join("refreshUtils.js"),
                runtime_dir: modules.join("react-refresh"),
            },
        };
        for file in [
            &package.preview_dir,
            &package.api,
            &package.refresh.entry,
            &package.refresh.runtime_dir,
        ] {
            std::fs::metadata(file)
                .with_context(|| format!("Workbench file missing: {}", file.display()))?;
        }
        Ok(package)
    }
}
