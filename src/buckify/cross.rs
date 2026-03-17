use starlark_syntax::codemap::Span;
use starlark_syntax::syntax::ast::{ArgumentP, AstExpr, AstNoPayload, AstStmt, ExprP, Stmt};
use starlark_syntax::syntax::module::AstModuleFields;
use starlark_syntax::syntax::{AstModule, Dialect};

const CROSS_SELECT_EXPR: &str =
    "select({\"//platforms:cross\": [\"config//:none\"], \"DEFAULT\": []})";

pub fn patch_rust_test_target_compatible_with(buck_content: String) -> String {
    let ast = match AstModule::parse("BUCK", buck_content.clone(), &Dialect::Extended) {
        Ok(ast) => ast,
        Err(_) => return buck_content,
    };

    let mut insert_positions = Vec::new();
    collect_rust_test_insert_positions(ast.statement(), &mut insert_positions);

    if insert_positions.is_empty() {
        return buck_content;
    }

    let mut out = buck_content;
    insert_positions.sort_unstable();
    for pos in insert_positions.into_iter().rev() {
        if pos >= out.len() {
            continue;
        }
        let insert = build_insert(&out, pos);
        out.insert_str(pos, &insert);
    }

    out
}

fn collect_rust_test_insert_positions(stmt: &AstStmt, out: &mut Vec<usize>) {
    match &stmt.node {
        Stmt::Statements(stmts) => {
            for s in stmts {
                collect_rust_test_insert_positions(s, out);
            }
        }
        Stmt::Expression(expr) => {
            if let Some(pos) = find_rust_test_call(expr) {
                out.push(pos);
            }
        }
        _ => {}
    }
}

fn find_rust_test_call(expr: &AstExpr) -> Option<usize> {
    if let ExprP::Call(callee, args) = &expr.node
        && let ExprP::Identifier(ident) = &callee.node
        && ident.node.ident == "rust_test"
    {
        if call_has_arg(&args.args, "target_compatible_with") {
            return None;
        }
        return insert_pos_before_closing_paren(expr.span);
    }
    None
}

fn call_has_arg(
    args: &[starlark_syntax::codemap::Spanned<ArgumentP<AstNoPayload>>],
    name: &str,
) -> bool {
    args.iter().any(|arg| {
        if let ArgumentP::Named(name_spanned, _) = &arg.node {
            name_spanned.node == name
        } else {
            false
        }
    })
}

fn insert_pos_before_closing_paren(span: Span) -> Option<usize> {
    let end = span.end().get() as usize;
    end.checked_sub(1)
}

fn build_insert(content: &str, insert_pos: usize) -> String {
    let needs_comma = needs_leading_comma(content, insert_pos);
    let mut out = String::new();
    if needs_comma {
        out.push(',');
    }
    out.push('\n');
    out.push_str("    target_compatible_with = ");
    out.push_str(CROSS_SELECT_EXPR);
    out.push_str(",\n");
    out
}

fn needs_leading_comma(content: &str, insert_pos: usize) -> bool {
    for ch in content[..insert_pos].chars().rev() {
        if ch.is_whitespace() {
            continue;
        }
        return ch != ',';
    }
    true
}
