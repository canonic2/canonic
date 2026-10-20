//! `import.meta.glob(...)` expanded into imports, as Vite expands it: each
//! call's literal patterns are matched against the project's files and the
//! call becomes an object from each file, relative to the importer as the
//! pattern was written, to its module, a named import, or a function that
//! imports it. Calls are found in the parsed module, so comments and string
//! literals are left as written. The folders searched are dependencies of the
//! module, so adding or removing a matching file rebuilds it.
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rspack_cacheable::{cacheable, cacheable_dyn};
use rspack_core::RunnerContext;
use rspack_error::{Result, error};
use rspack_loader_runner::{Identifier, Loader, LoaderContext};
use std::path::{Path, PathBuf};
use swc_common::{FileName, SourceMap, Spanned, sync::Lrc};
use swc_ecma_ast as ast;
use swc_ecma_parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_visit::{Visit, VisitWith};

/// One call's literal arguments.
#[derive(Debug, Default, PartialEq, Eq)]
struct Call {
    start: usize,
    end: usize,
    patterns: Vec<String>,
    eager: bool,
    import: Option<String>,
    query: Option<String>,
    exhaustive: bool,
}

fn literal(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(text)) => text.value.as_str().map(str::to_owned),
        ast::Expr::Tpl(template) if template.exprs.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.cooked.as_ref())
            .and_then(|cooked| cooked.as_str())
            .map(str::to_owned),
        _ => None,
    }
}

fn is_glob_callee(callee: &ast::Callee) -> bool {
    let ast::Callee::Expr(expr) = callee else {
        return false;
    };
    let ast::Expr::Member(member) = &**expr else {
        return false;
    };
    let ast::Expr::MetaProp(meta) = &*member.obj else {
        return false;
    };
    meta.kind == ast::MetaPropKind::ImportMeta
        && matches!(&member.prop, ast::MemberProp::Ident(name) if name.sym == *"glob")
}

struct Finder {
    offset: u32,
    calls: Vec<Result<Call, String>>,
}

impl Finder {
    fn read(&self, call: &ast::CallExpr) -> Result<Call, String> {
        let span = call.span();
        let mut found = Call {
            start: (span.lo.0 - self.offset) as usize,
            end: (span.hi.0 - self.offset) as usize,
            ..Call::default()
        };
        let Some(first) = call.args.first() else {
            return Err("import.meta.glob needs a pattern.".into());
        };
        match &*first.expr {
            ast::Expr::Array(array) => {
                for element in array.elems.iter().flatten() {
                    found.patterns.push(
                        literal(&element.expr)
                            .ok_or("import.meta.glob patterns must be string literals.")?,
                    );
                }
            }
            expr => found
                .patterns
                .push(literal(expr).ok_or("import.meta.glob patterns must be string literals.")?),
        }
        if let Some(options) = call.args.get(1) {
            let ast::Expr::Object(object) = &*options.expr else {
                return Err("import.meta.glob options must be an object literal.".into());
            };
            for property in &object.props {
                let ast::PropOrSpread::Prop(property) = property else {
                    return Err("import.meta.glob options must be literal.".into());
                };
                let ast::Prop::KeyValue(pair) = &**property else {
                    return Err("import.meta.glob options must be literal.".into());
                };
                let key = match &pair.key {
                    ast::PropName::Ident(name) => name.sym.to_string(),
                    ast::PropName::Str(name) => name.value.to_string_lossy().into_owned(),
                    _ => return Err("import.meta.glob options must be literal.".into()),
                };
                let boolean = || match &*pair.value {
                    ast::Expr::Lit(ast::Lit::Bool(value)) => Ok(value.value),
                    _ => Err(format!(
                        "import.meta.glob option {key} must be true or false."
                    )),
                };
                match key.as_str() {
                    "eager" => found.eager = boolean()?,
                    "exhaustive" => found.exhaustive = boolean()?,
                    "import" => {
                        found.import = Some(
                            literal(&pair.value)
                                .ok_or("import.meta.glob option import must be a string.")?,
                        );
                    }
                    "query" => {
                        let query = literal(&pair.value)
                            .ok_or("import.meta.glob option query must be a string.")?;
                        found.query = Some(if query.starts_with('?') {
                            query
                        } else {
                            format!("?{query}")
                        });
                    }
                    other => {
                        return Err(format!("import.meta.glob option {other} is not supported."));
                    }
                }
            }
        }
        Ok(found)
    }
}

impl Visit for Finder {
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if is_glob_callee(&call.callee) {
            self.calls.push(self.read(call));
            return;
        }
        call.visit_children_with(self);
    }
}

fn parse(source: &str, file: &Path) -> Result<(Vec<Result<Call, String>>, u32), String> {
    let map = Lrc::<SourceMap>::default();
    let input = map.new_source_file(FileName::Anon.into(), source.to_owned());
    let extension = file.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let syntax = match extension {
        "ts" | "mts" | "cts" => Syntax::Typescript(TsSyntax::default()),
        "tsx" => Syntax::Typescript(TsSyntax {
            tsx: true,
            ..Default::default()
        }),
        _ => Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
    };
    let mut parser = Parser::new(syntax, StringInput::from(&*input), None);
    let program = parser
        .parse_program()
        .map_err(|error| format!("{:?}", error.kind()))?;
    let mut finder = Finder {
        offset: input.start_pos.0,
        calls: vec![],
    };
    program.visit_with(&mut finder);
    Ok((finder.calls, input.start_pos.0))
}

/// The folder a pattern's static prefix names, and the pattern from the root.
fn base_of(pattern: &str, importer_dir: &Path, root: &Path) -> Result<(PathBuf, String), String> {
    let absolute = if let Some(rooted) = pattern.strip_prefix('/') {
        root.join(rooted)
    } else if pattern.starts_with("./") || pattern.starts_with("../") {
        importer_dir.join(pattern)
    } else {
        return Err(format!(
            "import.meta.glob pattern {pattern} must start with ./, ../ or /."
        ));
    };
    let normalized = crate::paths::resolve(Path::new("/"), &absolute.to_string_lossy());
    let text = normalized.to_string_lossy().replace('\\', "/");
    let glob_at = text.find(['*', '?', '[', '{']).unwrap_or(text.len());
    let base = text[..glob_at]
        .rsplit_once('/')
        .map_or("/", |(base, _)| base);
    Ok((
        PathBuf::from(if base.is_empty() { "/" } else { base }),
        text,
    ))
}

fn walk(folder: &Path, exhaustive: bool, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !exhaustive && (name == "node_modules" || name.starts_with('.')) {
            continue;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&path, exhaustive, files),
            Ok(kind) if kind.is_file() => files.push(path),
            _ => {}
        }
    }
}

/// `to` relative to `from_dir`, starting with `./` or `../` as Vite writes keys.
fn relative_key(from_dir: &Path, to: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let target: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        target[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    let joined = parts.join("/");
    if joined.starts_with("..") {
        joined
    } else {
        format!("./{joined}")
    }
}

/// What expanding a module's calls produced, and the folders it searched.
#[derive(Debug, Default)]
pub(crate) struct Expanded {
    pub(crate) source: String,
    pub(crate) folders: Vec<PathBuf>,
}

fn glob(pattern: &str) -> Result<globset::Glob, String> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|error| format!("import.meta.glob pattern {pattern}: {error}"))
}

/// Expands every `import.meta.glob` call in `source`; `None` when it has none.
#[expect(
    clippy::too_many_lines,
    reason = "one pass over the calls: patterns, files, keys and imports"
)]
pub(crate) fn expand(source: &str, file: &Path, root: &Path) -> Result<Option<Expanded>, String> {
    if !source.contains("import.meta.glob") {
        return Ok(None);
    }
    let (calls, _) = parse(source, file)?;
    if calls.is_empty() {
        return Ok(None);
    }
    let directory = file.parent().unwrap_or(root);
    let mut imports = vec![];
    let mut replacements = vec![];
    let mut folders = vec![];
    for (index, call) in calls.into_iter().enumerate() {
        let call = call?;
        let mut include = GlobSetBuilder::new();
        let mut exclude = GlobSetBuilder::new();
        let mut bases = vec![];
        let mut rooted = false;
        for pattern in &call.patterns {
            let (negative, pattern) = pattern
                .strip_prefix('!')
                .map_or((false, pattern.as_str()), |rest| (true, rest));
            rooted |= pattern.starts_with('/');
            let (base, absolute) = base_of(pattern, directory, root)?;
            if negative {
                exclude.add(glob(&absolute)?);
            } else {
                include.add(glob(&absolute)?);
                bases.push(base);
            }
        }
        let include: GlobSet = include.build().map_err(|error| error.to_string())?;
        let exclude: GlobSet = exclude.build().map_err(|error| error.to_string())?;
        let mut files = vec![];
        for base in &bases {
            walk(base, call.exhaustive, &mut files);
            folders.push(base.clone());
        }
        let mut keyed: Vec<(String, PathBuf)> = files
            .into_iter()
            .filter(|found| found != file)
            .filter(|found| {
                let text = found.to_string_lossy().replace('\\', "/");
                include.is_match(&text) && !exclude.is_match(&text)
            })
            .map(|found| {
                let key = if rooted {
                    format!(
                        "/{}",
                        found
                            .strip_prefix(root)
                            .unwrap_or(&found)
                            .to_string_lossy()
                            .replace('\\', "/")
                    )
                } else {
                    relative_key(directory, &found)
                };
                (key, found)
            })
            .collect();
        keyed.sort();
        keyed.dedup_by(|left, right| left.0 == right.0);
        let query = call.query.clone().unwrap_or_default();
        let mut fields = vec![];
        for (at, (key, found)) in keyed.iter().enumerate() {
            let specifier =
                super::super::entries::quote(&format!("{}{query}", relative_key(directory, found)));
            let name = format!("__workbench_glob_{index}_{at}");
            let value = if call.eager {
                match call.import.as_deref() {
                    None | Some("*") => {
                        imports.push(format!("import * as {name} from {specifier};"));
                    }
                    Some(import) => imports.push(format!(
                        "import {{ {} as {name} }} from {specifier};",
                        if import == "default" {
                            "default"
                        } else {
                            import
                        }
                    )),
                }
                name
            } else {
                match call.import.as_deref() {
                    None | Some("*") => format!("() => import({specifier})"),
                    Some(import) => format!(
                        "() => import({specifier}).then((module) => module[{}])",
                        super::super::entries::quote(import)
                    ),
                }
            };
            fields.push(format!("{}: {value}", super::super::entries::quote(key)));
        }
        replacements.push((call.start, call.end, format!("{{ {} }}", fields.join(", "))));
    }
    let mut out = String::with_capacity(source.len());
    let mut last = 0;
    for (start, end, text) in replacements {
        out.push_str(&source[last..start]);
        out.push_str(&text);
        last = end;
    }
    out.push_str(&source[last..]);
    let source = if imports.is_empty() {
        out
    } else {
        format!("{}\n{out}", imports.join("\n"))
    };
    Ok(Some(Expanded { source, folders }))
}

/// The loader that applies [`expand`] to project sources before compiling them.
#[cacheable]
pub(crate) struct GlobLoader {
    identifier: Identifier,
}

impl GlobLoader {
    pub(crate) fn new(identifier: Identifier) -> Self {
        Self { identifier }
    }
}

#[cacheable_dyn]
#[async_trait::async_trait]
impl Loader<RunnerContext> for GlobLoader {
    fn identifier(&self) -> Identifier {
        self.identifier
    }

    async fn run(&self, context: &mut LoaderContext<RunnerContext>) -> Result<()> {
        let Some(input) = context.take_content() else {
            return Ok(());
        };
        let source = input.try_into_string()?;
        let file = context
            .resource_path()
            .map(|path| PathBuf::from(path.as_str()))
            .unwrap_or_default();
        let root = PathBuf::from(context.context.options.context.as_str());
        match expand(&source, &file, &root).map_err(|message| error!("{message}"))? {
            Some(expanded) => {
                for folder in expanded.folders {
                    context.add_context_dependency(folder);
                }
                context.finish_with((expanded.source, None, None));
            }
            None => context.finish_with((source, None, None)),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "acme-glob-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for file in [
            "src/i18n/locales/fr.json",
            "src/i18n/locales/en.json",
            "src/i18n/locales/notes.txt",
            "src/i18n/locales/nested/de.json",
            "src/i18n/index.ts",
        ] {
            std::fs::create_dir_all(dir.join(file).parent().unwrap()).unwrap();
            std::fs::write(dir.join(file), "{}").unwrap();
        }
        dir
    }

    #[test]
    fn eager_globs_become_sorted_default_imports_keyed_as_written() {
        let root = project();
        let file = root.join("src/i18n/index.ts");
        let source = concat!(
            "// import.meta.glob('./locales/*.json') in a comment stays\n",
            "const text = \"import.meta.glob('./x/*.json')\";\n",
            "export const locales = import.meta.glob(\"./locales/*.json\", { eager: true, import: \"default\" });\n",
            "export const lazy = import.meta.glob(['./locales/**/*.json', '!./locales/en.json']);\n",
        );
        let expanded = expand(source, &file, &root).unwrap().unwrap();
        let out = &expanded.source;
        assert!(out.starts_with(
            "import { default as __workbench_glob_0_0 } from \"./locales/en.json\";\nimport { default as __workbench_glob_0_1 } from \"./locales/fr.json\";\n"
        ));
        assert!(out.contains(
            "export const locales = { \"./locales/en.json\": __workbench_glob_0_0, \"./locales/fr.json\": __workbench_glob_0_1 };"
        ));
        assert!(out.contains(
            "export const lazy = { \"./locales/fr.json\": () => import(\"./locales/fr.json\"), \"./locales/nested/de.json\": () => import(\"./locales/nested/de.json\") };"
        ));
        assert!(out.contains("// import.meta.glob('./locales/*.json') in a comment stays"));
        assert!(out.contains("const text = \"import.meta.glob('./x/*.json')\";"));
        assert_eq!(
            expanded.folders,
            [root.join("src/i18n/locales"), root.join("src/i18n/locales")]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rooted_patterns_and_named_imports_and_errors() {
        let root = project();
        let file = root.join("src/i18n/index.ts");
        let source = "const all = import.meta.glob('/src/i18n/locales/*.json', { eager: true, import: 'messages', query: 'raw' });";
        let out = expand(source, &file, &root).unwrap().unwrap().source;
        assert!(out.contains(
            "import { messages as __workbench_glob_0_0 } from \"./locales/en.json?raw\";"
        ));
        assert!(out.contains("\"/src/i18n/locales/en.json\": __workbench_glob_0_0"));
        assert!(expand("const x = 1;", &file, &root).unwrap().is_none());
        for (source, message) in [
            ("import.meta.glob(name)", "string literals"),
            ("import.meta.glob('locales/*.json')", "must start with"),
            (
                "import.meta.glob('./*.json', { as: 'raw' })",
                "not supported",
            ),
        ] {
            assert!(
                expand(source, &file, &root).unwrap_err().contains(message),
                "{source}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
