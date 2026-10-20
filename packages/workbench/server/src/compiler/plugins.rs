//! The project's module rules and native plugin capabilities, from
//! `workbench.config.ts` as data. Plugins are the plain objects the
//! `@canonic2/workbench` helpers make; each names its `plugin`:
//!
//! - `virtual-module`: `{ id, source }`, a module made from source text;
//! - `resolve`: `{ request, importer?, to }`, an exact import sent to another
//!   file, optionally only from importers matching `importer`;
//! - `import-glob`: `{ include? }`, `import.meta.glob` expanded at build time;
//! - `svg-sprite`: `{ dirs, symbolId?, register?, names?, inject?, domId? }`,
//!   an SVG sprite's registration and names modules.
//!
//! The global lists apply to every compiler; an adapter's apply only to the
//! compiler that builds that adapter's pages. Project paths and identifiers
//! are configuration: nothing here knows a project.
#![expect(
    clippy::unused_async,
    reason = "Rspack's hook macros need async functions, whether or not they await"
)]
pub(crate) mod glob;
pub(crate) mod svg;

use super::Profile;
use super::entries::digest;
use super::loaders::{GLOB, NODE, SPRITE};
use super::options::{loader, regex};
use super::overlay::Overlay;
use super::project::ProjectConfig;
use anyhow::{Result, anyhow, bail};
use rspack_core::{
    BoxPlugin, ModuleFactoryCreateData, ModuleRule, ModuleRuleEffect, ModuleRuleEnforce,
    ModuleRuleUse, ModuleRuleUseLoader, NormalModuleFactoryBeforeResolve, Plugin, RuleSetCondition,
    RuleSetConditionWithEmpty, RuleSetLogicalConditions,
};
use rspack_hook::{plugin, plugin_hook};
use rspack_regex::RspackRegex;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Which importers a resolution applies to.
#[derive(Clone, Debug)]
enum Importers {
    /// Globs over the importer's path from the project root.
    Globs(globset::GlobSet),
    /// A regular expression over the importer's absolute path.
    Pattern(RspackRegex),
}

impl Importers {
    fn matches(&self, root: &Path, importer: &str) -> bool {
        match self {
            Self::Globs(globs) => Path::new(importer)
                .strip_prefix(root)
                .is_ok_and(|relative| globs.is_match(relative)),
            Self::Pattern(pattern) => pattern.test(importer),
        }
    }
}

#[derive(Clone, Debug)]
struct Redirect {
    request: String,
    importers: Option<Importers>,
    target: String,
}

fn js_regex(value: &Value) -> Option<Result<RspackRegex>> {
    let tagged = value.get("$regexp")?;
    let source = tagged
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let flags = tagged
        .get("flags")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(
        RspackRegex::with_flags(source, flags)
            .map_err(|error| anyhow!("/{source}/{flags}: {error}")),
    )
}

fn importers(value: &Value) -> Result<Importers> {
    if let Some(pattern) = js_regex(value) {
        return Ok(Importers::Pattern(pattern?));
    }
    let patterns: Vec<&str> = match value {
        Value::String(text) => vec![text.as_str()],
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => bail!("importer must be a glob, a list of globs or a regular expression"),
    };
    let mut set = globset::GlobSetBuilder::new();
    for pattern in patterns {
        set.add(
            globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(|error| anyhow!("importer {pattern}: {error}"))?,
        );
    }
    Ok(Importers::Globs(set.build()?))
}

/// Every plugin of the config that applies to `profile`, with where it came from.
fn plugins_for(config: &ProjectConfig, profile: Option<Profile>) -> Vec<(String, &Value)> {
    let mut list: Vec<(String, &Value)> = config
        .plugins
        .iter()
        .map(|plugin| (String::new(), plugin))
        .collect();
    for (name, adapter) in &config.adapters {
        if profile.is_some_and(|profile| Profile::of(name) != profile) {
            continue;
        }
        for plugin in adapter
            .get("plugins")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            list.push((format!("adapter {name}: "), plugin));
        }
    }
    list
}

fn text_field<'a>(plugin: &'a Value, key: &str) -> Option<&'a str> {
    plugin
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// What the native compiler makes of one profile's plugins.
#[derive(Default)]
struct Capabilities {
    redirects: Vec<Redirect>,
    modules: Vec<(String, String)>,
    globs: Vec<Option<Value>>,
    sprites: Vec<(String, String, svg::Sprite, String)>,
}

fn capabilities(
    config: &ProjectConfig,
    profile: Option<Profile>,
    root: &Path,
) -> (Capabilities, Vec<String>) {
    let mut found = Capabilities::default();
    let mut problems = vec![];
    let virtual_dir = root.join(".workbench-rspack/virtual");
    for (where_, plugin) in plugins_for(config, profile) {
        let kind = text_field(plugin, "plugin").unwrap_or_default();
        let problem =
            |message: &str| format!("workbench.config.ts: {where_}{kind} plugin: {message}");
        match kind {
            "virtual-module" => match (text_field(plugin, "id"), plugin.get("source").and_then(Value::as_str)) {
                (Some(id), Some(source)) => {
                    let file = virtual_dir.join(format!("{}.js", digest(id)));
                    found.redirects.push(Redirect {
                        request: id.to_owned(),
                        importers: None,
                        target: file.to_string_lossy().into_owned(),
                    });
                    found.modules.push((file.to_string_lossy().into_owned(), source.to_owned()));
                }
                _ => problems.push(problem("needs an id and its source text.")),
            },
            "resolve" => match (text_field(plugin, "request"), text_field(plugin, "to")) {
                (Some(request), Some(to)) => {
                    let importers = match plugin.get("importer") {
                        None | Some(Value::Null) => Ok(None),
                        Some(value) => importers(value).map(Some),
                    };
                    match importers {
                        Ok(importers) => found.redirects.push(Redirect {
                            request: request.to_owned(),
                            importers,
                            target: if to.starts_with('.') || Path::new(to).is_absolute() {
                                crate::paths::resolve(root, to).to_string_lossy().into_owned()
                            } else {
                                to.to_owned()
                            },
                        }),
                        Err(error) => problems.push(problem(&format!("{error:#}"))),
                    }
                }
                _ => problems.push(problem("needs the request it matches and the file it resolves to.")),
            },
            "import-glob" => found.globs.push(plugin.get("include").cloned()),
            "svg-sprite" => {
                let dirs: Vec<PathBuf> = match plugin.get("dirs") {
                    Some(Value::String(dir)) => vec![crate::paths::resolve(root, dir)],
                    Some(Value::Array(dirs)) => dirs
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|dir| crate::paths::resolve(root, dir))
                        .collect(),
                    _ => vec![],
                };
                if dirs.is_empty() {
                    problems.push(problem("needs the folders that hold its icons."));
                    continue;
                }
                let symbol_id = text_field(plugin, "symbolId").unwrap_or("icon-[dir]-[name]");
                if !symbol_id.contains("[name]") {
                    problems.push(problem("symbolId must contain [name]."));
                    continue;
                }
                let sprite = svg::Sprite {
                    dirs,
                    symbol_id: symbol_id.to_owned(),
                    inject: text_field(plugin, "inject").unwrap_or("body-last").to_owned(),
                    dom_id: text_field(plugin, "domId").unwrap_or("__svg__icons__dom__").to_owned(),
                };
                let key = digest(&serde_json::to_string(&sprite).unwrap_or_default());
                for (module, default_id) in [
                    ("register", "virtual:svg-icons-register"),
                    ("names", "virtual:svg-icons-names"),
                ] {
                    let id = text_field(plugin, module).unwrap_or(default_id).to_owned();
                    let file = virtual_dir
                        .join(format!("svg-{key}-{module}.js"))
                        .to_string_lossy()
                        .into_owned();
                    found.redirects.push(Redirect {
                        request: id.clone(),
                        importers: None,
                        target: file.clone(),
                    });
                    found.sprites.push((file, module.to_owned(), sprite.clone(), id));
                }
            }
            _ => problems.push(format!(
                "workbench.config.ts: {where_}a plugin names no capability Workbench provides: use a plugin from @canonic2/workbench."
            )),
        }
    }
    (found, problems)
}

/// Problems with the config's plugins, for every profile.
pub(crate) fn problems(config: &ProjectConfig) -> Vec<String> {
    let mut problems = capabilities(config, None, Path::new("/")).1;
    for rule in rules_for(config, None) {
        if let Err(error) = module_rule(rule, Path::new("/")) {
            problems.push(format!("workbench.config.ts: {error:#}"));
        }
    }
    problems.dedup();
    problems
}

/// Sends matching imports to their configured files before resolution.
#[plugin]
#[derive(Debug)]
struct RedirectPlugin {
    root: PathBuf,
    redirects: Vec<Redirect>,
}

#[plugin_hook(NormalModuleFactoryBeforeResolve for RedirectPlugin)]
async fn before_resolve(
    &self,
    data: &mut ModuleFactoryCreateData,
) -> rspack_error::Result<Option<bool>> {
    let importer = data.issuer.as_deref().unwrap_or_default();
    let found = self.redirects.iter().find(|redirect| {
        redirect.request == data.request
            && redirect
                .importers
                .as_ref()
                .is_none_or(|importers| importers.matches(&self.root, importer))
    });
    if let Some(redirect) = found {
        data.request.clone_from(&redirect.target);
    }
    Ok(None)
}

impl Plugin for RedirectPlugin {
    fn name(&self) -> &'static str {
        "WorkbenchRedirectPlugin"
    }

    fn apply(&self, context: &mut rspack_core::ApplyContext<'_>) -> rspack_error::Result<()> {
        context
            .normal_module_factory_hooks
            .before_resolve
            .tap(before_resolve::new(self));
        Ok(())
    }
}

// --------------------------------------------------------------- rules

fn rules_for(config: &ProjectConfig, profile: Option<Profile>) -> Vec<&Value> {
    let mut rules: Vec<&Value> = config.rules.iter().collect();
    for (name, adapter) in &config.adapters {
        if profile.is_some_and(|profile| Profile::of(name) != profile) {
            continue;
        }
        rules.extend(
            adapter
                .get("rules")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        );
    }
    rules
}

fn condition(value: &Value, root: &Path) -> Result<RuleSetCondition> {
    if let Some(pattern) = js_regex(value) {
        return Ok(RuleSetCondition::Regexp(pattern?));
    }
    Ok(match value {
        // A string matches an absolute path by prefix; relative ones start at the root.
        Value::String(text) => RuleSetCondition::String(if Path::new(text).is_absolute() {
            text.clone()
        } else {
            crate::paths::resolve(root, text)
                .to_string_lossy()
                .into_owned()
        }),
        Value::Array(items) => RuleSetCondition::Array {
            items: items
                .iter()
                .map(|item| condition(item, root))
                .collect::<Result<_>>()?,
            can_sync: true,
        },
        Value::Object(object) => {
            let list = |key: &str| -> Result<Option<Vec<RuleSetCondition>>> {
                object
                    .get(key)
                    .map(|items| match items {
                        Value::Array(items) => {
                            items.iter().map(|item| condition(item, root)).collect()
                        }
                        item => Ok(vec![condition(item, root)?]),
                    })
                    .transpose()
            };
            let not = object
                .get("not")
                .map(|item| condition(item, root))
                .transpose()?;
            RuleSetCondition::Logical(Box::new(RuleSetLogicalConditions::new(
                list("and")?,
                list("or")?,
                not,
            )))
        }
        other => bail!(
            "a rule condition must be a string, a regular expression, a list or {{ and, or, not }}, not {other}"
        ),
    })
}

fn uses(value: &Value, options: Option<&Value>) -> Result<Vec<ModuleRuleUseLoader>> {
    let one = |request: &str, options: Option<&Value>| -> ModuleRuleUseLoader {
        if request.starts_with("builtin:") {
            loader(request, options)
        } else {
            // A project's JavaScript loader runs in the Node evaluator.
            let wrapped =
                json!({ "loader": request, "options": options.cloned().unwrap_or(Value::Null) });
            loader(
                &format!("{NODE}?{}", digest(&wrapped.to_string())),
                Some(&wrapped),
            )
        }
    };
    Ok(match value {
        Value::String(request) => vec![one(request, options)],
        Value::Array(items) => items
            .iter()
            .map(|item| uses(item, None))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect(),
        Value::Object(object) => match object.get("loader").and_then(Value::as_str) {
            Some(request) => vec![one(request, object.get("options"))],
            None => bail!("a rule's use needs a loader"),
        },
        _ => bail!("a rule's use must be a loader, an object with a loader, or a list of them"),
    })
}

const RULE_KEYS: [&str; 13] = [
    "test",
    "include",
    "exclude",
    "resource",
    "resourceQuery",
    "issuer",
    "enforce",
    "type",
    "loader",
    "options",
    "use",
    "oneOf",
    "rules",
];

/// One rule of the config as an Rspack module rule.
fn module_rule(value: &Value, root: &Path) -> Result<ModuleRule> {
    let Some(object) = value.as_object() else {
        bail!("a rule must be an object");
    };
    if let Some(key) = object
        .keys()
        .find(|key| !RULE_KEYS.contains(&key.as_str()) && key.as_str() != "sideEffects")
    {
        bail!("rules[{key}] is not supported by Workbench's compiler");
    }
    let get = |key: &str| object.get(key).filter(|value| !value.is_null());
    let mut rule = ModuleRule {
        test: get("test")
            .map(|value| condition(value, root))
            .transpose()?,
        include: get("include")
            .map(|value| condition(value, root))
            .transpose()?,
        exclude: get("exclude")
            .map(|value| condition(value, root))
            .transpose()?,
        resource: get("resource")
            .map(|value| condition(value, root))
            .transpose()?,
        resource_query: get("resourceQuery")
            .map(|value| condition(value, root).map(RuleSetConditionWithEmpty::new))
            .transpose()?,
        issuer: get("issuer")
            .map(|value| condition(value, root).map(RuleSetConditionWithEmpty::new))
            .transpose()?,
        ..ModuleRule::default()
    };
    let mut effect = ModuleRuleEffect {
        r#type: get("type").and_then(Value::as_str).map(Into::into),
        side_effects: get("sideEffects").and_then(Value::as_bool),
        enforce: match get("enforce").and_then(Value::as_str) {
            Some("pre") => ModuleRuleEnforce::Pre,
            Some("post") => ModuleRuleEnforce::Post,
            Some(other) => bail!("enforce must be pre or post, not {other}"),
            None => ModuleRuleEnforce::Normal,
        },
        ..Default::default()
    };
    if let Some(request) = get("loader").and_then(Value::as_str) {
        effect.r#use = ModuleRuleUse::Array(uses(&json!(request), get("options"))?);
    } else if let Some(value) = get("use") {
        effect.r#use = ModuleRuleUse::Array(uses(value, None)?);
    }
    rule.effect = effect;
    let nested = |key: &str| -> Result<Option<Vec<ModuleRule>>> {
        get(key)
            .map(|items| {
                items
                    .as_array()
                    .ok_or_else(|| anyhow!("{key} must be a list of rules"))?
                    .iter()
                    .map(|item| module_rule(item, root))
                    .collect()
            })
            .transpose()
    };
    rule.one_of = nested("oneOf")?;
    rule.rules = nested("rules")?;
    Ok(rule)
}

/// The rules and plugins one profile's compiler adds after Workbench's own.
/// Generated modules the capabilities need are written to `overlay`.
pub(crate) fn compile(
    config: &ProjectConfig,
    profile: Profile,
    root: &Path,
    overlay: &Overlay,
) -> Result<(Vec<ModuleRule>, Vec<BoxPlugin>)> {
    let (found, _) = capabilities(config, Some(profile), root);
    let mut rules = vec![];
    for (file, source) in &found.modules {
        overlay.write(file, source);
    }
    for include in &found.globs {
        let mut rule = ModuleRule {
            test: Some(RuleSetCondition::Regexp(regex(r"\.[cm]?[jt]sx?$"))),
            exclude: Some(RuleSetCondition::Regexp(regex(
                "[\\\\/]node_modules[\\\\/]",
            ))),
            effect: ModuleRuleEffect {
                enforce: ModuleRuleEnforce::Pre,
                r#use: ModuleRuleUse::Array(vec![loader(GLOB, None)]),
                ..Default::default()
            },
            ..Default::default()
        };
        if let Some(include) = include {
            rule.include = Some(condition(include, root)?);
        }
        rules.push(rule);
    }
    for (file, module, sprite, _) in &found.sprites {
        // A placeholder the resolver finds; the sprite loader writes the module.
        overlay.write(file, "export default {};\n");
        let options = svg::loader_options(sprite, module);
        rules.push(ModuleRule {
            resource: Some(RuleSetCondition::String(file.clone())),
            effect: ModuleRuleEffect {
                r#type: Some("javascript/auto".into()),
                r#use: ModuleRuleUse::Array(vec![loader(
                    &format!("{SPRITE}?{}", digest(&options.to_string())),
                    Some(&options),
                )]),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    for value in rules_for(config, Some(profile)) {
        // An invalid rule is reported with the config's problems.
        if let Ok(rule) = module_rule(value, root) {
            rules.push(rule);
        }
    }
    let mut plugins: Vec<BoxPlugin> = vec![];
    if !found.redirects.is_empty() {
        plugins.push(Box::new(RedirectPlugin::new_inner(
            root.to_path_buf(),
            found.redirects,
        )));
    }
    Ok((rules, plugins))
}

/// The data form of a native plugin descriptor, for tests and the SDK.
#[cfg(test)]
fn descriptor(kind: &str, fields: Value) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("plugin".into(), json!(kind));
    if let Value::Object(fields) = fields {
        object.extend(fields);
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(plugins: &[Value], rules: &[Value]) -> ProjectConfig {
        serde_json::from_value(json!({ "plugins": plugins, "rules": rules })).unwrap()
    }

    #[test]
    fn plugins_are_checked_and_reported() {
        let config = config(
            &[
                descriptor("virtual-module", json!({ "id": "virtual:acme" })),
                descriptor(
                    "resolve",
                    json!({ "request": "@acme/platform", "to": "./web.ts", "importer": 3 }),
                ),
                descriptor("svg-sprite", json!({ "dirs": [] })),
                descriptor("svg-sprite", json!({ "dirs": "icons", "symbolId": "icon" })),
                json!({ "plugin": "acme-unknown" }),
            ],
            &[
                json!({ "test": { "$regexp": { "source": "\\.acme$", "flags": "" } }, "generator": {} }),
            ],
        );
        let problems = problems(&config);
        assert!(problems[0].contains("virtual-module plugin: needs an id and its source text."));
        assert!(problems[1].contains("resolve plugin: importer must be"));
        assert!(problems[2].contains("needs the folders"));
        assert!(problems[3].contains("symbolId must contain [name]"));
        assert!(problems[4].contains("names no capability"));
        assert!(problems[5].contains("rules[generator] is not supported"));
    }

    #[test]
    fn importers_match_by_glob_or_pattern() {
        let root = Path::new("/acme");
        let globs = importers(&json!("src/native/**")).unwrap();
        assert!(globs.matches(root, "/acme/src/native/screen.tsx"));
        assert!(!globs.matches(root, "/acme/src/web/screen.tsx"));
        let pattern =
            importers(&json!({ "$regexp": { "source": "[\\\\/]native[\\\\/]", "flags": "" } }))
                .unwrap();
        assert!(pattern.matches(root, "/acme/src/native/screen.tsx"));
        assert!(!pattern.matches(root, "/acme/src/web/screen.tsx"));
    }

    #[test]
    fn rules_become_rspack_rules_with_project_loaders_bridged() {
        let rule = module_rule(
            &json!({
                "test": { "$regexp": { "source": "\\.tsx?$", "flags": "" } },
                "exclude": "node_modules", "enforce": "pre",
                "loader": "/acme/workbench/greeting-loader.cjs", "options": { "greeting": "hi" },
            }),
            Path::new("/acme"),
        )
        .unwrap();
        assert!(matches!(rule.effect.enforce, ModuleRuleEnforce::Pre));
        let ModuleRuleUse::Array(loaders) = &rule.effect.r#use else {
            panic!("loaders")
        };
        assert!(loaders[0].loader.starts_with(NODE));
        let options: Value = serde_json::from_str(loaders[0].options.as_deref().unwrap()).unwrap();
        assert_eq!(options["loader"], "/acme/workbench/greeting-loader.cjs");
        assert_eq!(options["options"]["greeting"], "hi");
        assert!(
            matches!(&rule.exclude, Some(RuleSetCondition::String(text)) if text == "/acme/node_modules")
        );
        let builtin = module_rule(
            &json!({ "use": [{ "loader": "builtin:lightningcss-loader" }] }),
            Path::new("/acme"),
        )
        .unwrap();
        let ModuleRuleUse::Array(loaders) = &builtin.effect.r#use else {
            panic!("loaders")
        };
        assert_eq!(loaders[0].loader, "builtin:lightningcss-loader");
    }
}
