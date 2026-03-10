use std::collections::{BTreeMap as Map, BTreeSet as Set};

use cargo_metadata::camino::Utf8Path;
use serde::ser::{Serialize, SerializeStruct, SerializeTupleStruct, Serializer};
use serde_derive::Serialize;
use starlark_syntax::syntax::ast::{ArgumentP, AstExpr, AstNoPayload, AstStmt, ExprP, Stmt};
use starlark_syntax::syntax::module::AstModuleFields;
use starlark_syntax::syntax::{AstModule, Dialect};

use crate::buckal_error;

#[derive(Serialize, Debug, PartialEq)]
#[serde(untagged)]
pub enum Rule {
    Load(Load),
    HttpArchive(HttpArchive),
    FileGroup(FileGroup),
    GitFetch(GitFetch),
    CargoManifest(CargoManifest),
    RustLibrary(RustLibrary),
    RustBinary(RustBinary),
    RustTest(RustTest),
    BuildscriptRun(BuildscriptRun),
}

impl Rule {
    pub fn as_rust_rule_mut(&mut self) -> Option<&mut dyn RustRule> {
        match self {
            Rule::RustLibrary(inner) => Some(inner),
            Rule::RustBinary(inner) => Some(inner),
            _ => None,
        }
    }
}

pub trait RustRule {
    fn deps_mut(&mut self) -> &mut Set<String>;
    fn os_deps_mut(&mut self) -> &mut Map<String, Set<String>>;
    fn rustc_flags_mut(&mut self) -> &mut Set<String>;
    fn env_mut(&mut self) -> &mut Map<String, String>;
    fn named_deps_mut(&mut self) -> &mut Map<String, String>;
    fn os_named_deps_mut(&mut self) -> &mut Map<String, Map<String, String>>;
}

#[derive(PartialEq, Clone, Copy)]
pub enum CargoTargetKind {
    Lib,
    Bin,
    CustomBuild,
    Test,
}

#[derive(Debug, PartialEq)]
pub struct Load {
    pub bzl: String,
    pub items: Set<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "http_archive")]
pub struct HttpArchive {
    pub name: String,
    pub urls: Set<String>,
    pub sha256: String,
    #[serde(rename = "type")]
    pub _type: String,
    pub strip_prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "git_fetch")]
pub struct GitFetch {
    pub name: String,
    pub repo: String,
    pub rev: String,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "cargo_manifest")]
pub struct CargoManifest {
    pub name: String,
    pub vendor: String,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "rust_library")]
pub struct RustLibrary {
    pub name: String,
    pub srcs: Set<String>,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub crate_root: String,
    pub edition: String,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub target_compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub exec_compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub env: Map<String, String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub features: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub rustc_flags: Set<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proc_macro: Option<bool>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub named_deps: Map<String, String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub os_named_deps: Map<String, Map<String, String>>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub os_deps: Map<String, Set<String>>,
    pub visibility: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub deps: Set<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "rust_binary")]
pub struct RustBinary {
    pub name: String,
    pub srcs: Set<String>,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub crate_root: String,
    pub edition: String,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub target_compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub exec_compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub env: Map<String, String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub features: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub rustc_flags: Set<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub named_deps: Map<String, String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub os_named_deps: Map<String, Map<String, String>>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub os_deps: Map<String, Set<String>>,
    pub visibility: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub deps: Set<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "rust_test")]
pub struct RustTest {
    pub name: String,
    pub srcs: Set<String>,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub crate_root: String,
    pub edition: String,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub target_compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub exec_compatible_with: Set<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub env: Map<String, String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub features: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub rustc_flags: Set<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub named_deps: Map<String, String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub os_named_deps: Map<String, Map<String, String>>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub os_deps: Map<String, Set<String>>,
    pub visibility: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub deps: Set<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "buildscript_run")]
pub struct BuildscriptRun {
    pub name: String,
    pub package_name: String,
    pub buildscript_rule: String,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub env: Map<String, String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub env_srcs: Set<String>,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub features: Set<String>,
    pub version: String,
    pub manifest_dir: String,
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub visibility: Set<String>,
}

#[derive(Default, Debug, PartialEq)]
pub struct Glob {
    pub include: Set<String>,
    pub exclude: Set<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename = "filegroup")]
pub struct FileGroup {
    pub name: String,
    pub srcs: Glob,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
}

impl Serialize for Load {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_tuple_struct("load", 0)?;
        s.serialize_field(&self.bzl)?;
        for item in &self.items {
            s.serialize_field(item)?;
        }
        s.end()
    }
}

impl Serialize for Glob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.exclude.is_empty() {
            serializer.serialize_newtype_struct("glob", &self.include)
        } else {
            let mut s = serializer.serialize_struct("glob", 2)?;
            s.serialize_field("include", &self.include)?;
            s.serialize_field("exclude", &self.exclude)?;
            s.end()
        }
    }
}

impl Glob {
    fn from_ast_expr(expr: &AstExpr) -> anyhow::Result<Self> {
        // Handle glob function call
        if let ExprP::Call(callee, args) = &expr.node
            && let ExprP::Identifier(ident) = &callee.node
            && ident.node.ident == "glob"
        {
            let mut include = Set::new();
            let mut exclude = Set::new();

            for arg in &args.args {
                match &arg.node {
                    ArgumentP::Positional(expr) => {
                        // First positional is include list
                        if let Some(items) = extract_string_list(expr) {
                            include = items;
                        }
                    }
                    ArgumentP::Named(name, expr) => {
                        let items = extract_string_list(expr).unwrap_or_default();
                        match name.node.as_str() {
                            "include" => include = items,
                            "exclude" => exclude = items,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            return Ok(Glob { include, exclude });
        }
        Err(anyhow::anyhow!("Missing or malformed glob call"))
    }
}

// Helper to extract string list from AST expression
fn extract_string_list(expr: &AstExpr) -> Option<Set<String>> {
    // Handle binary operations like: `["item1"] + select({...})` or `select({...}) + ["item2"]`
    // We only extract the literal list part, ignoring dynamic select() regardless of order
    // For `["item1"] + ["item2"]`, we combine both sides into a single set
    if let ExprP::Op(left, _op, right) = &expr.node {
        let left_items = extract_string_list(left);
        let right_items = extract_string_list(right);
        match (left_items, right_items) {
            (Some(mut l), Some(r)) => {
                l.extend(r);
                return Some(l);
            }
            (Some(l), None) => return Some(l),
            (None, Some(r)) => return Some(r),
            (None, None) => return None,
        }
    }

    if let ExprP::List(items) = &expr.node {
        let strings: Set<String> = items
            .iter()
            .filter_map(|item| {
                if let ExprP::Literal(lit) = &item.node {
                    // AstLiteral is an enum, we need to match on it
                    match lit {
                        starlark_syntax::syntax::ast::AstLiteral::String(s) => {
                            Some(s.node.to_string())
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();
        return Some(strings);
    }
    None
}

// Helper to extract string from AST expression
fn extract_string(expr: &AstExpr) -> Option<String> {
    if let ExprP::Literal(lit) = &expr.node {
        match lit {
            starlark_syntax::syntax::ast::AstLiteral::String(s) => Some(s.node.to_string()),
            _ => None,
        }
    } else {
        None
    }
}

// Helper to extract bool from AST expression
fn extract_bool(expr: &AstExpr) -> Option<bool> {
    if let ExprP::Identifier(ident) = &expr.node {
        match ident.node.ident.as_str() {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        }
    } else {
        None
    }
}

// Helper to extract dict from AST expression
fn extract_string_dict(expr: &AstExpr) -> Option<Map<String, String>> {
    if let ExprP::Dict(items) = &expr.node {
        let mut map = Map::new();
        for (key_expr, value_expr) in items {
            if let (Some(key), Some(value)) = (extract_string(key_expr), extract_string(value_expr))
            {
                map.insert(key, value);
            }
        }
        return Some(map);
    }
    None
}

// Helper to extract nested dict (dict of dicts)
fn extract_nested_string_dict(expr: &AstExpr) -> Option<Map<String, Map<String, String>>> {
    if let ExprP::Dict(items) = &expr.node {
        let mut map = Map::new();
        for (key_expr, value_expr) in items {
            if let Some(key) = extract_string(key_expr)
                && let Some(inner_map) = extract_string_dict(value_expr)
            {
                map.insert(key, inner_map);
            }
        }
        return Some(map);
    }
    None
}

// Helper to extract dict of lists
fn extract_dict_of_lists(expr: &AstExpr) -> Option<Map<String, Set<String>>> {
    if let ExprP::Dict(items) = &expr.node {
        let mut map = Map::new();
        for (key_expr, value_expr) in items {
            if let (Some(key), Some(list)) =
                (extract_string(key_expr), extract_string_list(value_expr))
            {
                map.insert(key, list);
            }
        }
        return Some(map);
    }
    None
}

// Structure to hold parsed kwargs from a rule call
struct RuleKwargs {
    args: Map<String, starlark_syntax::codemap::Spanned<ExprP<AstNoPayload>>>,
}

impl RuleKwargs {
    fn from_ast_args(args: &[starlark_syntax::codemap::Spanned<ArgumentP<AstNoPayload>>]) -> Self {
        let mut map = Map::new();
        for arg in args {
            if let ArgumentP::Named(name, expr) = &arg.node {
                map.insert(name.node.clone(), expr.clone());
            }
        }
        RuleKwargs { args: map }
    }

    fn get_str(&self, key: &str) -> anyhow::Result<String> {
        self.args
            .get(key)
            .and_then(extract_string)
            .ok_or_else(|| anyhow::anyhow!("Missing required string argument: {}", key))
    }

    fn get_str_opt(&self, key: &str) -> Option<String> {
        self.args.get(key).and_then(extract_string)
    }

    fn get_bool_opt(&self, key: &str) -> Option<bool> {
        self.args.get(key).and_then(extract_bool)
    }

    fn get_list(&self, key: &str) -> Set<String> {
        self.args
            .get(key)
            .and_then(extract_string_list)
            .unwrap_or_default()
    }

    fn get_dict(&self, key: &str) -> Map<String, String> {
        self.args
            .get(key)
            .and_then(extract_string_dict)
            .unwrap_or_default()
    }

    fn get_nested_dict(&self, key: &str) -> Map<String, Map<String, String>> {
        self.args
            .get(key)
            .and_then(extract_nested_string_dict)
            .unwrap_or_default()
    }

    fn get_dict_of_lists(&self, key: &str) -> Map<String, Set<String>> {
        self.args
            .get(key)
            .and_then(extract_dict_of_lists)
            .unwrap_or_default()
    }

    fn get_glob(&self, key: &str) -> anyhow::Result<Glob> {
        self.args
            .get(key)
            .map(Glob::from_ast_expr)
            .unwrap_or(Err(anyhow::anyhow!(
                "Missing required glob argument: {}",
                key
            )))
    }
}

macro_rules! impl_rust_rule {
    ($ty:ident) => {
        impl RustRule for $ty {
            fn deps_mut(&mut self) -> &mut Set<String> {
                &mut self.deps
            }

            fn os_deps_mut(&mut self) -> &mut Map<String, Set<String>> {
                &mut self.os_deps
            }

            fn rustc_flags_mut(&mut self) -> &mut Set<String> {
                &mut self.rustc_flags
            }

            fn env_mut(&mut self) -> &mut Map<String, String> {
                &mut self.env
            }

            fn named_deps_mut(&mut self) -> &mut Map<String, String> {
                &mut self.named_deps
            }

            fn os_named_deps_mut(&mut self) -> &mut Map<String, Map<String, String>> {
                &mut self.os_named_deps
            }
        }
    };
}

impl_rust_rule!(RustLibrary);
impl_rust_rule!(RustBinary);
impl_rust_rule!(RustTest);

fn patch_map<K, V>(dst: &mut Map<K, V>, src: &Map<K, V>)
where
    K: Clone + Ord,
    V: Clone,
{
    for (k, v) in src {
        dst.entry(k.clone()).or_insert_with(|| v.clone());
    }
}

fn patch_set<T>(dst: &mut Set<T>, src: &Set<T>)
where
    T: Clone + Ord,
{
    let to_add: Vec<_> = src.difference(dst).cloned().collect();
    dst.extend(to_add);
}

struct DepFieldsMut<'a> {
    deps: &'a mut Set<String>,
    os_deps: &'a mut Map<String, Set<String>>,
    named_deps: &'a mut Map<String, String>,
    os_named_deps: &'a mut Map<String, Map<String, String>>,
}

struct DepFieldsRef<'a> {
    deps: &'a Set<String>,
    os_deps: &'a Map<String, Set<String>>,
    named_deps: &'a Map<String, String>,
    os_named_deps: &'a Map<String, Map<String, String>>,
}

fn patch_deps_fields(patch_fields: &Set<String>, dst: &mut DepFieldsMut, src: &DepFieldsRef) {
    if patch_fields.contains("deps") {
        patch_set(dst.deps, src.deps);
    }

    if patch_fields.contains("os_deps") {
        for (plat, deps) in src.os_deps {
            patch_set(dst.os_deps.entry(plat.clone()).or_default(), deps);
        }
    }

    if patch_fields.contains("named_deps") {
        patch_map(dst.named_deps, src.named_deps);
    }

    if patch_fields.contains("os_named_deps") {
        for (alias, plat_map) in src.os_named_deps {
            let entry = dst.os_named_deps.entry(alias.clone()).or_default();
            patch_map(entry, plat_map);
        }
    }
}

macro_rules! impl_patch_from {
    ($ty:ident) => {
        impl $ty {
            fn patch_from(&mut self, other: &Self, patch_fields: &Set<String>) {
                // Patch target_compatible_with set
                if patch_fields.contains("target_compatible_with") {
                    patch_set(
                        &mut self.target_compatible_with,
                        &other.target_compatible_with,
                    );
                }
                // Patch compatible_with set
                if patch_fields.contains("compatible_with") {
                    patch_set(&mut self.compatible_with, &other.compatible_with);
                }
                // Patch exec_compatible_with set
                if patch_fields.contains("exec_compatible_with") {
                    patch_set(&mut self.exec_compatible_with, &other.exec_compatible_with);
                }
                // Patch env map
                if patch_fields.contains("env") {
                    patch_map(&mut self.env, &other.env);
                }
                // Patch features set
                if patch_fields.contains("features") {
                    patch_set(&mut self.features, &other.features);
                }
                // Patch rustc_flags set
                if patch_fields.contains("rustc_flags") {
                    patch_set(&mut self.rustc_flags, &other.rustc_flags);
                }
                // Patch visibility set
                if patch_fields.contains("visibility") {
                    patch_set(&mut self.visibility, &other.visibility);
                }

                let mut dst = DepFieldsMut {
                    deps: &mut self.deps,
                    os_deps: &mut self.os_deps,
                    named_deps: &mut self.named_deps,
                    os_named_deps: &mut self.os_named_deps,
                };
                let src = DepFieldsRef {
                    deps: &other.deps,
                    os_deps: &other.os_deps,
                    named_deps: &other.named_deps,
                    os_named_deps: &other.os_named_deps,
                };
                patch_deps_fields(patch_fields, &mut dst, &src);
            }
        }
    };
}

impl_patch_from!(RustLibrary);
impl_patch_from!(RustBinary);
impl_patch_from!(RustTest);

impl RustLibrary {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let srcs = kwargs.get_list("srcs");
        let crate_name = kwargs.get_str("crate")?;
        let crate_root = kwargs.get_str("crate_root")?;
        let edition = kwargs.get_str("edition")?;
        let target_compatible_with = kwargs.get_list("target_compatible_with");
        let compatible_with = kwargs.get_list("compatible_with");
        let exec_compatible_with = kwargs.get_list("exec_compatible_with");
        let env = kwargs.get_dict("env");
        let features = kwargs.get_list("features");
        let rustc_flags = kwargs.get_list("rustc_flags");
        let proc_macro = kwargs.get_bool_opt("proc_macro");
        let named_deps = kwargs.get_dict("named_deps");
        let os_named_deps = kwargs.get_nested_dict("os_named_deps");
        let os_deps = kwargs.get_dict_of_lists("os_deps");
        let visibility = kwargs.get_list("visibility");
        let deps = kwargs.get_list("deps");
        Ok(RustLibrary {
            name,
            srcs,
            crate_name,
            crate_root,
            edition,
            target_compatible_with,
            compatible_with,
            exec_compatible_with,
            env,
            features,
            rustc_flags,
            proc_macro,
            named_deps,
            os_named_deps,
            os_deps,
            visibility,
            deps,
        })
    }
}

impl RustBinary {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let srcs = kwargs.get_list("srcs");
        let crate_name = kwargs.get_str("crate")?;
        let crate_root = kwargs.get_str("crate_root")?;
        let edition = kwargs.get_str("edition")?;
        let target_compatible_with = kwargs.get_list("target_compatible_with");
        let compatible_with = kwargs.get_list("compatible_with");
        let exec_compatible_with = kwargs.get_list("exec_compatible_with");
        let env = kwargs.get_dict("env");
        let features = kwargs.get_list("features");
        let rustc_flags = kwargs.get_list("rustc_flags");
        let named_deps = kwargs.get_dict("named_deps");
        let os_named_deps = kwargs.get_nested_dict("os_named_deps");
        let os_deps = kwargs.get_dict_of_lists("os_deps");
        let visibility = kwargs.get_list("visibility");
        let deps = kwargs.get_list("deps");
        Ok(RustBinary {
            name,
            srcs,
            crate_name,
            crate_root,
            edition,
            target_compatible_with,
            compatible_with,
            exec_compatible_with,
            env,
            features,
            rustc_flags,
            named_deps,
            os_named_deps,
            os_deps,
            visibility,
            deps,
        })
    }
}

impl RustTest {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let srcs = kwargs.get_list("srcs");
        let crate_name = kwargs.get_str("crate")?;
        let crate_root = kwargs.get_str("crate_root")?;
        let edition = kwargs.get_str("edition")?;
        let target_compatible_with = kwargs.get_list("target_compatible_with");
        let compatible_with = kwargs.get_list("compatible_with");
        let exec_compatible_with = kwargs.get_list("exec_compatible_with");
        let env = kwargs.get_dict("env");
        let features = kwargs.get_list("features");
        let rustc_flags = kwargs.get_list("rustc_flags");
        let named_deps = kwargs.get_dict("named_deps");
        let os_named_deps = kwargs.get_nested_dict("os_named_deps");
        let os_deps = kwargs.get_dict_of_lists("os_deps");
        let visibility = kwargs.get_list("visibility");
        let deps = kwargs.get_list("deps");
        Ok(RustTest {
            name,
            srcs,
            crate_name,
            crate_root,
            edition,
            target_compatible_with,
            compatible_with,
            exec_compatible_with,
            env,
            features,
            rustc_flags,
            named_deps,
            os_named_deps,
            os_deps,
            visibility,
            deps,
        })
    }
}

impl BuildscriptRun {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let package_name = kwargs.get_str("package_name")?;
        let buildscript_rule = kwargs.get_str("buildscript_rule")?;
        let env = kwargs.get_dict("env");
        let env_srcs = kwargs.get_list("env_srcs");
        let features = kwargs.get_list("features");
        let version = kwargs.get_str("version")?;
        let manifest_dir = kwargs.get_str("manifest_dir")?;
        let visibility = kwargs.get_list("visibility");
        Ok(BuildscriptRun {
            name,
            package_name,
            buildscript_rule,
            env,
            env_srcs,
            features,
            version,
            manifest_dir,
            visibility,
        })
    }

    fn patch_from(&mut self, other: &BuildscriptRun, patch_fields: &Set<String>) {
        // Patch env map
        if patch_fields.contains("env") {
            patch_map(&mut self.env, &other.env);
        }
        // Patch features set
        if patch_fields.contains("features") {
            patch_set(&mut self.features, &other.features);
        }
        // Patch visibility set
        if patch_fields.contains("visibility") {
            patch_set(&mut self.visibility, &other.visibility);
        }
    }
}

impl HttpArchive {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let urls = kwargs.get_list("urls");
        let sha256 = kwargs.get_str("sha256")?;
        let _type = kwargs.get_str("type")?;
        let strip_prefix = kwargs.get_str("strip_prefix")?;
        let out = kwargs.get_str_opt("out");
        Ok(HttpArchive {
            name,
            urls,
            sha256,
            _type,
            strip_prefix,
            out,
        })
    }
}

impl GitFetch {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let repo = kwargs.get_str("repo")?;
        let rev = kwargs.get_str("rev")?;
        Ok(GitFetch { name, repo, rev })
    }
}

impl FileGroup {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let srcs = kwargs.get_glob("srcs")?;
        let out = kwargs.get_str_opt("out");
        Ok(FileGroup { name, srcs, out })
    }
}

impl CargoManifest {
    fn from_kwargs(kwargs: &RuleKwargs) -> anyhow::Result<Self> {
        let name = kwargs.get_str("name")?;
        let vendor = kwargs.get_str("vendor")?;
        Ok(CargoManifest { name, vendor })
    }
}

// Parse rules from AST
fn parse_rule_from_call(
    func_name: &str,
    args: &[starlark_syntax::codemap::Spanned<ArgumentP<AstNoPayload>>],
) -> Option<Rule> {
    let kwargs = RuleKwargs::from_ast_args(args);

    match func_name {
        "rust_library" => RustLibrary::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse rust_library: {}", e))
            .ok()
            .map(Rule::RustLibrary),
        "rust_binary" => RustBinary::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse rust_binary: {}", e))
            .ok()
            .map(Rule::RustBinary),
        "rust_test" => RustTest::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse rust_test: {}", e))
            .ok()
            .map(Rule::RustTest),
        "buildscript_run" => BuildscriptRun::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse buildscript_run: {}", e))
            .ok()
            .map(Rule::BuildscriptRun),
        "http_archive" => HttpArchive::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse http_archive: {}", e))
            .ok()
            .map(Rule::HttpArchive),
        "git_fetch" => GitFetch::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse git_fetch: {}", e))
            .ok()
            .map(Rule::GitFetch),
        "filegroup" => FileGroup::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse filegroup: {}", e))
            .ok()
            .map(Rule::FileGroup),
        "cargo_manifest" => CargoManifest::from_kwargs(&kwargs)
            .inspect_err(|e| buckal_error!("failed to parse cargo_manifest: {}", e))
            .ok()
            .map(Rule::CargoManifest),
        _ => None,
    }
}

fn rule_map_key(rule: &Rule) -> String {
    match rule {
        Rule::Load(load) => format!("load[{}]", load.bzl),
        Rule::HttpArchive(r) => format!("http_archive[{}]", r.name),
        Rule::FileGroup(r) => format!("filegroup[{}]", r.name),
        Rule::GitFetch(r) => format!("git_fetch[{}]", r.name),
        Rule::CargoManifest(r) => format!("cargo_manifest[{}]", r.name),
        Rule::RustLibrary(r) => format!("rust_library[{}]", r.name),
        Rule::RustBinary(r) => format!("rust_binary[{}]", r.name),
        Rule::RustTest(r) => format!("rust_test[{}]", r.name),
        Rule::BuildscriptRun(r) => format!("buildscript_run[{}]", r.name),
    }
}

// Walk AST to find rule calls
fn collect_rules(stmt: &AstStmt, rules: &mut Map<String, Rule>) {
    match &stmt.node {
        Stmt::Statements(stmts) => {
            for s in stmts {
                collect_rules(s, rules);
            }
        }
        Stmt::Load(load_stmt) => {
            // load("file.bzl", "symbol1", "symbol2")
            // Extract bzl path and imported symbols from LoadP structure
            let bzl = load_stmt.module.node.to_string();
            let items: Set<String> = load_stmt
                .args
                .iter()
                .map(|load_arg| load_arg.local.node.ident.clone())
                .collect();

            let load_rule = Load { bzl, items };
            let rule = Rule::Load(load_rule);
            rules.insert(rule_map_key(&rule), rule);
        }
        Stmt::Expression(expr) => {
            if let ExprP::Call(callee, args) = &expr.node
                && let ExprP::Identifier(ident) = &callee.node
            {
                let func_name = ident.node.ident.as_str();
                if let Some(rule) = parse_rule_from_call(func_name, &args.args) {
                    rules.insert(rule_map_key(&rule), rule);
                }
            }
        }
        _ => {}
    }
}

/// Parse a BUCK file and extract rules into a map keyed by `rule_type[rule_name]` for easy lookup.
pub fn parse_buck_file<T: AsRef<Utf8Path>>(file: T) -> anyhow::Result<Map<String, Rule>> {
    let buck_content = std::fs::read_to_string(file.as_ref())?;
    let ast = AstModule::parse(file.as_ref().as_str(), buck_content, &Dialect::Extended)
        .map_err(|e| anyhow::anyhow!("Failed to parse BUCK file: {}", e))?;

    let mut buck_rules: Map<String, Rule> = Map::new();
    collect_rules(ast.statement(), &mut buck_rules);

    Ok(buck_rules)
}

pub fn patch_buck_rules(
    existing: &Map<String, Rule>,
    to_patch: &mut [Rule],
    patch_fields: &Set<String>,
) {
    fn find_existing<'a>(existing: &'a Map<String, Rule>, rule: &Rule) -> Option<&'a Rule> {
        existing.get(&rule_map_key(rule))
    }

    for rule in to_patch.iter_mut() {
        match rule {
            Rule::RustLibrary(new_rule) => {
                let probe = Rule::RustLibrary(RustLibrary {
                    name: new_rule.name.clone(),
                    ..RustLibrary::default()
                });
                if let Some(Rule::RustLibrary(existing_rule)) = find_existing(existing, &probe) {
                    new_rule.patch_from(existing_rule, patch_fields);
                }
            }
            Rule::RustBinary(new_rule) => {
                let probe = Rule::RustBinary(RustBinary {
                    name: new_rule.name.clone(),
                    ..RustBinary::default()
                });
                if let Some(Rule::RustBinary(existing_rule)) = find_existing(existing, &probe) {
                    new_rule.patch_from(existing_rule, patch_fields);
                }
            }
            Rule::RustTest(new_rule) => {
                let probe = Rule::RustTest(RustTest {
                    name: new_rule.name.clone(),
                    ..RustTest::default()
                });
                if let Some(Rule::RustTest(existing_rule)) = find_existing(existing, &probe) {
                    new_rule.patch_from(existing_rule, patch_fields);
                }
            }
            Rule::BuildscriptRun(new_rule) => {
                let probe = Rule::BuildscriptRun(BuildscriptRun {
                    name: new_rule.name.clone(),
                    ..BuildscriptRun::default()
                });
                if let Some(Rule::BuildscriptRun(existing_rule)) = find_existing(existing, &probe) {
                    new_rule.patch_from(existing_rule, patch_fields);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cargo_metadata::camino::Utf8PathBuf;

    fn get_test_file(file_name: &str) -> Utf8PathBuf {
        let buck_file =
            Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("testcases/{}", file_name));
        assert!(
            buck_file.exists(),
            "Test BUCK file does not exist: {}",
            buck_file
        );
        buck_file
    }

    /// Test parsing a BUCK file with rules for a workspace member crate.
    #[test]
    fn test_parsing_workspace_member() {
        let rules =
            parse_buck_file(get_test_file("workspace_member.BUCK")).expect("parse should succeed");

        fn common_named_deps() -> Map<String, String> {
            Map::from([(
                "rig".to_string(),
                "//third-party/rust/crates/rig-core/0.30.0:rig-core".to_string(),
            )])
        }

        fn common_os_deps() -> Map<String, Set<String>> {
            Map::from([
                (
                    "linux".to_string(),
                    Set::from(["//third-party/rust/crates/pager/0.16.1:pager".to_string()]),
                ),
                (
                    "macos".to_string(),
                    Set::from(["//third-party/rust/crates/pager/0.16.1:pager".to_string()]),
                ),
            ])
        }

        fn common_base_deps() -> Set<String> {
            Set::from([
                "//third-party/rust/crates/anyhow/1.0.102:anyhow".to_string(),
                "//third-party/rust/crates/async-stream/0.3.6:async-stream".to_string(),
                "//third-party/rust/crates/async-trait/0.1.89:async-trait".to_string(),
                "//third-party/rust/crates/axum/0.8.8:axum".to_string(),
                "//third-party/rust/crates/byte-unit/5.2.0:byte-unit".to_string(),
                "//third-party/rust/crates/byteorder/1.5.0:byteorder".to_string(),
                "//third-party/rust/crates/bytes/1.11.1:bytes".to_string(),
                "//third-party/rust/crates/chrono/0.4.43:chrono".to_string(),
                "//third-party/rust/crates/clap/4.5.60:clap".to_string(),
                "//third-party/rust/crates/colored/3.1.1:colored".to_string(),
                "//third-party/rust/crates/crc32fast/1.5.0:crc32fast".to_string(),
                "//third-party/rust/crates/crossterm/0.28.1:crossterm".to_string(),
                "//third-party/rust/crates/dagrs/0.6.0:dagrs".to_string(),
                "//third-party/rust/crates/diffs/0.5.1:diffs".to_string(),
                "//third-party/rust/crates/diffy/0.4.2:diffy".to_string(),
                "//third-party/rust/crates/dirs/5.0.1:dirs".to_string(),
                "//third-party/rust/crates/flate2/1.1.9:flate2".to_string(),
                "//third-party/rust/crates/futures-core/0.3.32:futures-core".to_string(),
                "//third-party/rust/crates/futures-util/0.3.32:futures-util".to_string(),
                "//third-party/rust/crates/futures/0.3.32:futures".to_string(),
                "//third-party/rust/crates/git-internal/0.7.0:git-internal".to_string(),
                "//third-party/rust/crates/hex/0.4.3:hex".to_string(),
                "//third-party/rust/crates/http/1.4.0:http".to_string(),
                "//third-party/rust/crates/hyper-util/0.1.20:hyper-util".to_string(),
                "//third-party/rust/crates/ignore/0.4.25:ignore".to_string(),
                "//third-party/rust/crates/indicatif/0.18.4:indicatif".to_string(),
                "//third-party/rust/crates/infer/0.19.0:infer".to_string(),
                "//third-party/rust/crates/lazy_static/1.5.0:lazy_static".to_string(),
                "//third-party/rust/crates/lru-mem/0.3.0:lru-mem".to_string(),
                "//third-party/rust/crates/object_store/0.12.5:object_store".to_string(),
                "//third-party/rust/crates/once_cell/1.21.3:once_cell".to_string(),
                "//third-party/rust/crates/openssl/0.10.75:openssl".to_string(),
                "//third-party/rust/crates/path-absolutize/3.1.1:path-absolutize".to_string(),
                "//third-party/rust/crates/pathdiff/0.2.3:pathdiff".to_string(),
                "//third-party/rust/crates/pulldown-cmark/0.12.2:pulldown-cmark".to_string(),
                "//third-party/rust/crates/ratatui/0.29.0:ratatui".to_string(),
                "//third-party/rust/crates/regex/1.12.3:regex".to_string(),
                "//third-party/rust/crates/reqwest/0.12.28:reqwest".to_string(),
                "//third-party/rust/crates/ring/0.17.14:ring".to_string(),
                "//third-party/rust/crates/rmcp/0.13.0:rmcp".to_string(),
                "//third-party/rust/crates/rpassword/7.4.0:rpassword".to_string(),
                "//third-party/rust/crates/scopeguard/1.2.0:scopeguard".to_string(),
                "//third-party/rust/crates/sea-orm/1.1.19:sea-orm".to_string(),
                "//third-party/rust/crates/serde/1.0.228:serde".to_string(),
                "//third-party/rust/crates/serde_json/1.0.149:serde_json".to_string(),
                "//third-party/rust/crates/sha1/0.10.6:sha1".to_string(),
                "//third-party/rust/crates/similar/2.7.0:similar".to_string(),
                "//third-party/rust/crates/thiserror/2.0.18:thiserror".to_string(),
                "//third-party/rust/crates/tokio-stream/0.1.18:tokio-stream".to_string(),
                "//third-party/rust/crates/tokio-util/0.7.18:tokio-util".to_string(),
                "//third-party/rust/crates/tokio/1.49.0:tokio".to_string(),
                "//third-party/rust/crates/tower-http/0.6.8:tower-http".to_string(),
                "//third-party/rust/crates/tower/0.5.3:tower".to_string(),
                "//third-party/rust/crates/tracing-subscriber/0.3.22:tracing-subscriber"
                    .to_string(),
                "//third-party/rust/crates/tracing/0.1.44:tracing".to_string(),
                "//third-party/rust/crates/unicode-width/0.2.0:unicode-width".to_string(),
                "//third-party/rust/crates/url/2.5.8:url".to_string(),
                "//third-party/rust/crates/uuid/1.21.0:uuid".to_string(),
                "//third-party/rust/crates/walkdir/2.5.0:walkdir".to_string(),
                "//third-party/rust/crates/wax/0.6.0:wax".to_string(),
            ])
        }

        fn test_only_deps() -> Set<String> {
            Set::from([
                "//third-party/rust/crates/assert_cmd/2.1.2:assert_cmd".to_string(),
                "//third-party/rust/crates/serial_test/3.3.1:serial_test".to_string(),
                "//third-party/rust/crates/tempfile/3.25.0:tempfile".to_string(),
                "//third-party/rust/crates/testcontainers/0.25.2:testcontainers".to_string(),
            ])
        }

        fn common_test_deps() -> Set<String> {
            let mut deps = common_base_deps();
            deps.extend(test_only_deps());
            deps
        }

        fn common_test_env() -> Map<String, String> {
            Map::from([(
                "CARGO_BIN_EXE_libra".to_string(),
                "$(location :libra)".to_string(),
            )])
        }

        let mut binary_deps = common_base_deps();
        binary_deps.insert(":libra-lib".to_string());

        let mut test_with_bin_deps = common_test_deps();
        test_with_bin_deps.insert(":libra-lib".to_string());

        let expected_rules = vec![
            Rule::Load(Load {
                bzl: "@buckal//:cargo_manifest.bzl".to_string(),
                items: Set::from(["cargo_manifest".to_string()]),
            }),
            Rule::Load(Load {
                bzl: "@buckal//:wrapper.bzl".to_string(),
                items: Set::from([
                    "rust_binary".to_string(),
                    "rust_library".to_string(),
                    "rust_test".to_string(),
                ]),
            }),
            Rule::FileGroup(FileGroup {
                name: "vendor".to_string(),
                srcs: Glob {
                    include: Set::from(["**/**".to_string()]),
                    exclude: Set::new(),
                },
                out: None,
            }),
            Rule::CargoManifest(CargoManifest {
                name: "manifest".to_string(),
                vendor: ":vendor".to_string(),
            }),
            Rule::RustBinary(RustBinary {
                name: "libra".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "libra".to_string(),
                crate_root: "vendor/src/main.rs".to_string(),
                edition: "2024".to_string(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: binary_deps,
                ..Default::default()
            }),
            Rule::RustLibrary(RustLibrary {
                name: "libra-lib".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "libra".to_string(),
                crate_root: "vendor/src/lib.rs".to_string(),
                edition: "2024".to_string(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: common_base_deps(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "unittest".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "libra".to_string(),
                crate_root: "vendor/src/lib.rs".to_string(),
                edition: "2024".to_string(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: common_test_deps(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "ai_agent_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "ai_agent_test".to_string(),
                crate_root: "vendor/tests/ai_agent_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "ai_chat_agent_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "ai_chat_agent_test".to_string(),
                crate_root: "vendor/tests/ai_chat_agent_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "ai_dag_tool_loop_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "ai_dag_tool_loop_test".to_string(),
                crate_root: "vendor/tests/ai_dag_tool_loop_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "ai_storage_flow_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "ai_storage_flow_test".to_string(),
                crate_root: "vendor/tests/ai_storage_flow_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "cloud_storage_backup_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "cloud_storage_backup_test".to_string(),
                crate_root: "vendor/tests/cloud_storage_backup_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "command_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "command_test".to_string(),
                crate_root: "vendor/tests/command_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "e2e_mcp_flow".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "e2e_mcp_flow".to_string(),
                crate_root: "vendor/tests/e2e_mcp_flow.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "intent_flow_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "intent_flow_test".to_string(),
                crate_root: "vendor/tests/intent_flow_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "mcp_integration_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "mcp_integration_test".to_string(),
                crate_root: "vendor/tests/mcp_integration_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps.clone(),
                ..Default::default()
            }),
            Rule::RustTest(RustTest {
                name: "storage_r2_test".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "storage_r2_test".to_string(),
                crate_root: "vendor/tests/storage_r2_test.rs".to_string(),
                edition: "2024".to_string(),
                env: common_test_env(),
                features: Set::from(["default".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                named_deps: common_named_deps(),
                os_deps: common_os_deps(),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: test_with_bin_deps,
                ..Default::default()
            }),
        ];

        assert_eq!(
            rules.len(),
            expected_rules.len(),
            "workspace BUCK should produce expected rule count"
        );

        for expected in expected_rules {
            let key = rule_map_key(&expected);
            let actual = rules
                .get(&key)
                .unwrap_or_else(|| panic!("rule with key '{}' should be present", key));
            assert_eq!(
                actual, &expected,
                "rule with key '{}' should match expected",
                key
            );
        }
    }

    /// Test parsing a BUCK file with rules for a third-party crate from a registry.
    #[test]
    fn test_parsing_registry_crate() {
        let rules =
            parse_buck_file(get_test_file("registry_crate.BUCK")).expect("parse should succeed");

        let expected_rules = vec![
            Rule::Load(Load {
                bzl: "@buckal//:cargo_manifest.bzl".to_string(),
                items: Set::from(["cargo_manifest".to_string()]),
            }),
            Rule::Load(Load {
                bzl: "@buckal//:wrapper.bzl".to_string(),
                items: Set::from([
                    "buildscript_run".to_string(),
                    "rust_binary".to_string(),
                    "rust_library".to_string(),
                ]),
            }),
            Rule::HttpArchive(HttpArchive {
                name: "vendor".to_string(),
                urls: Set::from([
                    "https://static.crates.io/crates/aws-lc-rs/aws-lc-rs-1.15.4.crate".to_string(),
                ]),
                sha256: "7b7b6141e96a8c160799cc2d5adecd5cbbe5054cb8c7c4af53da0f83bb7ad256"
                    .to_string(),
                _type: "tar.gz".to_string(),
                strip_prefix: "aws-lc-rs-1.15.4".to_string(),
                ..Default::default()
            }),
            Rule::CargoManifest(CargoManifest {
                name: "manifest".to_string(),
                vendor: ":vendor".to_string(),
            }),
            Rule::RustLibrary(RustLibrary {
                name: "aws-lc-rs".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "aws_lc_rs".to_string(),
                crate_root: "vendor/src/lib.rs".to_string(),
                edition: "2021".to_string(),
                env: Map::from([(
                    "OUT_DIR".to_string(),
                    "$(location :build-script-run[out_dir])".to_string(),
                )]),
                features: Set::from([
                    "alloc".to_string(),
                    "aws-lc-sys".to_string(),
                    "default".to_string(),
                    "prebuilt-nasm".to_string(),
                    "ring-io".to_string(),
                    "ring-sig-verify".to_string(),
                ]),
                rustc_flags: Set::from([
                    "@$(location :build-script-run[rustc_flags])".to_string(),
                    "@$(location :manifest[env_flags])".to_string(),
                ]),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: Set::from([
                    "//third-party/rust/crates/aws-lc-sys/0.37.1:aws-lc-sys".to_string(),
                    "//third-party/rust/crates/untrusted/0.7.1:untrusted".to_string(),
                    "//third-party/rust/crates/zeroize/1.8.2:zeroize".to_string(),
                ]),
                ..Default::default()
            }),
            Rule::RustBinary(RustBinary {
                name: "build-script-build".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "build_script_build".to_string(),
                crate_root: "vendor/build.rs".to_string(),
                edition: "2021".to_string(),
                features: Set::from([
                    "alloc".to_string(),
                    "aws-lc-sys".to_string(),
                    "default".to_string(),
                    "prebuilt-nasm".to_string(),
                    "ring-io".to_string(),
                    "ring-sig-verify".to_string(),
                ]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                ..Default::default()
            }),
            Rule::BuildscriptRun(BuildscriptRun {
                name: "build-script-run".to_string(),
                package_name: "aws-lc-rs".to_string(),
                buildscript_rule: ":build-script-build".to_string(),
                env_srcs: Set::from([
                    "//third-party/rust/crates/aws-lc-sys/0.37.1:build-script-main-run[metadata]"
                        .to_string(),
                    ":manifest[env_dict]".to_string(),
                ]),
                features: Set::from([
                    "alloc".to_string(),
                    "aws-lc-sys".to_string(),
                    "default".to_string(),
                    "prebuilt-nasm".to_string(),
                    "ring-io".to_string(),
                    "ring-sig-verify".to_string(),
                ]),
                version: "1.15.4".to_string(),
                manifest_dir: ":vendor".to_string(),
                visibility: Set::from(["PUBLIC".to_string()]),
                ..Default::default()
            }),
        ];

        assert_eq!(
            rules.len(),
            expected_rules.len(),
            "workspace BUCK should produce expected rule count"
        );

        for expected in expected_rules {
            let key = rule_map_key(&expected);
            let actual = rules
                .get(&key)
                .unwrap_or_else(|| panic!("rule with key '{}' should be present", key));
            assert_eq!(
                actual, &expected,
                "rule with key '{}' should match expected",
                key
            );
        }
    }

    /// Test parsing a BUCK file with rules for a third-party crate from a git repository.
    #[test]
    fn test_parsing_git_repo() {
        let rules = parse_buck_file(get_test_file("git_repo.BUCK")).expect("parse should succeed");

        let expected_rules = vec![
            Rule::Load(Load {
                bzl: "@buckal//:cargo_manifest.bzl".to_string(),
                items: Set::from(["cargo_manifest".to_string()]),
            }),
            Rule::Load(Load {
                bzl: "@buckal//:wrapper.bzl".to_string(),
                items: Set::from(["rust_library".to_string()]),
            }),
            Rule::GitFetch(GitFetch {
                name: "vendor".to_string(),
                repo: "https://github.com/web3infra-foundation/git-internal.git".to_string(),
                rev: "65b910d7571f36aa231958992e005a7a1c0838e9".to_string(),
            }),
            Rule::CargoManifest(CargoManifest {
                name: "manifest".to_string(),
                vendor: ":vendor".to_string(),
            }),
            Rule::RustLibrary(RustLibrary {
                name: "git-internal".to_string(),
                srcs: Set::from([":vendor".to_string()]),
                crate_name: "git_internal".to_string(),
                crate_root: "vendor/src/lib.rs".to_string(),
                edition: "2024".to_string(),
                features: Set::from(["default".to_string(), "diff_mydrs".to_string()]),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                visibility: Set::from(["PUBLIC".to_string()]),
                deps: Set::from([
                    "//third-party/rust/crates/ahash/0.8.12:ahash".to_string(),
                    "//third-party/rust/crates/async-trait/0.1.89:async-trait".to_string(),
                    "//third-party/rust/crates/axum/0.8.8:axum".to_string(),
                    "//third-party/rust/crates/bincode/2.0.1:bincode".to_string(),
                    "//third-party/rust/crates/bstr/1.12.1:bstr".to_string(),
                    "//third-party/rust/crates/byteorder/1.5.0:byteorder".to_string(),
                    "//third-party/rust/crates/bytes/1.11.1:bytes".to_string(),
                    "//third-party/rust/crates/chrono/0.4.44:chrono".to_string(),
                    "//third-party/rust/crates/colored/3.1.1:colored".to_string(),
                    "//third-party/rust/crates/crc32fast/1.5.0:crc32fast".to_string(),
                    "//third-party/rust/crates/dashmap/6.1.0:dashmap".to_string(),
                    "//third-party/rust/crates/diffs/0.5.1:diffs".to_string(),
                    "//third-party/rust/crates/encoding_rs/0.8.35:encoding_rs".to_string(),
                    "//third-party/rust/crates/flate2/1.1.9:flate2".to_string(),
                    "//third-party/rust/crates/futures-util/0.3.32:futures-util".to_string(),
                    "//third-party/rust/crates/futures/0.3.32:futures".to_string(),
                    "//third-party/rust/crates/hex/0.4.3:hex".to_string(),
                    "//third-party/rust/crates/libc/0.2.182:libc".to_string(),
                    "//third-party/rust/crates/lru-mem/0.3.0:lru-mem".to_string(),
                    "//third-party/rust/crates/memchr/2.8.0:memchr".to_string(),
                    "//third-party/rust/crates/natord/1.0.9:natord".to_string(),
                    "//third-party/rust/crates/num_cpus/1.17.0:num_cpus".to_string(),
                    "//third-party/rust/crates/path-absolutize/3.1.1:path-absolutize".to_string(),
                    "//third-party/rust/crates/rayon/1.11.0:rayon".to_string(),
                    "//third-party/rust/crates/ring/0.17.14:ring".to_string(),
                    "//third-party/rust/crates/sea-orm/1.1.19:sea-orm".to_string(),
                    "//third-party/rust/crates/serde/1.0.228:serde".to_string(),
                    "//third-party/rust/crates/serde_json/1.0.149:serde_json".to_string(),
                    "//third-party/rust/crates/sha1/0.10.6:sha1".to_string(),
                    "//third-party/rust/crates/sha2/0.10.9:sha2".to_string(),
                    "//third-party/rust/crates/similar/2.7.0:similar".to_string(),
                    "//third-party/rust/crates/tempfile/3.26.0:tempfile".to_string(),
                    "//third-party/rust/crates/thiserror/2.0.18:thiserror".to_string(),
                    "//third-party/rust/crates/threadpool/1.8.1:threadpool".to_string(),
                    "//third-party/rust/crates/tokio-stream/0.1.18:tokio-stream".to_string(),
                    "//third-party/rust/crates/tokio/1.50.0:tokio".to_string(),
                    "//third-party/rust/crates/tracing-subscriber/0.3.22:tracing-subscriber"
                        .to_string(),
                    "//third-party/rust/crates/tracing/0.1.44:tracing".to_string(),
                    "//third-party/rust/crates/uuid/1.21.0:uuid".to_string(),
                    "//third-party/rust/crates/zstd-sys/2.0.16+zstd.1.5.7:zstd-sys".to_string(),
                ]),
                ..Default::default()
            }),
        ];

        assert_eq!(
            rules.len(),
            expected_rules.len(),
            "workspace BUCK should produce expected rule count"
        );

        for expected in expected_rules {
            let key = rule_map_key(&expected);
            let actual = rules
                .get(&key)
                .unwrap_or_else(|| panic!("rule with key '{}' should be present", key));
            assert_eq!(
                actual, &expected,
                "rule with key '{}' should match expected",
                key
            );
        }
    }

    /// Test parsing a BUCK file with a single `load` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_load() {
        let rules =
            parse_buck_file(get_test_file("single_load.BUCK")).expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::Load(Load {
            bzl: "@prelude//rust:defs.bzl".to_string(),
            items: Set::from([
                "rust_library".to_string(),
                "rust_binary".to_string(),
                "rust_test".to_string(),
            ]),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("load rule should be present");
        assert_eq!(actual, &expected, "parsed load rule should match expected");
    }

    /// Test parsing a BUCK file with a `http_archive` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_http_archive() {
        let rules = parse_buck_file(get_test_file("single_http_archive.BUCK"))
            .expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::HttpArchive(HttpArchive {
            name: "example_archive".to_string(),
            urls: Set::from(["https://example.com/archive.tar.gz".to_string()]),
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            _type: "tar.gz".to_string(),
            strip_prefix: "archive".to_string(),
            out: Some("example_out".to_string()),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("http_archive rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed http_archive rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `filegroup` rule that includes a `glob` with both `include` and `exclude` patterns.
    #[test]
    fn test_parsing_single_file_group() {
        let rules =
            parse_buck_file(get_test_file("single_file_group.BUCK")).expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::FileGroup(FileGroup {
            name: "example_files".to_string(),
            srcs: Glob {
                include: Set::from([
                    "file1.txt".to_string(),
                    "file2.txt".to_string(),
                    "examples/**".to_string(),
                ]),
                exclude: Set::from(["examples/exclude/**".to_string()]),
            },
            out: Some("example_out".to_string()),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("filegroup rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed filegroup rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `git_fetch` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_git_fetch() {
        let rules =
            parse_buck_file(get_test_file("single_git_fetch.BUCK")).expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::GitFetch(GitFetch {
            name: "example_repo".to_string(),
            repo: "https://example.com/repo.git".to_string(),
            rev: "abcdef1234567890".to_string(),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("git_fetch rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed git_fetch rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `cargo_manifest` rule that includes all possible fields.
    #[test]
    fn test_buck_parser_single_cargo_manifest() {
        let rules = parse_buck_file(get_test_file("single_cargo_manifest.BUCK"))
            .expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::CargoManifest(CargoManifest {
            name: "example_manifest".to_string(),
            vendor: ":vendor".to_string(),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("cargo_manifest rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed cargo_manifest rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `rust_library` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_rust_library() {
        let rules = parse_buck_file(get_test_file("single_rust_library.BUCK"))
            .expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::RustLibrary(RustLibrary {
            name: "example_lib".to_string(),
            srcs: Set::from(["src/lib.rs".to_string()]),
            crate_name: "example_lib".to_string(),
            crate_root: "src/lib.rs".to_string(),
            edition: "2024".to_string(),
            target_compatible_with: Set::from(["prelude//os/constraints:windows".to_string()]),
            compatible_with: Set::from(["prelude//os/constraints:linux".to_string()]),
            exec_compatible_with: Set::from(["prelude//os/constraints:macos".to_string()]),
            env: Map::from([("RUST_LOG".to_string(), "debug".to_string())]),
            features: Set::from(["default".to_string()]),
            rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
            proc_macro: Some(true),
            named_deps: Map::from([("serde".to_string(), ":serde_dep".to_string())]),
            os_named_deps: Map::from([(
                "win_dep".to_string(),
                Map::from([("windows".to_string(), ":windows_dep".to_string())]),
            )]),
            os_deps: Map::from([("linux".to_string(), Set::from([":linux_dep".to_string()]))]),
            visibility: Set::from(["PUBLIC".to_string()]),
            deps: Set::from([":dep".to_string()]),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("rust_library rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed rust_library rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `rust_binary` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_rust_binary() {
        let rules = parse_buck_file(get_test_file("single_rust_binary.BUCK"))
            .expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::RustBinary(RustBinary {
            name: "example_bin".to_string(),
            srcs: Set::from(["src/main.rs".to_string()]),
            crate_name: "example_bin".to_string(),
            crate_root: "src/main.rs".to_string(),
            edition: "2024".to_string(),
            target_compatible_with: Set::from(["prelude//os/constraints:windows".to_string()]),
            compatible_with: Set::from(["prelude//os/constraints:linux".to_string()]),
            exec_compatible_with: Set::from(["prelude//os/constraints:macos".to_string()]),
            env: Map::from([("RUST_LOG".to_string(), "debug".to_string())]),
            features: Set::from(["default".to_string()]),
            rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
            deps: Set::from([":dep".to_string()]),
            os_deps: Map::from([("linux".to_string(), Set::from([":linux_dep".to_string()]))]),
            named_deps: Map::from([("serde".to_string(), ":serde_dep".to_string())]),
            os_named_deps: Map::from([(
                "win_dep".to_string(),
                Map::from([("windows".to_string(), ":windows_dep".to_string())]),
            )]),
            visibility: Set::from(["PUBLIC".to_string()]),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("rust_binary rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed rust_binary rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `rust_test` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_rust_test() {
        let rules =
            parse_buck_file(get_test_file("single_rust_test.BUCK")).expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::RustTest(RustTest {
            name: "example_test".to_string(),
            srcs: Set::from(["src/lib.rs".to_string()]),
            crate_name: "example_test".to_string(),
            crate_root: "src/lib.rs".to_string(),
            edition: "2024".to_string(),
            target_compatible_with: Set::from(["prelude//os/constraints:windows".to_string()]),
            compatible_with: Set::from(["prelude//os/constraints:linux".to_string()]),
            exec_compatible_with: Set::from(["prelude//os/constraints:macos".to_string()]),
            env: Map::from([("RUST_LOG".to_string(), "debug".to_string())]),
            features: Set::from(["default".to_string()]),
            rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
            deps: Set::from([":dep".to_string()]),
            os_deps: Map::from([("linux".to_string(), Set::from([":linux_dep".to_string()]))]),
            named_deps: Map::from([("serde".to_string(), ":serde_dep".to_string())]),
            os_named_deps: Map::from([(
                "win_dep".to_string(),
                Map::from([("windows".to_string(), ":windows_dep".to_string())]),
            )]),
            visibility: Set::from(["PUBLIC".to_string()]),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("rust_test rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed rust_test rule should match expected"
        );
    }

    /// Test parsing a BUCK file with a `buildscript_run` rule that includes all possible fields.
    #[test]
    fn test_parsing_single_build_script_run() {
        let rules = parse_buck_file(get_test_file("single_build_script_run.BUCK"))
            .expect("parse should succeed");
        assert_eq!(rules.len(), 1);
        let expected = Rule::BuildscriptRun(BuildscriptRun {
            name: "demo-build-script-run".to_string(),
            package_name: "demo".to_string(),
            buildscript_rule: ":demo-build-script-build".to_string(),
            env: Map::from([("RUST_LOG".to_string(), "debug".to_string())]),
            env_srcs: Set::from([
                "//path/to/example:example-build-script-main-run[metadata]".to_string(),
                ":manifest[env_dict]".to_string(),
            ]),
            features: Set::from(["alloc".to_string(), "default".to_string()]),
            version: "1.2.3".to_string(),
            manifest_dir: ":vendor".to_string(),
            visibility: Set::from(["PUBLIC".to_string()]),
        });
        let actual = rules
            .get(&rule_map_key(&expected))
            .expect("buildscript_run rule should be present");
        assert_eq!(
            actual, &expected,
            "parsed buildscript_run rule should match expected"
        );
    }

    /// Test parsing a BUCK file with invalid Starlark syntax to ensure we get a proper error message.
    #[test]
    fn test_parsing_invalid_starlark() {
        let err =
            parse_buck_file(get_test_file("invalid_starlark.BUCK")).expect_err("parse should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("Failed to parse BUCK file"),
            "unexpected error message: {msg}"
        );
    }

    /// Test parsing a BUCK file with a rule that is missing required fields to ensure it is skipped and does not cause a parse failure.
    #[test]
    fn test_parsing_required_fields_missing() {
        let rules = parse_buck_file(get_test_file("required_fields_missing.BUCK"))
            .expect("parse should succeed");
        assert!(
            !rules.contains_key("rust_library[incomplete]"),
            "rule with missing required args should be skipped"
        );
    }

    /// Test parsing a BUCK file with `rust_library` rules that has a `rustc_flags` field containing a dynamic `select()` expression, to ensure we can still extract any literal values.
    #[test]
    fn test_parsing_literal_with_dynamic_select() {
        let rules = parse_buck_file(get_test_file("literal_with_dynamic_select.BUCK"))
            .expect("parse should succeed");

        let expected_rules = vec![
            Rule::RustLibrary(RustLibrary {
                name: "with_select".to_string(),
                srcs: Set::from(["src/lib.rs".to_string()]),
                crate_name: "with_select".to_string(),
                crate_root: "src/lib.rs".to_string(),
                edition: "2024".to_string(),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                visibility: Set::from(["PUBLIC".to_string()]),
                ..Default::default()
            }),
            Rule::RustLibrary(RustLibrary {
                name: "with_select_reversed".to_string(),
                srcs: Set::from(["src/lib.rs".to_string()]),
                crate_name: "with_select_reversed".to_string(),
                crate_root: "src/lib.rs".to_string(),
                edition: "2024".to_string(),
                rustc_flags: Set::from(["@$(location :manifest[env_flags])".to_string()]),
                visibility: Set::from(["PUBLIC".to_string()]),
                ..Default::default()
            }),
            Rule::RustLibrary(RustLibrary {
                name: "merge_both_sides".to_string(),
                srcs: Set::from(["src/lib.rs".to_string()]),
                crate_name: "merge_both_sides".to_string(),
                crate_root: "src/lib.rs".to_string(),
                edition: "2024".to_string(),
                rustc_flags: Set::from([
                    "@$(location //third-party/rust/crates/windows:build-script-run[rustc_flags])"
                        .to_string(),
                    "@$(location :manifest[env_flags])".to_string(),
                ]),
                visibility: Set::from(["PUBLIC".to_string()]),
                ..Default::default()
            }),
        ];

        assert_eq!(
            rules.len(),
            expected_rules.len(),
            "workspace BUCK should produce expected rule count"
        );

        for expected in expected_rules {
            let key = rule_map_key(&expected);
            let actual = rules
                .get(&key)
                .unwrap_or_else(|| panic!("rule with key '{}' should be present", key));
            assert_eq!(
                actual, &expected,
                "rule with key '{}' should match expected",
                key
            );
        }
    }
}
