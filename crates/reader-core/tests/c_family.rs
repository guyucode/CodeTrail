use reader_core::{open_project, parse_symbols};
use std::fs;

#[test]
fn c_functions_prototypes_types_macros_and_pointer_variables() {
    let source = "#define LIMIT 8\n#define SCALE(x) ((x)*2)\ntypedef unsigned Count;\nstruct Request { int id; };\nint forward(int value);\nint (*callback)(int);\nint *pointer_result(int value) { return 0; }\nint sum(int a, int b) { return a + b; }\n";
    let symbols = parse_symbols("logic.c", source).unwrap();
    for name in [
        "LIMIT",
        "SCALE",
        "Count",
        "Request",
        "forward",
        "pointer_result",
        "sum",
    ] {
        assert!(
            symbols.iter().any(|s| s.name == name),
            "missing {name}: {symbols:?}"
        );
    }
    assert!(!symbols.iter().any(|s| s.name == "callback"));
    let factory = parse_symbols(
        "factory.c",
        "int (*factory(int count))(int); int (wrapped)(int);",
    )
    .unwrap();
    assert!(factory.iter().any(|s| s.name == "factory"));
    assert!(factory.iter().any(|s| s.name == "wrapped"));
    let sum = symbols.iter().find(|s| s.name == "sum").unwrap();
    assert_eq!(sum.range.start_line, 8);
    assert_eq!(sum.range.end_line, 8);
    assert!(sum.signature.contains("int sum"));
}

#[test]
fn cpp_namespaces_templates_overloads_and_out_of_class_methods() {
    let source = r#"namespace app {
template<class T> class Box {
public:
  Box() {}
  ~Box() {}
  T get() const { return T{}; }
  int run(int x);
  int run(double x);
  int operator+(int x) const { return x; }
};
int Box<int>::run(int x) { return x; }
}
"#;
    let symbols = parse_symbols("box.hpp", source).unwrap();
    for name in [
        "app",
        "app::Box",
        "app::Box::Box",
        "app::Box::~Box",
        "app::Box::get",
        "app::Box::run",
        "app::Box::operator+",
        "app::Box<int>::run",
    ] {
        assert!(
            symbols.iter().any(|s| s.qualified_name == name),
            "missing {name}: {symbols:?}"
        );
    }
    let runs: Vec<_> = symbols
        .iter()
        .filter(|s| s.qualified_name == "app::Box::run")
        .collect();
    assert_eq!(runs.len(), 2);
    assert_ne!(runs[0].id, runs[1].id);
}

#[test]
fn headers_extensions_and_syntax_errors_keep_partial_navigation() {
    for path in [
        "widget.h",
        "widget.H",
        "widget.hxx",
        "widget.hh",
        "widget.ipp",
        "widget.tpp",
        "widget.inl",
        "widget.C",
    ] {
        let symbols = parse_symbols(
            path,
            "namespace app { class Widget { public: void run() {} }; }",
        )
        .unwrap();
        assert!(
            symbols
                .iter()
                .any(|s| s.qualified_name == "app::Widget::run"),
            "{path}: {symbols:?}"
        );
    }
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("broken.cpp"),
        "int before() { return 1; }\nint broken( {\n",
    )
    .unwrap();
    let snapshot = open_project(root.path()).unwrap();
    assert!(snapshot.symbols.iter().any(|s| s.name == "before"));
    assert!(snapshot.warnings.iter().any(|w| w.contains("broken.cpp")));
}

#[test]
fn cpp_nested_namespaces_conversions_and_multiple_prototypes() {
    let symbols = parse_symbols("api.hpp", "namespace app::detail { class Flag { public: explicit operator bool() const { return true; } }; int start(), stop(); }").unwrap();
    for name in [
        "app::detail::Flag::operator bool",
        "app::detail::start",
        "app::detail::stop",
    ] {
        assert!(
            symbols.iter().any(|s| s.qualified_name == name),
            "{name}: {symbols:?}"
        );
    }
}

#[test]
fn typedef_does_not_invent_a_lexical_scope_for_the_struct_tag() {
    let symbols = parse_symbols("types.h", "typedef struct Item { int id; } ItemAlias;").unwrap();
    assert!(symbols.iter().any(|s| s.qualified_name == "ItemAlias"));
    assert!(symbols.iter().any(|s| s.qualified_name == "Item"));
    assert!(!symbols
        .iter()
        .any(|s| s.qualified_name == "ItemAlias::Item"));
}
