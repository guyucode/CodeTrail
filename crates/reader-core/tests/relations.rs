use reader_core::{relations::analyze, *};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    dir
}

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn commit(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["add", "--all"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-qm",
            "base",
        ],
    );
}

fn bind(root: &Path, symbol_id: &str) -> String {
    let snapshot = save_feature(
        root,
        FeatureDraft {
            id: None,
            name: symbol_id.into(),
            description: String::new(),
            color: "#8b5cf6".into(),
            parent_id: None,
        },
    )
    .unwrap();
    let feature = snapshot
        .features
        .iter()
        .find(|f| f.name == symbol_id)
        .unwrap();
    let symbol = snapshot.symbols.iter().find(|s| s.id == symbol_id).unwrap();
    bind_anchor(
        root,
        BindRequest {
            feature_id: feature.id.clone(),
            path: symbol.range.file.clone(),
            start_line: symbol.range.start_line,
            end_line: symbol.range.end_line,
            symbol_id: Some(symbol.id.clone()),
            note: None,
            anchor_id: None,
        },
    )
    .unwrap();
    feature.id.clone()
}

fn static_relations(root: &Path, id: &str) -> SymbolRelations {
    analyze(root, id, "HEAD", "WORKTREE", false).unwrap()
}

#[test]
fn changed_middle_function_impacts_only_its_direct_caller_feature() {
    let source = "def c():\n    return 1\n\ndef b():\n    return c()\n\ndef a():\n    return b()\n\ndef d():\n    return a()\n\ndef reference_only():\n    return b\n";
    let dir = project(&[("flow.py", source)]);
    commit(dir.path());
    let a = bind(dir.path(), "flow.py::a");
    let b = bind(dir.path(), "flow.py::b");
    let c = bind(dir.path(), "flow.py::c");
    let d = bind(dir.path(), "flow.py::d");
    let reference_only = bind(dir.path(), "flow.py::reference_only");
    // A Feature can contain both a directly changed callee and its caller.
    // Direct evidence takes priority over the possible one-hop relationship.
    bind_anchor(
        dir.path(),
        BindRequest {
            feature_id: b.clone(),
            path: "flow.py".into(),
            start_line: 7,
            end_line: 8,
            symbol_id: Some("flow.py::a".into()),
            note: None,
            anchor_id: None,
        },
    )
    .unwrap();
    let before = open_project(dir.path()).unwrap();
    let feature = before.features.iter().find(|f| f.id == a).unwrap();
    set_review(
        dir.path(),
        &a,
        ReviewState::Verified,
        Some(&feature.current_digest),
    )
    .unwrap();
    fs::write(
        dir.path().join("flow.py"),
        source.replace("return c()", "return c() + 1"),
    )
    .unwrap();
    let result = static_relations(dir.path(), "flow.py::b");
    assert_eq!(result.incoming.len(), 1);
    assert_eq!(
        result.incoming[0].source_symbol_id.as_deref(),
        Some("flow.py::a")
    );
    assert_eq!(result.incoming[0].site.start_line, 8);
    assert_eq!(result.outgoing.len(), 1);
    assert_eq!(
        result.outgoing[0].target_symbol_id.as_deref(),
        Some("flow.py::c")
    );
    assert!(result.changed_symbol_ids.contains(&"flow.py::b".into()));
    let direct = result.impacts.iter().find(|i| i.feature_id == b).unwrap();
    assert_eq!(direct.kind, FeatureImpactKind::Direct);
    let possible = result.impacts.iter().find(|i| i.feature_id == a).unwrap();
    assert_eq!(possible.kind, FeatureImpactKind::Possible);
    assert_eq!(possible.evidence.len(), 1);
    assert!(!result.impacts.iter().any(|i| i.feature_id == c));
    assert!(!result.impacts.iter().any(|i| i.feature_id == d));
    assert!(!result
        .impacts
        .iter()
        .any(|i| i.feature_id == reference_only));
    assert_eq!(result.references.len(), 1);
    assert_eq!(
        open_project(dir.path())
            .unwrap()
            .features
            .iter()
            .find(|f| f.id == a)
            .unwrap()
            .review_state,
        ReviewState::Verified
    );
}

#[test]
fn imports_aliases_and_same_names_use_exact_module_paths() {
    let dir = project(&[
        ("one/service.py", "def run():\n    return 1\n"),
        ("two/service.py", "def run():\n    return 2\n"),
        ("app.py", "from one.service import run as first\nimport two.service as second\n\ndef route():\n    return first(), second.run()\n"),
    ]);
    let result = static_relations(dir.path(), "app.py::route");
    let ids: Vec<_> = result
        .outgoing
        .iter()
        .filter_map(|r| r.target_symbol_id.as_deref())
        .collect();
    assert_eq!(ids, vec!["one/service.py::run", "two/service.py::run"]);
    assert!(result
        .outgoing
        .iter()
        .all(|r| r.resolution == RelationResolution::Resolved));
    assert_eq!(
        static_relations(dir.path(), "one/service.py::run")
            .incoming
            .len(),
        1
    );
}

#[test]
fn parameter_and_assignment_shadowing_never_join_global_same_name() {
    let dir = project(&[("app.py", "def target():\n    pass\n\ndef parameter(target):\n    target()\n\ndef local():\n    target()\n    target = lambda: None\n\ndef rebound():\n    return target()\n\ntarget = object()\n")]);
    for name in ["parameter", "local", "rebound"] {
        let result = static_relations(dir.path(), &format!("app.py::{name}"));
        assert!(!result.outgoing.is_empty());
        assert!(result.outgoing.iter().all(|r| r.target_symbol_id.is_none()));
    }
    assert!(static_relations(dir.path(), "app.py::target")
        .incoming
        .is_empty());
}

#[test]
fn lexical_closure_and_class_scope_are_distinct() {
    let dir = project(&[("app.py", "def helper():\n    return 0\n\nclass Worker:\n    def helper(self):\n        return 1\n    def run(self):\n        return helper(), self.helper()\n\ndef outer():\n    def helper():\n        return 2\n    return helper()\n")]);
    let method = static_relations(dir.path(), "app.py::Worker::run");
    assert_eq!(
        method.outgoing[0].target_symbol_id.as_deref(),
        Some("app.py::helper")
    );
    assert_eq!(
        method.outgoing[1].resolution,
        RelationResolution::Unresolved
    );
    let closure = static_relations(dir.path(), "app.py::outer");
    assert_eq!(
        closure.outgoing[0].target_symbol_id.as_deref(),
        Some("app.py::outer::helper")
    );
}

#[test]
fn passing_and_storing_a_function_are_references_not_calls() {
    let dir = project(&[("app.py", "def target():\n    pass\n\ndef consume(value):\n    pass\n\ndef route():\n    callback = target\n    consume(target)\n    callback()\n")]);
    let target = static_relations(dir.path(), "app.py::target");
    assert!(target.incoming.is_empty());
    assert_eq!(target.references.len(), 2);
    assert!(target
        .references
        .iter()
        .all(|r| r.kind == RelationKind::Reference));
    let route = static_relations(dir.path(), "app.py::route");
    assert_eq!(route.outgoing.len(), 2);
    assert_eq!(
        route.outgoing[0].target_symbol_id.as_deref(),
        Some("app.py::consume")
    );
    assert_eq!(route.outgoing[1].resolution, RelationResolution::Unresolved);
}

#[test]
fn external_dynamic_and_missing_calls_remain_visible_with_reasons() {
    let dir = project(&[("app.py", "import requests\n\ndef run(receiver, callbacks):\n    requests.get('https://example.invalid')\n    receiver.method()\n    callbacks[0]()\n    missing()\n")]);
    let result = static_relations(dir.path(), "app.py::run");
    assert_eq!(result.outgoing.len(), 4);
    assert_eq!(result.outgoing[0].resolution, RelationResolution::External);
    assert!(result
        .outgoing
        .iter()
        .all(|r| r.target.is_none() && !r.detail.is_empty()));
    assert!(result.outgoing[1..]
        .iter()
        .all(|r| r.resolution == RelationResolution::Unresolved));
}

#[test]
fn relative_imports_and_reexports_resolve_without_global_name_search() {
    let dir = project(&[
        ("pkg/__init__.py", "from .worker import run as exported\n"),
        ("pkg/worker.py", "def run():\n    return 1\n"),
        ("pkg/client.py", "from . import exported\nfrom .worker import run as direct\n\ndef route():\n    return exported(), direct()\n"),
    ]);
    let result = static_relations(dir.path(), "pkg/client.py::route");
    assert_eq!(result.outgoing.len(), 2);
    assert!(result
        .outgoing
        .iter()
        .all(|r| r.target_symbol_id.as_deref() == Some("pkg/worker.py::run")));
}

#[test]
fn unaliased_namespace_import_does_not_invent_unimported_submodules() {
    let dir = project(&[
        ("pkg/loaded.py", "def run():\n    pass\n"),
        ("pkg/unloaded.py", "def run():\n    pass\n"),
        (
            "app.py",
            "import pkg.loaded\n\ndef route():\n    pkg.loaded.run()\n    pkg.unloaded.run()\n",
        ),
    ]);
    let result = static_relations(dir.path(), "app.py::route");
    assert_eq!(
        result.outgoing[0].target_symbol_id.as_deref(),
        Some("pkg/loaded.py::run")
    );
    assert_eq!(
        result.outgoing[1].resolution,
        RelationResolution::Unresolved
    );
}

#[test]
fn ambiguous_source_roots_never_choose_first_match() {
    let dir = project(&[
        ("service.py", "def run():\n    return 1\n"),
        ("src/service.py", "def run():\n    return 2\n"),
        (
            "app.py",
            "from service import run\n\ndef route():\n    return run()\n",
        ),
    ]);
    let result = static_relations(dir.path(), "app.py::route");
    assert_eq!(result.outgoing[0].resolution, RelationResolution::Ambiguous);
    assert!(result.outgoing[0].target_symbol_id.is_none());
}

#[test]
fn comprehension_lambda_and_pattern_bindings_do_not_leak_or_join_outer_functions() {
    let dir = project(&[("app.py", "def target():\n    pass\n\ndef route(values):\n    [target() for target in values]\n    callback = lambda target: target()\n    target()\n\ndef matched(value):\n    match value:\n        case {'callback': target}:\n            target()\n")]);
    let target = static_relations(dir.path(), "app.py::target");
    assert_eq!(target.incoming.len(), 1);
    assert_eq!(target.incoming[0].site.start_line, 7);
    assert!(static_relations(dir.path(), "app.py::matched")
        .outgoing
        .iter()
        .all(|r| r.resolution != RelationResolution::Resolved));
}

#[test]
fn evidence_columns_hashes_and_ids_are_real_and_deterministic() {
    let source =
        "def target():\n    return 1\n\ndef route():\n    label = '😀'; target(); target()\n";
    let dir = project(&[("app.py", source)]);
    let a = static_relations(dir.path(), "app.py::target");
    let b = static_relations(dir.path(), "app.py::target");
    assert_eq!(a, b);
    assert_eq!(a.incoming.len(), 2);
    assert_ne!(a.incoming[0].id, a.incoming[1].id);
    let line = source.lines().nth(4).unwrap();
    let prefix = &line[..line.find("target").unwrap()];
    assert_eq!(
        a.incoming[0].site_column,
        prefix.encode_utf16().count() as u32 + 1
    );
    assert_eq!(
        a.file_hashes["app.py"],
        read_file(dir.path(), "app.py").unwrap().hash
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("app.py")).unwrap(),
        source
    );
    assert!(!dir.path().join(".reader").exists());
    fs::write(
        dir.path().join("app.py"),
        source.replace("return 1", "return 2"),
    )
    .unwrap();
    assert_ne!(
        a.file_hashes["app.py"],
        read_file(dir.path(), "app.py").unwrap().hash
    );
    assert_ne!(
        a.source_digest,
        static_relations(dir.path(), "app.py::target").source_digest
    );
}

#[test]
fn historical_comparisons_never_infer_impact_from_current_graph() {
    let dir = project(&[(
        "app.py",
        "def target():\n    return 1\n\ndef route():\n    return target()\n",
    )]);
    commit(dir.path());
    bind(dir.path(), "app.py::target");
    fs::write(
        dir.path().join("app.py"),
        "def target():\n    return 2\n\ndef route():\n    return target()\n",
    )
    .unwrap();
    let result = analyze(dir.path(), "app.py::target", "HEAD", "HEAD", false).unwrap();
    assert_eq!(result.incoming.len(), 1);
    assert!(result.changed_symbol_ids.is_empty());
    assert!(result.impacts.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("WORKTREE") && w.contains("历史")));
}

#[test]
fn deleted_identity_has_unknown_historical_callers() {
    let dir = project(&[(
        "app.py",
        "def old():\n    return 1\n\ndef route():\n    return old()\n",
    )]);
    commit(dir.path());
    fs::write(
        dir.path().join("app.py"),
        "def renamed():\n    return 1\n\ndef route():\n    return old()\n",
    )
    .unwrap();
    assert!(analyze(dir.path(), "app.py::old", "HEAD", "WORKTREE", false).is_err());
    let result = static_relations(dir.path(), "app.py::route");
    assert_eq!(
        result.outgoing[0].resolution,
        RelationResolution::Unresolved
    );
    assert!(result.warnings.iter().any(|w| w.contains("历史调用者")));
}

#[test]
fn unsupported_language_is_explicit_without_fake_relationships() {
    let dir = project(&[("main.rs", "fn target() {}\nfn route() { target(); }\n")]);
    let result = static_relations(dir.path(), "main.rs::route");
    assert!(
        result.incoming.is_empty() && result.outgoing.is_empty() && result.references.is_empty()
    );
    assert_eq!(result.engine.status, RelationEngineStatus::Unavailable);
    assert!(result.warnings.iter().any(|w| w.contains("Python")));
}

#[test]
fn conditional_definitions_and_wildcard_imports_stay_uncertain() {
    let dir = project(&[
        ("one.py", "def run():\n    pass\n"),
        ("two.py", "def run():\n    pass\n"),
        ("app.py", "if configured:\n    from one import run\nelse:\n    from two import run\n\ndef route():\n    run()\n"),
        ("wild.py", "from one import *\n\ndef route():\n    run()\n"),
    ]);
    let branch = static_relations(dir.path(), "app.py::route");
    assert_eq!(branch.outgoing[0].resolution, RelationResolution::Ambiguous);
    let wildcard = static_relations(dir.path(), "wild.py::route");
    assert_eq!(
        wildcard.outgoing[0].resolution,
        RelationResolution::Unresolved
    );
    assert!(static_relations(dir.path(), "one.py::run")
        .incoming
        .is_empty());
}

#[test]
fn defaults_decorators_and_lambda_bodies_are_not_called_by_the_declared_function() {
    let dir = project(&[("app.py", "def factory():\n    return lambda value: value\n\n@factory()\ndef route(value=factory()):\n    callback = lambda: factory()\n    return callback\n")]);
    let route = static_relations(dir.path(), "app.py::route");
    assert!(route.outgoing.is_empty());
    let factory = static_relations(dir.path(), "app.py::factory");
    assert_eq!(factory.incoming.len(), 3);
    assert!(factory
        .incoming
        .iter()
        .all(|r| r.source_symbol_id.is_none()));
}

#[test]
fn explicit_global_rebinding_and_attribute_writes_do_not_keep_original_targets() {
    let dir = project(&[
        ("service.py", "def run():\n    pass\n"),
        ("app.py", "import service\n\ndef target():\n    pass\n\ndef replace(value):\n    global target\n    target = value\n\ndef route():\n    service.run = target\n    service.run()\n    target()\n"),
    ]);
    let result = static_relations(dir.path(), "app.py::route");
    assert_eq!(result.outgoing.len(), 2);
    assert!(result
        .outgoing
        .iter()
        .all(|r| r.resolution == RelationResolution::Unresolved));
}

#[test]
fn early_evaluation_and_forward_deferred_calls_are_distinguished() {
    let dir = project(&[("app.py", "early()\n\ndef early():\n    return later()\n\ndef later():\n    return 1\n\ndef route():\n    nested()\n    def nested():\n        return 1\n")]);
    assert!(static_relations(dir.path(), "app.py::early")
        .incoming
        .is_empty());
    let early = static_relations(dir.path(), "app.py::early");
    assert_eq!(
        early.outgoing[0].target_symbol_id.as_deref(),
        Some("app.py::later")
    );
    let route = static_relations(dir.path(), "app.py::route");
    assert_eq!(route.outgoing[0].resolution, RelationResolution::Unresolved);
}

#[test]
fn returned_and_indexed_callables_never_become_direct_calls_to_the_receiver_name() {
    let dir = project(&[("app.py", "def factory():\n    return lambda: 1\n\ndef route(callbacks):\n    factory()()\n    callbacks[0]()\n")]);
    let route = static_relations(dir.path(), "app.py::route");
    assert_eq!(route.outgoing.len(), 3);
    let inner = route
        .outgoing
        .iter()
        .find(|r| r.target_name == "factory")
        .unwrap();
    assert_eq!(inner.target_symbol_id.as_deref(), Some("app.py::factory"));
    for target in ["factory()", "callbacks[0]"] {
        let dynamic = route
            .outgoing
            .iter()
            .find(|r| r.target_name == target)
            .unwrap();
        assert_eq!(dynamic.resolution, RelationResolution::Unresolved);
        assert!(dynamic.target_symbol_id.is_none() && dynamic.target.is_none());
        assert!(dynamic.detail.contains("动态调用表达式"));
    }
    assert_eq!(
        static_relations(dir.path(), "app.py::factory")
            .incoming
            .len(),
        1
    );
}

#[test]
fn module_attribute_rebinding_invalidates_outer_import_across_functions_and_aliases() {
    for source in [
        "import service\n\ndef replacement():\n    pass\n\ndef patch():\n    service.run = replacement\n\npatch()\n\ndef caller():\n    service.run()\n",
        "import service as public\n\ndef replacement():\n    pass\n\ndef patch():\n    import service as private\n    private.run = replacement\n\npatch()\n\ndef caller():\n    public.run()\n",
        "from pkg import service as public\n\ndef replacement():\n    pass\n\ndef patch():\n    public.run = replacement\n\npatch()\n\ndef caller():\n    public.run()\n",
    ] {
        let dir = project(&[("service.py", "def run():\n    return 1\n"), ("pkg/service.py", "def run():\n    return 1\n"), ("app.py", source)]);
        // Even if a language server can navigate to the original declaration,
        // it cannot revive a binding that this captured source explicitly writes.
        for semantic in [false, true] {
            let result = analyze(dir.path(), "app.py::caller", "HEAD", "WORKTREE", semantic).unwrap();
            assert_eq!(result.outgoing.len(), 1);
            assert_eq!(result.outgoing[0].resolution, RelationResolution::Unresolved);
            assert!(result.outgoing[0].target_symbol_id.is_none());
            assert_eq!(result.outgoing[0].origin, RelationOrigin::Syntax);
            assert!(result.warnings.iter().any(|w| w.contains("导入模块属性写入")));
        }
    }
}

#[test]
fn local_receiver_parameter_does_not_invalidate_a_distinct_global_import() {
    let dir = project(&[
        ("service.py", "def run():\n    return 1\n"),
        ("app.py", "import service\n\ndef patch(service, replacement):\n    service.run = replacement\n\ndef caller():\n    service.run()\n"),
    ]);
    let result = static_relations(dir.path(), "app.py::caller");
    assert_eq!(
        result.outgoing[0].target_symbol_id.as_deref(),
        Some("service.py::run")
    );
    assert!(result.warnings.iter().any(|w| w.contains("参数、容器")));
}

#[test]
fn decorated_targets_explicitly_explain_declaration_and_runtime_identity_boundary() {
    let dir = project(&[("app.py", "def decorate(original):\n    return lambda: 2\n\n@decorate\ndef target():\n    return 1\n\ndef caller():\n    return target()\n")]);
    for semantic in [false, true] {
        let result = analyze(dir.path(), "app.py::target", "HEAD", "WORKTREE", semantic).unwrap();
        assert_eq!(result.incoming.len(), 1);
        let relation = &result.incoming[0];
        assert_eq!(relation.target_symbol_id.as_deref(), Some("app.py::target"));
        assert!(
            relation.detail.contains("@decorator") && relation.detail.contains("替换可调用对象")
        );
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("选中声明 target") && w.contains("带装饰器")));
        assert_eq!(result.symbol.range.start_line, 4);
    }
}

#[test]
fn unrelated_files_cannot_exhaust_a_late_symbols_relationships() {
    let dir = project(&[
        ("z_logic.py", "def leaf():\n    return 1\n\ndef focus():\n    return leaf()\n"),
        ("zz_caller.py", "from z_logic import focus as renamed\n\ndef caller():\n    return renamed()\n\ndef reference():\n    return renamed\n"),
    ]);
    // Over 50,000 unrelated reads sort before the real application and caller.
    for index in 0..8 {
        let noise = format!(
            "def noise():\n    value = 1\n{}",
            "    value\n".repeat(7000)
        );
        fs::write(dir.path().join(format!("a_noise_{index}.py")), noise).unwrap();
    }
    let result = static_relations(dir.path(), "z_logic.py::focus");
    assert!(result
        .outgoing
        .iter()
        .any(|r| r.target_symbol_id.as_deref() == Some("z_logic.py::leaf")));
    assert!(result
        .incoming
        .iter()
        .any(|r| r.source_symbol_id.as_deref() == Some("zz_caller.py::caller")));
    assert!(result
        .references
        .iter()
        .any(|r| r.source_symbol_id.as_deref() == Some("zz_caller.py::reference")));
    assert!(
        !result.warnings.iter().any(|w| w.contains("位置上限")
            || w.contains("位置超过")
            || w.contains("关系声明分析不完整")),
        "{:?}",
        result.warnings
    );
}

#[test]
fn one_oversized_file_is_reported_without_starving_other_files() {
    let dir = project(&[
        ("z_logic.py", "def focus():\n    return 1\n"),
        (
            "zz_caller.py",
            "from z_logic import focus\n\ndef caller():\n    return focus()\n",
        ),
    ]);
    fs::write(
        dir.path().join("a_huge.py"),
        format!("value = 1\n{}", "value\n".repeat(50_001)),
    )
    .unwrap();
    let result = static_relations(dir.path(), "z_logic.py::focus");
    assert!(result
        .incoming
        .iter()
        .any(|r| r.source_symbol_id.as_deref() == Some("zz_caller.py::caller")));
    let limits: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| w.contains("单文件 50000"))
        .collect();
    assert_eq!(limits.len(), 1);
    assert!(limits[0].contains("a_huge.py"));
    assert!(!result
        .warnings
        .iter()
        .any(|w| w.contains("语法错误") && w.contains("a_huge.py")));
    assert_eq!(result.engine.status, RelationEngineStatus::Partial);
}

#[test]
fn actual_query_results_still_have_an_explicit_bounded_limit() {
    let dir = project(&[("target.py", "def focus():\n    return 1\n")]);
    for file in ["a.py", "b.py"] {
        fs::write(
            dir.path().join(file),
            format!(
                "from target import focus\n\ndef caller():\n{}",
                "    focus()\n".repeat(26_000)
            ),
        )
        .unwrap();
    }
    let result = static_relations(dir.path(), "target.py::focus");
    assert_eq!(result.incoming.len(), 50_000);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("当前符号或变更影响的关系结果超过")));
    assert_eq!(result.engine.status, RelationEngineStatus::Partial);
}
