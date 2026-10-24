//! Rspack options for one profile's compiler: Workbench's format handling,
//! then the project's rules and native plugin capabilities. Output goes to
//! `/out/<profile>/<generation>/` in memory; the persistent cache lives in the
//! system temporary folder. Nothing is written inside the project.
use super::Profile;
use super::host::{EntriesPlugin, EntrySet, RefreshRuntimePlugin};
use super::loaders::{HTML, HTML_PART, LoadersPlugin, VUE, VUE_STYLE};
use super::overlay::Overlay;
use super::project::ProjectConfig;
use super::{BASE, Package};
use anyhow::{Context as _, Result};
use rspack::builder::{Builder as _, OptimizationOptionsBuilder, OutputOptionsBuilder};
use rspack_core::{
    Alias, CacheOptions, Compiler, CssAutoOrModuleParserOptions, Experiments, ExternalItem,
    ExternalItemValue, Filename, Mode, ModuleOptions, ModuleRule, ModuleRuleEffect, ModuleRuleUse,
    ModuleRuleUseLoader, ParserOptions, PersistentCacheOptions, PublicPath, Resolve,
    RuleSetCondition, RuleSetConditionWithEmpty, StorageOptions, TsconfigOptions,
    TsconfigReferences,
};
use rspack_fs::{MemoryFileSystem, NativeFileSystem};
use rspack_paths::Utf8PathBuf;
use rspack_regex::RspackRegex;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};

const EXTENSIONS: [&str; 9] = [
    ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".json", ".vue", ".html",
];
const NATIVE_EXTENSIONS: [&str; 4] = [".web.tsx", ".web.ts", ".web.jsx", ".web.js"];
const ASSETS: &str =
    r"\.(svg|png|jpe?g|gif|webp|avif|ico|bmp|woff2?|ttf|otf|eot|mp4|webm|mov|mp3|ogg|wav|m4a)$";

/// The stand-in entry every compiler starts with.
pub(crate) const IDLE: &str = "__workbench_idle";

/// What a compiler is for: live previews with hot updates, or one portable build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    Live,
    Portable,
}

pub(crate) struct Build<'a> {
    pub(crate) profile: Profile,
    pub(crate) generation: u64,
    pub(crate) root: &'a Path,
    pub(crate) package: &'a Package,
    pub(crate) project: &'a ProjectConfig,
    pub(crate) cache_dir: &'a Path,
    pub(crate) purpose: Purpose,
    /// The persistent cache's version: changes with the binary and the options.
    pub(crate) identity: String,
}

pub(crate) fn regex(source: &str) -> RspackRegex {
    RspackRegex::new(source).expect("valid compiler pattern")
}

fn regex_condition(source: &str) -> RuleSetCondition {
    RuleSetCondition::Regexp(regex(source))
}

fn query(source: &str) -> RuleSetConditionWithEmpty {
    RuleSetConditionWithEmpty::new(regex_condition(source))
}

pub(crate) fn loader(name: &str, options: Option<&Value>) -> ModuleRuleUseLoader {
    ModuleRuleUseLoader {
        loader: name.to_owned(),
        options: options.map(Value::to_string),
        cache: true,
        options_cache_key: options.map(Value::to_string).unwrap_or_default(),
    }
}

fn uses(loaders: Vec<ModuleRuleUseLoader>) -> ModuleRuleUse {
    ModuleRuleUse::Array(loaders)
}

fn swc(development: bool, is_module: bool) -> Value {
    let mut options = json!({
        "jsc": {
            "parser": { "syntax": "typescript", "tsx": true },
            "transform": { "react": { "runtime": "automatic", "development": development, "refresh": development } },
            "target": "es2020"
        }
    });
    if is_module {
        options["isModule"] = json!(true);
    }
    options
}

fn escape(path: &Path) -> String {
    regex::escape(&path.to_string_lossy())
}

/// Workbench's rules for every format, before the project's own.
#[expect(
    clippy::too_many_lines,
    reason = "the rule list for every format, in order"
)]
fn rules(build: &Build<'_>) -> Vec<ModuleRule> {
    let development = build.purpose == Purpose::Live;
    let package = build.package;
    let swc_loader = |is_module| loader("builtin:swc-loader", Some(&swc(development, is_module)));
    let mut rules = vec![];
    if development {
        // React Refresh's loader runs first, as Rspack's refresh plugin
        // unshifts it: project sources only, not packages or generated entries.
        let excluded = [
            "node_modules".to_owned(),
            r"\.workbench-rspack[\\/]".to_owned(),
            escape(&package.refresh.entry),
            escape(&package.refresh.runtime),
            escape(&package.refresh.utils),
            escape(&package.refresh.runtime_dir),
        ]
        .join("|");
        rules.push(ModuleRule {
            test: Some(regex_condition(r"\.([cm]js|[jt]sx?|flow)$")),
            exclude: Some(regex_condition(&excluded)),
            effect: ModuleRuleEffect {
                r#use: uses(vec![loader("builtin:react-refresh-loader", None)]),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    rules.push(ModuleRule {
        test: Some(regex_condition(r"\.[cm]?[jt]sx?$")),
        exclude: Some(regex_condition("node_modules")),
        effect: ModuleRuleEffect {
            r#type: Some("javascript/auto".into()),
            ..Default::default()
        },
        one_of: Some(vec![
            // A local script an HTML preview loads, run again on each render.
            ModuleRule {
                resource_query: Some(query("workbench-run")),
                effect: ModuleRuleEffect {
                    r#use: uses(vec![swc_loader(true), loader(HTML_PART, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
            ModuleRule {
                effect: ModuleRuleEffect {
                    r#use: uses(vec![swc_loader(false)]),
                    ..Default::default()
                },
                ..Default::default()
            },
        ]),
        ..Default::default()
    });
    // Custom properties stay global, as Vite's and Vue's CSS modules leave
    // them: other stylesheets and SVG sprites read them by their written name.
    let css_module = || {
        ParserOptions::CssAutoOrModule(CssAutoOrModuleParserOptions {
            named_exports: Some(false),
            dashed_idents: Some(false),
            ..Default::default()
        })
    };
    // Vue components: the Vue loader makes the script a module, each style
    // block is native CSS, and a `<style module>` block a CSS module.
    rules.push(ModuleRule {
        test: Some(regex_condition(r"\.vue$")),
        one_of: Some(vec![
            ModuleRule {
                resource_query: Some(query(r"workbench-vue-style=\d+&module")),
                effect: ModuleRuleEffect {
                    r#type: Some("css/module".into()),
                    parser: Some(css_module()),
                    r#use: uses(vec![loader(VUE_STYLE, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
            ModuleRule {
                resource_query: Some(query("workbench-vue-style")),
                effect: ModuleRuleEffect {
                    r#type: Some("css".into()),
                    r#use: uses(vec![loader(VUE_STYLE, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
            ModuleRule {
                effect: ModuleRuleEffect {
                    r#type: Some("javascript/auto".into()),
                    r#use: uses(vec![swc_loader(true), loader(VUE, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
        ]),
        ..Default::default()
    });
    rules.push(ModuleRule {
        test: Some(regex_condition(r"\.html?$")),
        one_of: Some(vec![
            ModuleRule {
                resource_query: Some(query("workbench-style")),
                effect: ModuleRuleEffect {
                    r#type: Some("css".into()),
                    r#use: uses(vec![loader(HTML_PART, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
            ModuleRule {
                resource_query: Some(query("workbench-script")),
                effect: ModuleRuleEffect {
                    r#type: Some("javascript/auto".into()),
                    r#use: uses(vec![swc_loader(true), loader(HTML_PART, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
            ModuleRule {
                effect: ModuleRuleEffect {
                    r#type: Some("javascript/auto".into()),
                    r#use: uses(vec![loader(HTML, None)]),
                    ..Default::default()
                },
                ..Default::default()
            },
        ]),
        ..Default::default()
    });
    rules.push(ModuleRule {
        test: Some(regex_condition(r"\.css$")),
        one_of: Some(vec![
            // Projects import CSS modules as a default object. The CSS plugin
            // reads `css/module` options only in this variant.
            ModuleRule {
                test: Some(regex_condition(r"\.module\.css$")),
                effect: ModuleRuleEffect {
                    r#type: Some("css/module".into()),
                    parser: Some(css_module()),
                    ..Default::default()
                },
                ..Default::default()
            },
            ModuleRule {
                effect: ModuleRuleEffect {
                    r#type: Some("css".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        ]),
        ..Default::default()
    });
    rules.push(ModuleRule {
        test: Some(regex_condition(ASSETS)),
        effect: ModuleRuleEffect {
            r#type: Some("asset/resource".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    // Workbench's package declares no side effects, but its request mocks and
    // React Native Web host styles are imported for nothing else.
    rules.push(ModuleRule {
        include: Some(RuleSetCondition::Array {
            items: vec![
                RuleSetCondition::String(
                    package
                        .preview_dir
                        .join("requests.js")
                        .to_string_lossy()
                        .into(),
                ),
                RuleSetCondition::String(
                    package
                        .preview_dir
                        .join("react-native-web.css")
                        .to_string_lossy()
                        .into(),
                ),
            ],
            can_sync: true,
        }),
        effect: ModuleRuleEffect {
            side_effects: Some(true),
            ..Default::default()
        },
        ..Default::default()
    });
    rules
}

/// Exact aliases (relative targets from the project root) and deduplicated packages.
pub(crate) fn aliases(
    root: &Path,
    project: &ProjectConfig,
    profile: Profile,
    api: &Path,
) -> Vec<(String, String)> {
    let mut aliases = vec![(
        "@canonic2/workbench$".to_owned(),
        api.to_string_lossy().into_owned(),
    )];
    if profile == Profile::ReactNativeWeb {
        aliases.push(("react-native$".to_owned(), "react-native-web".to_owned()));
    }
    for (key, value) in &project.aliases {
        let target = if value.starts_with('.') || Path::new(value).is_absolute() {
            crate::paths::resolve(root, value)
                .to_string_lossy()
                .into_owned()
        } else {
            value.clone()
        };
        aliases.push((format!("{key}$"), target));
    }
    for name in &project.dedupe {
        // As Node's require.resolve(name + '/package.json') from the root:
        // the nearest node_modules folder that holds the package.
        let found = root.ancestors().find_map(|folder| {
            let candidate = folder.join("node_modules").join(name);
            candidate
                .join("package.json")
                .is_file()
                .then_some(candidate)
        });
        if let Some(found) = found {
            aliases.push((name.clone(), found.to_string_lossy().into_owned()));
        }
    }
    aliases
}

fn javascript_parser() -> rspack_core::ParserOptionsMap {
    use rspack_core::{
        DynamicImportMode, ImportMeta, JavascriptParserCommonjsExportsOption,
        JavascriptParserCommonjsOptions, JavascriptParserOptions, JavascriptParserOrder,
        JavascriptParserUrl, JavascriptParserWorkerOptions,
    };
    // The defaults the Node binding applies. The Rust builder leaves
    // `import_meta` unset for a `web` target, and Rspack then panics.
    let options = JavascriptParserOptions {
        dynamic_import_mode: Some(DynamicImportMode::Lazy),
        dynamic_import_preload: Some(JavascriptParserOrder::Disable),
        dynamic_import_prefetch: Some(JavascriptParserOrder::Disable),
        url: Some(JavascriptParserUrl::Enable),
        expr_context_critical: Some(true),
        unknown_context_critical: Some(true),
        wrapped_context_critical: Some(false),
        strict_this_context_on_imports: Some(false),
        wrapped_context_reg_exp: Some(regex(".*")),
        worker: Some(JavascriptParserWorkerOptions::new(
            vec!["...".to_owned()],
            None,
        )),
        import_meta: Some(ImportMeta::Enabled),
        require_alias: Some(false),
        require_as_expression: Some(true),
        require_dynamic: Some(true),
        require_resolve: Some(true),
        commonjs: Some(JavascriptParserCommonjsOptions {
            exports: JavascriptParserCommonjsExportsOption::Enable,
        }),
        import_dynamic: Some(true),
        commonjs_magic_comments: Some(false),
        jsx: Some(false),
        ..Default::default()
    };
    [("javascript".to_owned(), ParserOptions::Javascript(options))]
        .into_iter()
        .collect()
}

fn utf8(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The compiler for one profile and generation.
#[expect(
    clippy::too_many_lines,
    reason = "every option of the compiler in one builder"
)]
pub(crate) fn compiler(
    build: &Build<'_>,
    overlay: &Overlay,
    entries: Arc<Mutex<EntrySet>>,
    out: Arc<MemoryFileSystem>,
    project_rules: Vec<ModuleRule>,
    plugins: Vec<rspack_core::BoxPlugin>,
) -> Result<Compiler> {
    use rspack_plugin_javascript::define_plugin::{DefinePlugin, DefineValue};
    let profile = build.profile;
    let live = build.purpose == Purpose::Live;
    let root = build.root;
    let unique_name = profile.unique_name();
    let extensions = build.project.resolve_extensions.clone().unwrap_or_else(|| {
        let native = if profile == Profile::ReactNativeWeb {
            NATIVE_EXTENSIONS.as_slice()
        } else {
            &[]
        };
        native
            .iter()
            .chain(EXTENSIONS.iter())
            .map(|extension| (*extension).to_owned())
            .collect()
    });
    let mut alias = aliases(root, build.project, profile, &build.package.api);
    if live {
        // As the JavaScript refresh plugin adds it, before the project's own.
        alias.insert(
            0,
            (
                "react-refresh".to_owned(),
                utf8(&build.package.refresh.runtime_dir),
            ),
        );
    }
    let alias = alias
        .into_iter()
        .map(|(key, target)| (key, vec![rspack_resolver::AliasValue::Path(target)]))
        .collect();
    let tsconfig = root.join("tsconfig.json");
    let resolve = Resolve {
        alias: Some(Alias::MergeAlias(alias)),
        extensions: Some(extensions),
        modules: Some(vec![
            utf8(&root.join("node_modules")),
            "node_modules".to_owned(),
        ]),
        // The project's `paths`, such as `@/*`, resolve as in its own build.
        tsconfig: tsconfig.is_file().then(|| TsconfigOptions {
            config_file: Utf8PathBuf::from(utf8(&tsconfig)),
            references: TsconfigReferences::Auto,
        }),
        ..Default::default()
    };
    let mut output = OutputOptionsBuilder::default();
    let mut optimization = OptimizationOptionsBuilder::default();
    if live {
        let public_path = format!("{BASE}{}/{}/", profile.name(), build.generation);
        output
            .path(format!("/out/{}/{}", profile.name(), build.generation))
            .public_path(PublicPath::Filename(Filename::from(public_path)))
            .filename(Filename::from("[name].[contenthash:8].js"))
            .chunk_filename(Filename::from("chunks/[name].[contenthash:8].js"))
            // Stylesheets keep stable names: Rspack's CSS hot update fetches
            // a chunk's stylesheet by the name its runtime already knows.
            .css_filename(Filename::from("[name].css"))
            .css_chunk_filename(Filename::from("chunks/[name].css"))
            .asset_module_filename(Filename::from("assets/[name].[hash:8][ext]"))
            .unique_name(unique_name.clone())
            .hot_update_chunk_filename(Filename::from("hot/[id].[fullhash].hot-update.js"))
            .hot_update_main_filename(Filename::from("hot/[runtime].[fullhash].hot-update.json"));
        optimization
            .minimize(false)
            .module_ids("named".to_owned())
            .runtime_chunk(rspack_plugin_runtime_chunk::RuntimeChunkOptions {
                name: rspack_plugin_runtime_chunk::RuntimeChunkName::Single,
            });
    } else {
        output
            .path("/out")
            .public_path(PublicPath::Auto)
            .filename(Filename::from("[name].js"))
            .chunk_filename(Filename::from("chunks/[contenthash:8].js"))
            .css_filename(Filename::from("[name].css"))
            .css_chunk_filename(Filename::from("chunks/[contenthash:8].css"))
            .asset_module_filename(Filename::from("assets/[name].[hash:8][ext]"))
            .module(true)
            .chunk_format("module".to_owned())
            .chunk_loading(rspack_core::ChunkLoading::Enable(
                rspack_core::ChunkLoadingType::Import,
            ))
            .library(rspack_core::LibraryOptions {
                name: None,
                export: None,
                library_type: "module".to_owned(),
                umd_named_define: None,
                auxiliary_comment: None,
                amd_container: None,
            });
        optimization.minimize(true);
    }
    let externals: rspack_core::ExternalItemObject = [(
        "/_workbench/preview-runtime.js".to_owned(),
        ExternalItemValue::String("__workbenchRuntime".to_owned()),
    )]
    .into_iter()
    .collect();
    let mut define: DefineValue = [
        (
            "process.env.NODE_ENV".to_owned(),
            json!(if live {
                "\"development\""
            } else {
                "\"production\""
            }),
        ),
        ("__VUE_OPTIONS_API__".to_owned(), json!("true")),
        ("__VUE_PROD_DEVTOOLS__".to_owned(), json!("false")),
        (
            "__VUE_PROD_HYDRATION_MISMATCH_DETAILS__".to_owned(),
            json!("false"),
        ),
    ]
    .into_iter()
    .collect();
    if live {
        define.insert(
            "__react_refresh_library__".to_owned(),
            json!(serde_json::to_string(&unique_name)?),
        );
        define.insert("__reload_on_runtime_errors__".to_owned(), json!("false"));
    }
    for (key, value) in &build.project.define {
        define.insert(key.clone(), value.clone());
    }
    let cache = if live {
        CacheOptions::Persistent(PersistentCacheOptions {
            build_dependencies: vec![],
            version: build.identity.clone(),
            storage: StorageOptions::FileSystem {
                directory: Utf8PathBuf::from(utf8(build.cache_dir)),
            },
            portable: false,
            readonly: false,
            max_age: 7 * 24 * 60 * 60 * 1000,
            max_memory_generations: rspack_core::MaxMemoryGenerations::Infinity,
        })
    } else {
        CacheOptions::Memory { max_generations: 1 }
    };
    // Rspack's builder adds webpack's default `./src` entry when a compiler
    // has none; an empty module stands in until entries join.
    let idle = utf8(&root.join(".workbench-rspack").join("idle.js"));
    overlay.write(&idle, "");
    let mut module_rules = rules(build);
    module_rules.extend(project_rules);
    let mut builder = Compiler::builder();
    builder
        .context(utf8(root).as_str())
        .mode(if live {
            Mode::Development
        } else {
            Mode::Production
        })
        .target(vec!["web".to_owned()])
        .output(output)
        .optimization(optimization)
        .externals(ExternalItem::Object(externals))
        .externals_type("window".to_owned())
        .resolve(resolve)
        .cache(cache)
        .module(
            ModuleOptions::builder()
                .rules(module_rules)
                .parser(javascript_parser()),
        )
        .experiments(Experiments::builder().css(true))
        .entry(IDLE, idle.as_str())
        .enable_loader_swc()
        .input_filesystem(Arc::new(overlay.clone()))
        .intermediate_filesystem(Arc::new(NativeFileSystem::new(false)))
        .output_filesystem(out)
        .plugin(Box::new(LoadersPlugin::create()))
        .plugin(Box::new(EntriesPlugin::create(
            rspack_core::Context::from(utf8(root).as_str()),
            entries,
        )))
        .plugin(Box::new(DefinePlugin::new(define)))
        // The `web` target's preset: remote URLs, such as a stylesheet's
        // `@import` of a font service, stay external.
        .plugin(rspack_plugin_externals::http_externals_rspack_plugin(false));
    if live {
        let refresh = &build.package.refresh;
        let provide: rustc_hash::FxHashMap<String, Vec<String>> = [
            (
                "$ReactRefreshRuntime$".to_owned(),
                vec![utf8(&refresh.runtime)],
            ),
            (
                "__react_refresh_utils__".to_owned(),
                vec![utf8(&refresh.utils)],
            ),
        ]
        .into_iter()
        .collect();
        builder
            .enable_loader_react_refresh()
            .plugin(Box::new(
                rspack_plugin_hmr::HotModuleReplacementPlugin::default(),
            ))
            .plugin(Box::new(rspack_plugin_entry::EntryPlugin::new(
                rspack_core::Context::from(utf8(root).as_str()),
                utf8(&refresh.entry),
                rspack_core::EntryOptions::default(),
            )))
            .plugin(Box::new(
                rspack_plugin_javascript::provide_plugin::ProvidePlugin::new(provide),
            ))
            .plugin(Box::new(RefreshRuntimePlugin::create()));
    }
    if live {
        builder.plugin(Box::new(split_chunks()));
    }
    for plugin in plugins {
        builder.plugin(plugin);
    }
    builder.build().context("preview compiler options")
}

/// `splitChunks: { chunks: "all" }` with Rspack's development defaults:
/// packages in a vendors group, modules two entries share in a default group.
fn split_chunks() -> rspack_plugin_split_chunks::SplitChunksPlugin {
    use rspack_core::SourceType;
    use rspack_plugin_split_chunks::{
        CacheGroup, CacheGroupTest, ChunkFilter, ChunkNameGetter, FallbackCacheGroup,
        PluginOptions, SplitChunkSizes, SplitChunksPlugin,
    };
    let types = [SourceType::JavaScript, SourceType::Unknown];
    let sizes = |bytes: f64| SplitChunkSizes::with_initial_value(&types, bytes);
    let group = |key: &str, test: CacheGroupTest, priority: f64, min_chunks: u32, id_hint: &str| {
        CacheGroup {
            key: key.to_owned(),
            chunk_filter: ChunkFilter::All,
            test,
            r#type: rspack_plugin_split_chunks::create_default_module_type_filter(),
            layer: rspack_plugin_split_chunks::create_default_module_layer_filter(),
            name: ChunkNameGetter::Disabled,
            priority,
            min_size: sizes(10_000.0),
            min_size_reduction: sizes(10_000.0),
            enforce_size_threshold: sizes(30_000.0),
            reuse_existing_chunk: true,
            min_chunks,
            id_hint: id_hint.to_owned(),
            max_initial_requests: f64::INFINITY,
            max_async_requests: f64::INFINITY,
            max_async_size: SplitChunkSizes::empty(),
            max_initial_size: SplitChunkSizes::empty(),
            filename: None,
            automatic_name_delimiter: "-".to_owned(),
            used_exports: false,
        }
    };
    SplitChunksPlugin::new(PluginOptions {
        dedup_depth: 0,
        cache_groups: vec![
            group(
                "defaultVendors",
                CacheGroupTest::RegExp(regex(r"[\\/]node_modules[\\/]")),
                -10.0,
                1,
                "vendors",
            ),
            group("default", CacheGroupTest::Enabled, -20.0, 2, ""),
        ],
        fallback_cache_group: FallbackCacheGroup {
            chunks_filter: ChunkFilter::All,
            min_size: sizes(10_000.0),
            max_async_size: SplitChunkSizes::empty(),
            max_initial_size: SplitChunkSizes::empty(),
            automatic_name_delimiter: "-".to_owned(),
        },
        hide_path_info: None,
    })
}
