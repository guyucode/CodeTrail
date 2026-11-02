//! Syntactic declarations only. This module makes no claims about resolved calls.
use crate::types::{CodeRange, Symbol};
use anyhow::{Context, Result};
use std::collections::HashMap;
use tree_sitter::{Language, Node, Parser};

pub fn language_for(path: &str) -> &'static str {
    if path.ends_with(".C") || path.ends_with(".H") {
        return "cpp";
    }
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "py" | "pyi" => "python",
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" | "jsonc" => "json",
        "md" | "mdx" => "markdown",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        "sh" | "bash" | "zsh" => "shell",
        "sql" => "sql",
        "c" | "h" => "c",
        "cc" | "cp" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ipp" | "tpp"
        | "inl" => "cpp",
        "go" => "go",
        "java" => "java",
        "xml" | "svg" => "xml",
        _ => "plaintext",
    }
}

fn grammar(path: &str) -> Option<Language> {
    match language_for(path) {
        "c" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" => Some(tree_sitter_cpp::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "typescript" if path.ends_with(".tsx") => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        _ => None,
    }
}

pub fn parse_symbols(path: &str, content: &str) -> Result<Vec<Symbol>> {
    Ok(parse_with_status(path, content)?.0)
}

pub(crate) fn parse_with_status(path: &str, content: &str) -> Result<(Vec<Symbol>, bool)> {
    let Some(language) = grammar(path) else {
        return Ok((Vec::new(), false));
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .context("语言解析器 ABI 不兼容")?;
    let mut tree = parser.parse(content, None).context("语法解析未完成")?;
    // .h is shared by C and C++; prefer C unless the C++ grammar recovers more syntax.
    if language_for(path) == "c"
        && path.to_ascii_lowercase().ends_with(".h")
        && tree.root_node().has_error()
    {
        parser.set_language(&tree_sitter_cpp::LANGUAGE.into())?;
        if let Some(cpp) = parser.parse(content, None) {
            if error_count(cpp.root_node(), 0) < error_count(tree.root_node(), 0) {
                tree = cpp;
            }
        }
    }
    let mut state = Extraction {
        path,
        content,
        symbols: Vec::new(),
        identities: HashMap::new(),
    };
    state.visit(tree.root_node(), &[], 0);
    state.symbols.sort_by(|a, b| {
        (a.range.start_line, a.range.end_line, &a.id).cmp(&(
            b.range.start_line,
            b.range.end_line,
            &b.id,
        ))
    });
    Ok((state.symbols, tree.root_node().has_error()))
}

fn error_count(node: Node<'_>, depth: usize) -> usize {
    if depth > 256 {
        return 1;
    }
    let mut cursor = node.walk();
    usize::from(node.is_error() || node.is_missing())
        + node
            .children(&mut cursor)
            .map(|n| error_count(n, depth + 1))
            .sum::<usize>()
}

// Follow declarator fields, never parameter identifiers or arbitrary descendants.
fn c_name(node: Node<'_>, content: &str, depth: usize) -> Option<String> {
    if depth > 64 {
        return None;
    }
    if node.kind() == "operator_cast" {
        let end = node.child_by_field_name("declarator")?.start_byte();
        return Some(content.get(node.start_byte()..end)?.trim().to_owned());
    }
    if node.kind() == "parenthesized_declarator" {
        return c_name(node.named_child(0)?, content, depth + 1);
    }
    match node.kind() {
        "identifier"
        | "field_identifier"
        | "type_identifier"
        | "namespace_identifier"
        | "nested_namespace_specifier"
        | "qualified_identifier"
        | "template_function"
        | "template_method"
        | "template_type"
        | "destructor_name"
        | "operator_name" => Some(content.get(node.byte_range())?.trim().to_owned()),
        _ => c_name(node.child_by_field_name("declarator")?, content, depth + 1),
    }
}
fn c_callable(node: Node<'_>, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    if node.kind() == "operator_cast" {
        return true;
    }
    if node.kind() == "function_declarator" {
        // (*callback)(int) is a variable; (*factory(int))(int) is a function.
        return node.child_by_field_name("declarator").is_some_and(|n| {
            n.kind() != "parenthesized_declarator"
                || n.named_child(0).is_some_and(|inner| {
                    matches!(inner.kind(), "identifier" | "qualified_identifier")
                        || c_callable(inner, depth + 1)
                })
        });
    }
    node.child_by_field_name("declarator")
        .is_some_and(|n| c_callable(n, depth + 1))
}

struct Extraction<'a> {
    path: &'a str,
    content: &'a str,
    symbols: Vec<Symbol>,
    identities: HashMap<String, u32>,
}

impl Extraction<'_> {
    fn text(&self, node: Node<'_>) -> &str {
        self.content.get(node.byte_range()).unwrap_or("")
    }

    fn visit(&mut self, node: Node<'_>, owners: &[String], depth: usize) {
        if depth > 256 {
            return;
        }
        let mut nested = owners.to_vec();
        if node.kind() == "impl_item" {
            if let Some(ty) = node.child_by_field_name("type") {
                let mut scope = self.text(ty).to_owned();
                if let Some(tr) = node.child_by_field_name("trait") {
                    scope = format!("<{} as {}>", scope, self.text(tr));
                }
                nested.push(scope);
            }
        }
        if matches!(language_for(self.path), "c" | "cpp")
            && matches!(node.kind(), "declaration" | "field_declaration")
        {
            let mut cursor = node.walk();
            for declarator in node.children_by_field_name("declarator", &mut cursor) {
                if c_callable(declarator, 0) {
                    if let Some(name) = c_name(declarator, self.content, 0) {
                        self.record(
                            node,
                            name,
                            if node.kind() == "field_declaration" {
                                "method"
                            } else {
                                "function"
                            },
                            owners,
                        );
                    }
                }
            }
        } else if let Some((name, kind)) = self.declaration(node) {
            let scope = self.record(node, name, kind, &nested);
            if !matches!(language_for(self.path), "c" | "cpp") || !matches!(kind, "type" | "macro")
            {
                nested = scope;
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, &nested, depth + 1);
        }
    }

    fn record(
        &mut self,
        node: Node<'_>,
        name: String,
        kind: &'static str,
        owners: &[String],
    ) -> Vec<String> {
        let mut nested = owners.to_vec();
        let prefix = nested.join("::");
        if !prefix.is_empty() && name.starts_with(&(prefix + "::")) {
            nested = vec![name.clone()];
        } else {
            nested.push(name.clone());
        }
        let qualified_name = nested.join("::");
        let base_id = format!("{}::{}", self.path, qualified_name);
        let count = self.identities.entry(base_id.clone()).or_default();
        *count += 1;
        let id = if *count == 1 {
            base_id
        } else {
            format!("{base_id}#{}", *count)
        };
        let mut start = node.start_position().row + 1;
        if let Some(parent) = node.parent() {
            if matches!(parent.kind(), "decorated_definition" | "export_statement") {
                start = parent.start_position().row + 1;
            }
        }
        let mut previous = node.prev_named_sibling();
        while let Some(sibling) = previous {
            if sibling.kind() == "attribute_item" {
                start = sibling.start_position().row + 1;
                previous = sibling.prev_named_sibling();
            } else {
                break;
            }
        }
        let end = node.end_position();
        let end_line = if end.column == 0 {
            end.row.max(start)
        } else {
            end.row + 1
        };
        let signature_end = node
            .child_by_field_name("body")
            .map(|n| n.start_byte())
            .unwrap_or(node.end_byte());
        let signature = self
            .content
            .get(node.start_byte()..signature_end)
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(220)
            .collect();
        self.symbols.push(Symbol {
            id,
            name,
            qualified_name,
            kind: kind.to_owned(),
            signature,
            range: CodeRange {
                file: self.path.to_owned(),
                start_line: start as u32,
                end_line: end_line as u32,
            },
        });
        nested
    }

    fn c_declaration(&self, node: Node<'_>) -> Option<(String, &'static str)> {
        let kind = match node.kind() {
            "namespace_definition" => "namespace",
            "struct_specifier" => "struct",
            "class_specifier" => "class",
            "union_specifier" => "union",
            "enum_specifier" => "enum",
            "alias_declaration" | "type_definition" => "type",
            "preproc_def" | "preproc_function_def" => "macro",
            "function_definition" | "declaration" | "field_declaration" => {
                let declarator = node.child_by_field_name("declarator")?;
                if !c_callable(declarator, 0) {
                    return None;
                }
                let name = c_name(declarator, self.content, 0)?;
                return Some((
                    name,
                    if node.kind() == "field_declaration" {
                        "method"
                    } else {
                        "function"
                    },
                ));
            }
            _ => return None,
        };
        let name = node
            .child_by_field_name("name")
            .and_then(|n| c_name(n, self.content, 0))
            .or_else(|| {
                node.child_by_field_name("declarator")
                    .and_then(|n| c_name(n, self.content, 0))
            })?;
        Some((name, kind))
    }

    fn declaration(&self, node: Node<'_>) -> Option<(String, &'static str)> {
        if matches!(language_for(self.path), "c" | "cpp") {
            return self.c_declaration(node);
        }
        let kind = match node.kind() {
            "function_definition"
            | "function_item"
            | "function_declaration"
            | "generator_function_declaration" => "function",
            "method_definition" | "method_signature" | "abstract_method_signature" => "method",
            "class_definition" | "class_declaration" | "abstract_class_declaration" => "class",
            "struct_item" => "struct",
            "enum_item" | "enum_declaration" => "enum",
            "trait_item" | "interface_declaration" => "interface",
            "type_item" | "type_alias_declaration" => "type",
            "mod_item" => "module",
            "const_item" | "static_item" => "constant",
            "variable_declarator" => {
                let value = node.child_by_field_name("value")?;
                if !matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "generator_function"
                ) {
                    return None;
                }
                "function"
            }
            _ => return None,
        };
        let name = self.text(node.child_by_field_name("name")?).trim();
        if name.is_empty() {
            None
        } else {
            Some((name.to_owned(), kind))
        }
    }
}
