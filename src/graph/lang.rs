//! Per-language tree-sitter configuration for the MVP code graph (PRD §8.10):
//! which node kinds are definitions, how to read a symbol's name, and how to read an
//! import target. MVP languages: Python, JavaScript/JSX, TypeScript, TSX.

use tree_sitter::{Language, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
}

impl Lang {
    /// Map a file extension to a language, or `None` if unsupported.
    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext {
            "py" | "pyi" => Some(Lang::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "ts" | "mts" | "cts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            _ => None,
        }
    }

    pub fn tree_sitter(self) -> Language {
        match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    /// Node kinds that declare a class-like container.
    pub fn class_kinds(self) -> &'static [&'static str] {
        match self {
            Lang::Python => &["class_definition"],
            _ => &["class_declaration", "abstract_class_declaration", "interface_declaration"],
        }
    }

    /// Node kinds that declare a function/method.
    pub fn function_kinds(self) -> &'static [&'static str] {
        match self {
            Lang::Python => &["function_definition"],
            _ => &[
                "function_declaration",
                "generator_function_declaration",
                "method_definition",
            ],
        }
    }

    /// Node kinds for imports.
    pub fn import_kinds(self) -> &'static [&'static str] {
        match self {
            Lang::Python => &["import_statement", "import_from_statement"],
            _ => &["import_statement"],
        }
    }

    /// Classify a definition node into the stored `symbol_type`, given whether its
    /// nearest enclosing definition is a class.
    pub fn symbol_type(self, kind: &str, in_class: bool) -> &'static str {
        if self.class_kinds().contains(&kind) {
            if kind == "interface_declaration" {
                "interface"
            } else {
                "class"
            }
        } else if kind == "method_definition" || (in_class && self.function_kinds().contains(&kind)) {
            "method"
        } else {
            "function"
        }
    }
}

/// The declared name of a definition node (`name` field), if present.
pub fn def_name(node: Node, src: &[u8]) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    name.utf8_text(src).ok().map(|s| s.to_string())
}

/// Best-effort import target (module specifier) for an import node.
pub fn import_target(node: Node, src: &[u8], lang: Lang) -> Option<String> {
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
    // Python `import x` (and fallbacks): strip the leading keyword from the first line.
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
    let _ = lang;
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}
