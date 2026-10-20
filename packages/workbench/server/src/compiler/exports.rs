//! The named exports of a project module, read statically with SWC and
//! following `export * from` through local files, for docs file lenses.
//! Type-only exports are not values and are left out; packages stay closed,
//! as in a build with packages external.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast as ast;
use swc_ecma_parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax};

/// How `export * from` specifiers resolve to files.
pub(crate) struct Resolution<'a> {
    pub(crate) root: &'a Path,
    pub(crate) extensions: &'a [String],
    /// Exact aliases, with targets already absolute or package names.
    pub(crate) aliases: &'a [(String, String)],
}

fn parse(file: &Path) -> Result<ast::Module, String> {
    let source =
        std::fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let map = Lrc::<SourceMap>::default();
    let input = map.new_source_file(FileName::Anon.into(), source);
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
    let module = parser
        .parse_module()
        .map_err(|error| format!("{}: {:?}", file.display(), error.kind()))?;
    if let Some(error) = parser.take_errors().into_iter().next() {
        return Err(format!("{}: {:?}", file.display(), error.kind()));
    }
    Ok(module)
}

fn bindings(pattern: &ast::Pat, names: &mut BTreeSet<String>) {
    match pattern {
        ast::Pat::Ident(binding) => {
            names.insert(binding.id.sym.to_string());
        }
        ast::Pat::Array(array) => {
            for element in array.elems.iter().flatten() {
                bindings(element, names);
            }
        }
        ast::Pat::Object(object) => {
            for property in &object.props {
                match property {
                    ast::ObjectPatProp::KeyValue(pair) => bindings(&pair.value, names),
                    ast::ObjectPatProp::Assign(assign) => {
                        names.insert(assign.key.id.sym.to_string());
                    }
                    ast::ObjectPatProp::Rest(rest) => bindings(&rest.arg, names),
                }
            }
        }
        ast::Pat::Rest(rest) => bindings(&rest.arg, names),
        ast::Pat::Assign(assign) => bindings(&assign.left, names),
        _ => {}
    }
}

fn export_name(name: &ast::ModuleExportName) -> String {
    match name {
        ast::ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ast::ModuleExportName::Str(text) => text.value.to_string_lossy().into_owned(),
    }
}

impl Resolution<'_> {
    fn candidates(&self, base: &Path) -> Vec<PathBuf> {
        let mut candidates = vec![base.to_path_buf()];
        for extension in self.extensions {
            candidates.push(PathBuf::from(format!("{}{extension}", base.display())));
        }
        for extension in self.extensions {
            candidates.push(base.join(format!("index{extension}")));
        }
        candidates
    }

    /// A local file for `specifier` from `importer`, or `None` for a package.
    fn resolve(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        let base = if specifier.starts_with('.') {
            crate::paths::resolve(importer.parent().unwrap_or(self.root), specifier)
        } else if specifier.starts_with('/') {
            PathBuf::from(specifier)
        } else {
            let (_, target) = self.aliases.iter().find(|(key, _)| key == specifier)?;
            let target = Path::new(target);
            if !target.is_absolute() {
                return None;
            }
            target.to_path_buf()
        };
        self.candidates(&base)
            .into_iter()
            .find(|candidate| candidate.is_file())
    }

    /// The module's value exports in byte order, `default` included.
    pub(crate) fn exports(&self, file: &Path) -> Result<Vec<String>, String> {
        let mut names = BTreeSet::new();
        let mut visited = BTreeSet::new();
        self.collect(file, &mut names, &mut visited, true)?;
        Ok(names.into_iter().collect())
    }

    fn collect(
        &self,
        file: &Path,
        names: &mut BTreeSet<String>,
        visited: &mut BTreeSet<PathBuf>,
        with_default: bool,
    ) -> Result<(), String> {
        if !visited.insert(file.to_path_buf()) {
            return Ok(());
        }
        let module = parse(file)?;
        for item in &module.body {
            let ast::ModuleItem::ModuleDecl(declaration) = item else {
                continue;
            };
            match declaration {
                ast::ModuleDecl::ExportDecl(export) => match &export.decl {
                    ast::Decl::Var(variables) => {
                        for declarator in &variables.decls {
                            bindings(&declarator.name, names);
                        }
                    }
                    ast::Decl::Fn(function) => {
                        names.insert(function.ident.sym.to_string());
                    }
                    ast::Decl::Class(class) => {
                        names.insert(class.ident.sym.to_string());
                    }
                    ast::Decl::TsEnum(enumeration) if !enumeration.declare => {
                        names.insert(enumeration.id.sym.to_string());
                    }
                    ast::Decl::Using(using) => {
                        for declarator in &using.decls {
                            bindings(&declarator.name, names);
                        }
                    }
                    _ => {}
                },
                ast::ModuleDecl::ExportNamed(export) if !export.type_only => {
                    for item in &export.specifiers {
                        match item {
                            ast::ExportSpecifier::Named(spec) if !spec.is_type_only => {
                                names.insert(export_name(
                                    spec.exported.as_ref().unwrap_or(&spec.orig),
                                ));
                            }
                            ast::ExportSpecifier::Namespace(spec) => {
                                names.insert(export_name(&spec.name));
                            }
                            ast::ExportSpecifier::Default(spec) => {
                                names.insert(spec.exported.sym.to_string());
                            }
                            ast::ExportSpecifier::Named(_) => {}
                        }
                    }
                }
                ast::ModuleDecl::ExportDefaultDecl(_) | ast::ModuleDecl::ExportDefaultExpr(_)
                    if with_default =>
                {
                    names.insert("default".to_owned());
                }
                ast::ModuleDecl::ExportAll(all) if !all.type_only => {
                    let request = all.src.value.to_string_lossy();
                    if let Some(target) = self.resolve(file, &request) {
                        // `export *` never re-exports a default.
                        self.collect(&target, names, visited, false)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_follow_local_reexports_and_skip_types() {
        let dir = std::env::temp_dir().join(format!("acme-exports-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(
            dir.join("main.ts"),
            "export const zeta = 1, { inner, deep: [first] } = {} as any;\nexport * from './more';\nexport * from 'acme-package';\nexport function alpha() {}\nexport { zeta as beta };\nexport default 3;\nexport type T = 1;\nexport { type U } from './more';\nexport * as grouped from './nested';\n// export const commented = 1;\nconst text = 'export const quoted = 1';\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("more.ts"),
            "export const middle = 2; export const zeta2 = 1; export type U = 1; export default 4;\n",
        )
        .unwrap();
        std::fs::write(dir.join("nested/index.ts"), "export const hidden = 1;\n").unwrap();
        let extensions = vec![".ts".to_owned(), ".tsx".to_owned()];
        let resolution = Resolution {
            root: &dir,
            extensions: &extensions,
            aliases: &[],
        };
        assert_eq!(
            resolution.exports(&dir.join("main.ts")).unwrap(),
            [
                "alpha", "beta", "default", "first", "grouped", "inner", "middle", "zeta", "zeta2"
            ]
        );
        std::fs::write(dir.join("broken.ts"), "export const = ;").unwrap();
        assert!(resolution.exports(&dir.join("broken.ts")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
