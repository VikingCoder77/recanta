//! Per-language tree-sitter configuration for the MVP code graph (PRD §8.10): how to
//! classify each node (definition / import / recurse), read a symbol's name, and read an
//! import target. Languages: Python, JavaScript/JSX, TypeScript, TSX, Rust.

use tree_sitter::{Language, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
}

/// What a node means for the code graph.
pub enum Classified {
    /// An import/use statement.
    Import,
    /// A definition. `emit=false` records no symbol but still scopes children (e.g. a
    /// Rust `impl` block, so its methods become `Type.method`). `methods=true` means a
    /// direct function child is a method, not a free function.
    Def { symbol_type: &'static str, emit: bool, scopes: bool, methods: bool },
    /// Nothing to record; recurse into children to find nested definitions.
    Recurse,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext {
            "py" | "pyi" => Some(Lang::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "ts" | "mts" | "cts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "rs" => Some(Lang::Rust),
            _ => None,
        }
    }

    pub fn tree_sitter(self) -> Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    /// Classify a node. `in_methods_scope` is whether the nearest enclosing definition
    /// makes its functions methods (a class / trait / impl).
    pub fn classify(self, kind: &str, in_methods_scope: bool) -> Classified {
        use Classified::{Def, Import, Recurse};
        let func = if in_methods_scope { "method" } else { "function" };
        match self {
            Lang::Python => match kind {
                "function_definition" => Def { symbol_type: func, emit: true, scopes: true, methods: false },
                "class_definition" => Def { symbol_type: "class", emit: true, scopes: true, methods: true },
                "import_statement" | "import_from_statement" => Import,
                _ => Recurse,
            },
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx => match kind {
                "function_declaration" | "generator_function_declaration" => {
                    Def { symbol_type: func, emit: true, scopes: true, methods: false }
                }
                "method_definition" => Def { symbol_type: "method", emit: true, scopes: true, methods: false },
                "class_declaration" | "abstract_class_declaration" => {
                    Def { symbol_type: "class", emit: true, scopes: true, methods: true }
                }
                "interface_declaration" => Def { symbol_type: "interface", emit: true, scopes: true, methods: true },
                "import_statement" => Import,
                _ => Recurse,
            },
            Lang::Rust => match kind {
                "function_item" => Def { symbol_type: func, emit: true, scopes: true, methods: false },
                "struct_item" | "union_item" => Def { symbol_type: "struct", emit: true, scopes: true, methods: false },
                "enum_item" => Def { symbol_type: "enum", emit: true, scopes: true, methods: false },
                "trait_item" => Def { symbol_type: "trait", emit: true, scopes: true, methods: true },
                // impl blocks scope their methods to the implementing type but are not a
                // symbol themselves (the struct/enum already is).
                "impl_item" => Def { symbol_type: "impl", emit: false, scopes: true, methods: true },
                "mod_item" => Def { symbol_type: "module", emit: true, scopes: true, methods: false },
                "use_declaration" | "extern_crate_declaration" => Import,
                _ => Recurse,
            },
        }
    }
}

/// The declared name of a definition node — the `name` field, or the `type` field for
/// nodes like a Rust `impl` block (named after the type they implement). Generics are
/// stripped (`Foo<T>` → `Foo`).
pub fn def_name(node: Node, src: &[u8]) -> Option<String> {
    let n = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))?;
    let text = n.utf8_text(src).ok()?;
    let base = text.split('<').next().unwrap_or(text).trim();
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

/// Best-effort import target (module specifier) for an import node.
pub fn import_target(node: Node, src: &[u8], lang: Lang) -> Option<String> {
    if lang == Lang::Rust {
        // `use a::b::c;` / `use a::{b, c};` / `extern crate foo;`
        let text = node.utf8_text(src).ok()?;
        let first = text.lines().next().unwrap_or(text).trim();
        let body = first
            .strip_prefix("use ")
            .or_else(|| first.strip_prefix("extern crate "))
            .unwrap_or(first);
        let target: String = body
            .split(['{', ';', ' '])
            .next()
            .unwrap_or(body)
            .trim_end_matches(':')
            .to_string();
        return if target.is_empty() { None } else { Some(target) };
    }
    // JS/TS: `import ... from "x"` — the `source` field is a string literal.
    if let Some(source) = node.child_by_field_name("source") {
        return source
            .utf8_text(src)
            .ok()
            .map(|s| s.trim_matches(['"', '\'', '`']).to_string());
    }
    // Python `from x import y` — the `module_name` field.
    if let Some(m) = node.child_by_field_name("module_name") {
        return m.utf8_text(src).ok().map(|s| s.trim().to_string());
    }
    // Python `import x` and fallbacks.
    let text = node.utf8_text(src).ok()?;
    let first = text.lines().next().unwrap_or(text).trim();
    let stripped = first
        .strip_prefix("import ")
        .or_else(|| first.strip_prefix("from "))
        .unwrap_or(first);
    let target = stripped
        .split_whitespace()
        .next()
        .unwrap_or(stripped)
        .trim_matches(['"', '\'', '`', ';']);
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}
