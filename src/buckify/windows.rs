use std::{collections::BTreeSet as Set, vec};

use cargo_metadata::Package;
use starlark_syntax::codemap::{Pos, Span, Spanned};
use starlark_syntax::syntax::ast::{
    ArgumentP, AstExpr, AstLiteral, AstNoPayload, AstStmt, CallArgsP, ExprP, IdentP, Stmt,
};
use starlark_syntax::syntax::module::AstModuleFields;
use starlark_syntax::syntax::{AstModule, Dialect};

use crate::{RUST_CRATES_ROOT, context::BuckalContext};

#[derive(Default)]
struct WindowsImportLibFlags {
    gnu: Vec<String>,
    msvc_x86_64: Vec<String>,
}

pub(super) fn patch_root_windows_rustc_flags(
    mut buck_content: String,
    ctx: &BuckalContext,
    root: &Package,
) -> String {
    let bin_names: Vec<String> = root
        .targets
        .iter()
        .filter(|t| t.kind.contains(&cargo_metadata::TargetKind::Bin))
        .map(|t| t.name.clone())
        .collect();

    let mut rust_test_names: Set<String> = root
        .targets
        .iter()
        .filter(|t| t.kind.contains(&cargo_metadata::TargetKind::Test))
        .map(|t| t.name.clone())
        .collect();

    let lib_targets: Vec<_> = root
        .targets
        .iter()
        .filter(|t| {
            t.kind.contains(&cargo_metadata::TargetKind::Lib)
                || t.kind.contains(&cargo_metadata::TargetKind::CDyLib)
                || t.kind.contains(&cargo_metadata::TargetKind::DyLib)
                || t.kind.contains(&cargo_metadata::TargetKind::RLib)
                || t.kind.contains(&cargo_metadata::TargetKind::StaticLib)
                || t.kind.contains(&cargo_metadata::TargetKind::ProcMacro)
        })
        .collect();

    for lib_target in lib_targets {
        if lib_target.test {
            rust_test_names.insert(format!("{}-unittest", lib_target.name));
        }
    }

    if bin_names.is_empty() && rust_test_names.is_empty() {
        return buck_content;
    }

    let flags = windows_import_lib_flags(ctx);
    let select_expr = render_windows_rustc_flags_select(&flags);
    if select_expr.is_empty() {
        return buck_content;
    }

    for bin_name in bin_names {
        buck_content = apply_rustc_flags_patch_to_content(
            &buck_content,
            "rust_binary",
            &bin_name,
            &select_expr,
        );
    }

    for test_name in rust_test_names {
        buck_content = apply_rustc_flags_patch_to_content(
            &buck_content,
            "rust_test",
            &test_name,
            &select_expr,
        );
    }

    buck_content
}

fn windows_import_lib_flags(ctx: &BuckalContext) -> WindowsImportLibFlags {
    let mut flags = WindowsImportLibFlags::default();

    let push_build_script_rustc_flags = |package_name: &str, out: &mut Vec<String>| {
        let mut matches: Vec<_> = ctx
            .packages_map
            .values()
            .filter(|p| p.name.to_string() == package_name)
            .collect();
        matches.sort_by(|a, b| a.version.cmp(&b.version));
        for package in matches {
            let pkg_name = package.name.to_string();
            out.push(format!(
                "@$(location //{}/{}/{}:{}-build-script-run[rustc_flags])",
                RUST_CRATES_ROOT, pkg_name, package.version, pkg_name
            ));
        }
    };

    // GNU targets.
    push_build_script_rustc_flags("windows_x86_64_gnu", &mut flags.gnu);
    push_build_script_rustc_flags("winapi-x86_64-pc-windows-gnu", &mut flags.gnu);

    // MSVC targets.
    push_build_script_rustc_flags("windows_x86_64_msvc", &mut flags.msvc_x86_64);

    flags
}

fn render_windows_rustc_flags_select(flags: &WindowsImportLibFlags) -> String {
    const CONSTRAINT_WINDOWS: &str = "prelude//os/constraints:windows";
    const CONSTRAINT_ABI_GNU: &str = "prelude//abi/constraints:gnu";
    const SELECT_DEFAULT: &str = "DEFAULT";

    if flags.gnu.is_empty() && flags.msvc_x86_64.is_empty() {
        return String::new();
    }

    let windows_select = build_select(&[
        (CONSTRAINT_ABI_GNU, build_string_list(&flags.gnu)),
        (SELECT_DEFAULT, build_string_list(&flags.msvc_x86_64)),
    ]);

    let select_expr = build_select(&[
        (CONSTRAINT_WINDOWS, windows_select),
        (SELECT_DEFAULT, build_empty_list()),
    ]);

    // Pretty-print the AST with proper indentation
    let mut out = String::new();
    pretty_print_expr(&select_expr, &mut out, 4);
    out
}

/// Create a dummy span for AST nodes (required by starlark_syntax but not used for our purpose)
fn dummy_span() -> Span {
    Span::new(Pos::new(0), Pos::new(0))
}

/// Wrap a value in a Spanned with a dummy span
fn spanned<T>(node: T) -> Spanned<T> {
    Spanned {
        span: dummy_span(),
        node,
    }
}

/// Build a string literal AST node
fn build_string_literal(s: &str) -> AstExpr {
    spanned(ExprP::Literal(AstLiteral::String(spanned(s.to_owned()))))
}

/// Build a list of string literals
fn build_string_list(items: &[String]) -> AstExpr {
    let list_items: Vec<AstExpr> = items.iter().map(|s| build_string_literal(s)).collect();
    spanned(ExprP::List(list_items))
}

/// Build an empty list
fn build_empty_list() -> AstExpr {
    spanned(ExprP::List(vec![]))
}

/// Build a select() call with a dictionary argument
fn build_select(entries: &[(&str, AstExpr)]) -> AstExpr {
    let dict_entries: Vec<(AstExpr, AstExpr)> = entries
        .iter()
        .map(|(k, v)| (build_string_literal(k), v.clone()))
        .collect();

    let dict_expr = spanned(ExprP::Dict(dict_entries));

    let select_ident = spanned(ExprP::Identifier(spanned(IdentP {
        ident: "select".to_owned(),
        payload: (),
    })));

    let args = CallArgsP {
        args: vec![spanned(ArgumentP::Positional(dict_expr))],
    };

    spanned(ExprP::Call(Box::new(select_ident), args))
}

/// Pretty-print an AST expression with proper indentation
fn pretty_print_expr(expr: &AstExpr, out: &mut String, indent: usize) {
    match &expr.node {
        ExprP::Literal(AstLiteral::String(s)) => {
            write_string_literal(out, &s.node);
        }
        ExprP::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
            } else {
                out.push_str("[\n");
                for item in items {
                    write_indent(out, indent + 4);
                    pretty_print_expr(item, out, indent + 4);
                    out.push_str(",\n");
                }
                write_indent(out, indent);
                out.push(']');
            }
        }
        ExprP::Call(callee, args) => {
            // Handle select() calls specially
            if let ExprP::Identifier(ident) = &callee.node
                && ident.node.ident == "select"
            {
                out.push_str("select(");
                if let Some(arg) = args.args.first()
                    && let ArgumentP::Positional(dict_expr) = &arg.node
                {
                    pretty_print_dict(dict_expr, out, indent);
                }
                out.push(')');
                return;
            }
            // Generic call handling (not used in our case)
            out.push_str(&format!("{}", expr.node));
        }
        _ => {
            out.push_str(&format!("{}", expr.node));
        }
    }
}

/// Pretty-print a dictionary expression
fn pretty_print_dict(expr: &AstExpr, out: &mut String, indent: usize) {
    if let ExprP::Dict(entries) = &expr.node {
        out.push_str("{\n");
        for (key, value) in entries {
            write_indent(out, indent + 4);
            pretty_print_expr(key, out, indent + 4);
            out.push_str(": ");
            pretty_print_expr(value, out, indent + 4);
            out.push_str(",\n");
        }
        write_indent(out, indent);
        out.push('}');
    }
}

fn write_indent(out: &mut String, spaces: usize) {
    for _ in 0..spaces {
        out.push(' ');
    }
}

fn write_string_literal(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
}

fn apply_rustc_flags_patch_to_content(
    buck_content: &str,
    rule_name: &str,
    bin_name: &str,
    select_expr: &str,
) -> String {
    // Parse the Starlark content into an AST
    let ast = match AstModule::parse("BUCK", buck_content.to_owned(), &Dialect::Extended) {
        Ok(ast) => ast,
        Err(_) => return buck_content.to_owned(),
    };

    // Find the insertion point by walking the AST
    let insert_pos = match find_rustc_flags_end_in_rule(ast.statement(), rule_name, bin_name) {
        Some(pos) => pos,
        None => return buck_content.to_owned(),
    };

    // Insert the select expression at the found position
    let mut out = String::with_capacity(buck_content.len() + select_expr.len() + 4);
    out.push_str(&buck_content[..insert_pos]);
    out.push_str(" + ");
    out.push_str(select_expr);
    out.push_str(&buck_content[insert_pos..]);
    out
}

/// Walk the AST to find a rust rule call with the given name and return the
/// byte position just after the closing `]` of its `rustc_flags` list.
fn find_rustc_flags_end_in_rule(
    stmt: &AstStmt,
    rule_name: &str,
    target_name: &str,
) -> Option<usize> {
    match &stmt.node {
        Stmt::Statements(stmts) => {
            for s in stmts {
                if let Some(pos) = find_rustc_flags_end_in_rule(s, rule_name, target_name) {
                    return Some(pos);
                }
            }
            None
        }
        Stmt::Expression(expr) => find_in_expr(expr, rule_name, target_name),
        _ => None,
    }
}

fn find_in_expr(expr: &AstExpr, rule_name: &str, target_name: &str) -> Option<usize> {
    if let ExprP::Call(callee, args) = &expr.node {
        // Check if this is a call to a target rule
        if let ExprP::Identifier(ident) = &callee.node
            && ident.node.ident == rule_name
        {
            return find_rustc_flags_in_call(&args.args, target_name);
        }
    }
    None
}

fn find_rustc_flags_in_call(
    args: &[Spanned<ArgumentP<AstNoPayload>>],
    target_name: &str,
) -> Option<usize> {
    // First, check if the `name` argument matches
    let mut name_matches = false;
    let mut rustc_flags_end: Option<usize> = None;

    for arg in args {
        if let ArgumentP::Named(name_spanned, value) = &arg.node {
            let arg_name = &name_spanned.node;
            if arg_name == "name" {
                // Check if the value is a string literal matching our target
                if let ExprP::Literal(AstLiteral::String(s)) = &value.node
                    && s.node == target_name
                {
                    name_matches = true;
                }
            } else if arg_name == "rustc_flags" {
                // Get the end position of the rustc_flags value (should be a list)
                if let ExprP::List(_) = &value.node {
                    rustc_flags_end = Some(value.span.end().get() as usize);
                }
            }
        }
    }

    if name_matches { rustc_flags_end } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;

    #[test]
    fn render_windows_rustc_flags_select_empty() {
        let flags = WindowsImportLibFlags::default();
        assert_eq!(render_windows_rustc_flags_select(&flags), "");
    }

    #[test]
    fn render_windows_rustc_flags_select_structured_output() {
        let flags = WindowsImportLibFlags {
            gnu: vec!["@gnu1".to_owned(), "@gnu2".to_owned()],
            msvc_x86_64: vec!["@msvc64".to_owned()],
        };

        let rendered = render_windows_rustc_flags_select(&flags);

        let expected = indoc! {r#"
            select({
                    "prelude//os/constraints:windows": select({
                        "prelude//abi/constraints:gnu": [
                            "@gnu1",
                            "@gnu2",
                        ],
                        "DEFAULT": [
                            "@msvc64",
                        ],
                    }),
                    "DEFAULT": [],
                })"#};

        assert_eq!(rendered, expected);
    }

    #[test]
    fn apply_rustc_flags_patch_to_content_patches_named_binary_only() {
        let input = indoc! {r#"
            rust_library(
                name = "bin",
                rustc_flags = [
                    "libflag",
                ],
            )

            rust_binary(
                name = "bin",
                rustc_flags = [
                    "binflag",
                ],
            )
            "#};

        let expected = indoc! {r#"
            rust_library(
                name = "bin",
                rustc_flags = [
                    "libflag",
                ],
            )

            rust_binary(
                name = "bin",
                rustc_flags = [
                    "binflag",
                ] + select({"DEFAULT": []}),
            )
            "#};

        let patched = apply_rustc_flags_patch_to_content(
            input,
            "rust_binary",
            "bin",
            "select({\"DEFAULT\": []})",
        );
        assert_eq!(patched, expected);
    }

    #[test]
    fn apply_rustc_flags_patch_to_content_does_not_touch_other_binaries() {
        let input = indoc! {r#"
            rust_binary(
                name = "a",
                rustc_flags = [
                    "aflag",
                ],
            )

            rust_binary(
                name = "b",
                rustc_flags = [
                    "bflag",
                ],
            )
            "#};

        let expected = indoc! {r#"
            rust_binary(
                name = "a",
                rustc_flags = [
                    "aflag",
                ],
            )

            rust_binary(
                name = "b",
                rustc_flags = [
                    "bflag",
                ] + select({"DEFAULT": []}),
            )
            "#};

        let patched = apply_rustc_flags_patch_to_content(
            input,
            "rust_binary",
            "b",
            "select({\"DEFAULT\": []})",
        );
        assert_eq!(patched, expected);
    }

    #[test]
    fn apply_rustc_flags_patch_to_content_patches_named_test_only() {
        let input = indoc! {r#"
            rust_binary(
                name = "bin",
                rustc_flags = [
                    "binflag",
                ],
            )

            rust_test(
                name = "bin-unittest",
                rustc_flags = [
                    "testflag",
                ],
            )
            "#};

        let expected = indoc! {r#"
            rust_binary(
                name = "bin",
                rustc_flags = [
                    "binflag",
                ],
            )

            rust_test(
                name = "bin-unittest",
                rustc_flags = [
                    "testflag",
                ] + select({"DEFAULT": []}),
            )
            "#};

        let patched = apply_rustc_flags_patch_to_content(
            input,
            "rust_test",
            "bin-unittest",
            "select({\"DEFAULT\": []})",
        );
        assert_eq!(patched, expected);
    }
}
