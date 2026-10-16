//! Conservative, side-effect-free extraction of literal preview definitions.
use serde_json::{Map, Number, Value};
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast as ast;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};

fn value(expr: &ast::Expr) -> Option<Value> {
    use ast::{Expr, Lit};
    match expr {
        Expr::Lit(Lit::Str(text)) => Some(Value::String(text.value.as_str()?.into())),
        Expr::Lit(Lit::Num(number)) => Number::from_f64(number.value).map(Value::Number),
        Expr::Lit(Lit::Bool(boolean)) => Some(Value::Bool(boolean.value)),
        Expr::Lit(Lit::Null(_)) => Some(Value::Null),
        Expr::Tpl(template) if template.exprs.is_empty() => {
            Some(Value::String(template.quasis.first()?.cooked.as_ref()?.as_str()?.into()))
        }
        Expr::Paren(inner) => value(&inner.expr),
        Expr::TsAs(inner) => value(&inner.expr),
        Expr::TsSatisfies(inner) => value(&inner.expr),
        Expr::TsConstAssertion(inner) => value(&inner.expr),
        Expr::Unary(unary) if unary.op == ast::UnaryOp::Minus => {
            Number::from_f64(-value(&unary.arg)?.as_f64()?).map(Value::Number)
        }
        Expr::Array(array) => array
            .elems
            .iter()
            .map(|element| {
                let element = element.as_ref()?;
                if element.spread.is_some() {
                    return None;
                }
                value(&element.expr)
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        Expr::Object(object) => {
            let mut fields = Map::new();
            for property in &object.props {
                let ast::PropOrSpread::Prop(property) = property else {
                    return None;
                };
                let ast::Prop::KeyValue(pair) = property.as_ref() else {
                    return None;
                };
                let key = match &pair.key {
                    ast::PropName::Ident(name) => name.sym.to_string(),
                    ast::PropName::Str(name) => name.value.as_str()?.into(),
                    // Numeric keys and __proto__ have special JavaScript semantics.
                    _ => return None,
                };
                if key == "__proto__" || key.parse::<u32>().is_ok() {
                    return None;
                }
                fields.insert(key, value(&pair.value)?);
            }
            Some(Value::Object(fields))
        }
        _ => None,
    }
}

pub(crate) fn parse(source: String, tsx: bool) -> Option<Value> {
    let map = Lrc::<SourceMap>::default();
    let file = map.new_source_file(FileName::Anon.into(), source);
    let mut parser = Parser::new(
        Syntax::Typescript(TsSyntax { tsx, ..Default::default() }),
        StringInput::from(&*file),
        None,
    );
    let module = parser.parse_module().ok()?;
    if !parser.take_errors().is_empty() {
        return None;
    }
    let mut imported = false;
    let mut result = None;
    for item in module.body {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) => {
                if import.type_only {
                    continue;
                }
                if import.src.value.as_str()? != "@canonic2/workbench" {
                    return None;
                }
                for specifier in import.specifiers {
                    let ast::ImportSpecifier::Named(name) = specifier else {
                        return None;
                    };
                    if name.is_type_only {
                        continue;
                    }
                    let original = name.imported.as_ref().map_or_else(
                        || name.local.sym.to_string(),
                        |name| match name {
                            ast::ModuleExportName::Ident(name) => name.sym.to_string(),
                            ast::ModuleExportName::Str(name) => {
                                name.value.to_string_lossy().into_owned()
                            }
                        },
                    );
                    if original == "definePreview" && name.local.sym == *"definePreview" {
                        imported = true;
                    }
                }
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultExpr(export)) => {
                let ast::Expr::Call(call) = *export.expr else {
                    return None;
                };
                let ast::Callee::Expr(callee) = call.callee else {
                    return None;
                };
                let ast::Expr::Ident(name) = *callee else {
                    return None;
                };
                if name.sym != *"definePreview"
                    || call.args.len() != 1
                    || call.args[0].spread.is_some()
                    || result.is_some()
                {
                    return None;
                }
                result = value(&call.args[0].expr);
            }
            // Types are erased; any executable statement must go through Node.
            ast::ModuleItem::Stmt(
                ast::Stmt::Decl(ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_))
                | ast::Stmt::Empty(_),
            ) => {}
            _ => return None,
        }
    }
    if imported { result } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn extract(body: &str) -> Option<Value> {
        parse(
            format!(
                "import {{definePreview}} from '@canonic2/workbench'; export default definePreview({body});"
            ),
            false,
        )
    }
    #[test]
    fn accepts_literals_and_erased_wrappers() {
        assert_eq!(
            extract(
                "({a: `text`, b: [-2, true, false, null, {c: 'ok'}]} as const) satisfies Object"
            )
            .unwrap()["a"],
            "text"
        );
    }
    #[test]
    fn computed_or_executable_code_needs_evaluation() {
        for body in [
            "{a: title}",
            "{...other}",
            "{a: () => 1}",
            "{get a(){return 1}}",
            "{a: `x${1}`}",
            "{a: [,...other]}",
            "{[key]: 1}",
            "{__proto__: {}}",
            "{a: /x/}",
            "{a: 1n}",
        ] {
            assert!(extract(body).is_none(), "{body}");
        }
        assert!(
            parse("throw new Error('bad'); export default definePreview({});".into(), false)
                .is_none()
        );
        assert!(
            parse("import './side-effect'; export default definePreview({});".into(), false)
                .is_none()
        );
        assert!(extract("{broken:").is_none());
    }
}
