use reader_core::search::search_code;
use std::fs;
#[test]
fn literal_search_finds_variables_calls_and_unicode_without_tools() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("logic.py"),
        "def Route():\n    数量 = 3\n    return Route(数量)\n",
    )
    .unwrap();
    let result = search_code(root.path(), "route").unwrap();
    assert_eq!(result.matches.len(), 2);
    assert_eq!(result.matches[1].line, 3);
    assert_eq!(result.matches[0].file_hash.len(), 64);
    assert_eq!(search_code(root.path(), "数量").unwrap().matches.len(), 2);
    assert!(search_code(root.path(), "\n").is_err());
    assert!(search_code(root.path(), &"x".repeat(201)).is_err());
}
#[test]
fn search_obeys_ignores_budgets_and_tracks_source_changes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("node_modules")).unwrap();
    fs::write(root.path().join("node_modules/hidden.py"), "needle").unwrap();
    fs::write(root.path().join(".gitignore"), "secret.py\n").unwrap();
    fs::write(root.path().join("secret.py"), "needle").unwrap();
    fs::write(root.path().join("binary.bin"), b"needle\0").unwrap();
    fs::write(root.path().join("main.py"), "needle\n".repeat(220)).unwrap();
    let result = search_code(root.path(), "needle").unwrap();
    assert_eq!(result.matches.len(), 200);
    assert!(result.truncated);
    assert!(!result.warnings.is_empty());
    assert!(result.matches.iter().all(|m| m.path == "main.py"));
    let old = result.matches[0].file_hash.clone();
    fs::write(root.path().join("main.py"), "needle changed").unwrap();
    assert_ne!(
        old,
        search_code(root.path(), "needle").unwrap().matches[0].file_hash
    );
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("outside.py"), "outside_token").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("outside.py"),
            root.path().join("link.py"),
        )
        .unwrap();
        assert!(search_code(root.path(), "outside_token")
            .unwrap()
            .matches
            .is_empty());
    }
}
