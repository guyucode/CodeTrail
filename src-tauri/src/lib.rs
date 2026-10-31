use reader_core::{
    BindRequest, ChangeSummary, FeatureDraft, FileContent, FileDiff, ProjectSnapshot, ReviewState,
    SymbolRelations,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};

#[derive(Clone, Default)]
struct Session(Arc<Mutex<Option<PathBuf>>>);

// A session holds one explicitly opened repository. Commands never accept a new
// filesystem root indirectly through a read/write request. Work runs off the UI
// thread, and metadata mutations are serialized together with refresh operations.
async fn with_root<T: Send + 'static>(
    session: Session,
    operation: impl FnOnce(&Path) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let guard = session
            .0
            .lock()
            .map_err(|_| "项目会话不可用，请重新打开应用".to_string())?;
        let root = guard
            .as_ref()
            .ok_or_else(|| "请先打开一个仓库".to_string())?;
        operation(root)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn open_project(
    path: String,
    session: State<'_, Session>,
) -> Result<ProjectSnapshot, String> {
    let state = session.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state.0.lock().map_err(|_| "项目会话不可用".to_string())?;
        let root = Path::new(&path)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let result = reader_core::open_project(&root).map_err(|error| error.to_string())?;
        *guard = Some(root);
        Ok(result)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn refresh_project(session: State<'_, Session>) -> Result<ProjectSnapshot, String> {
    with_root(session.inner().clone(), |root| {
        reader_core::open_project(root).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn read_file(path: String, session: State<'_, Session>) -> Result<FileContent, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::read_file(root, &path).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn save_feature(
    draft: FeatureDraft,
    session: State<'_, Session>,
) -> Result<ProjectSnapshot, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::save_feature(root, draft).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn delete_feature(
    feature_id: String,
    session: State<'_, Session>,
) -> Result<ProjectSnapshot, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::delete_feature(root, &feature_id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn bind_anchor(
    request: BindRequest,
    session: State<'_, Session>,
) -> Result<ProjectSnapshot, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::bind_anchor(root, request).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn remove_anchor(
    feature_id: String,
    anchor_id: String,
    session: State<'_, Session>,
) -> Result<ProjectSnapshot, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::remove_anchor(root, &feature_id, &anchor_id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn set_review(
    feature_id: String,
    state: ReviewState,
    expected_digest: Option<String>,
    session: State<'_, Session>,
) -> Result<ProjectSnapshot, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::set_review(root, &feature_id, state, expected_digest.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn get_changes(
    base: String,
    head: String,
    session: State<'_, Session>,
) -> Result<ChangeSummary, String> {
    with_root(session.inner().clone(), move |root| {
        let snapshot = reader_core::open_project(root).map_err(|e| e.to_string())?;
        reader_core::git::changes(root, &base, &head, &snapshot).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn get_file_diff(
    path: String,
    base: String,
    head: String,
    session: State<'_, Session>,
) -> Result<FileDiff, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::git::file_diff(root, &path, &base, &head).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn get_relations(
    symbol_id: String,
    base: String,
    head: String,
    semantic: bool,
    session: State<'_, Session>,
) -> Result<SymbolRelations, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::relations::analyze(root, &symbol_id, &base, &head, semantic)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
async fn export_review(session: State<'_, Session>) -> Result<String, String> {
    with_root(session.inner().clone(), move |root| {
        reader_core::export_review(root).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn journal_action(
    root: String,
    action: String,
    payload: serde_json::Value,
    session: State<'_, Session>,
) -> Result<serde_json::Value, String> {
    with_root(session.inner().clone(), move |opened| {
        if opened != Path::new(&root) {
            return Err("项目已切换；草稿仍保留在本机，请返回原项目保存".into());
        }
        let run = || -> anyhow::Result<serde_json::Value> {
            Ok(match action.as_str() {
                "traceList" => serde_json::to_value(reader_core::trace::list(opened)?)?,
                "traceRead" => serde_json::to_value(reader_core::trace::read(
                    opened,
                    payload["runId"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("缺少运行 ID"))?,
                )?)?,
                "read" => serde_json::to_value(reader_core::journal::read(opened)?)?,
                "save" => serde_json::to_value(reader_core::journal::save_annotation(
                    opened,
                    serde_json::from_value(payload["note"].clone())?,
                    payload["expectedVersion"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("缺少版本"))?,
                )?)?,
                "delete" => {
                    reader_core::journal::delete_annotation(
                        opened,
                        payload["id"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("缺少 ID"))?,
                        payload["expectedVersion"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("缺少版本"))?,
                    )?;
                    serde_json::Value::Null
                }
                "export" => serde_json::to_value(reader_core::journal::export_file(opened)?)?,
                _ => anyhow::bail!("未知记录操作"),
            })
        };
        run().map_err(|e| e.to_string())
    })
    .await
}

#[derive(Default)]
struct CollaborationHost(Mutex<Option<reader_core::collaboration::Host>>);

#[tauri::command]
async fn collaboration_action(
    root: String,
    action: String,
    payload: serde_json::Value,
    session: State<'_, Session>,
    host: State<'_, CollaborationHost>,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    let opened = session.inner().clone();
    let private = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("collaboration");
    // Serialize local host lifecycle; network operations run away from the UI thread.
    let mut request = payload;
    if action == "create" {
        let mut guard = host.0.lock().map_err(|_| "共读主持状态不可用")?;
        if guard.is_none() {
            *guard = Some(
                reader_core::collaboration::Host::start(&private.join("host"), None)
                    .map_err(|e| e.to_string())?,
            );
        }
        let service = guard.as_ref().unwrap();
        request["server"] = serde_json::json!(service.server);
        request["createKey"] = serde_json::json!(service.key);
    }
    with_root(opened, move |opened| {
        if opened != Path::new(&root) {
            return Err("项目已切换，请返回原项目操作共读".into());
        }
        reader_core::collaboration::action(opened, &private.join("connections"), &action, request)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn save_collaboration_export(
    root: String,
    owner_id: String,
    session: State<'_, Session>,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let private = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("collaboration/connections");
    with_root(session.inner().clone(), move |opened| {
        if opened != Path::new(&root) {
            return Err("项目已切换".into());
        }
        let text = reader_core::collaboration::action(
            opened,
            &private,
            "export",
            serde_json::json!({"ownerId":owner_id}),
        )
        .map_err(|e| e.to_string())?;
        let selected = app
            .dialog()
            .file()
            .set_file_name("CodeTrail-tasks.md")
            .add_filter("Markdown", &["md"])
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|e| e.to_string())?;
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            return Err("请选择 .md 文件".into());
        }
        std::fs::write(&path, text.as_str().ok_or("任务导出格式错误")?)
            .map_err(|e| e.to_string())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Session::default())
        .manage(CollaborationHost::default())
        .setup(|app| {
            if let Ok(resources) = app.path().resource_dir() {
                reader_core::semantic::set_bundled_pyright(
                    resources.join("semantic/pyright/langserver.index.js"),
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_project,
            refresh_project,
            read_file,
            save_feature,
            delete_feature,
            bind_anchor,
            remove_anchor,
            set_review,
            get_changes,
            get_file_diff,
            get_relations,
            export_review,
            journal_action,
            collaboration_action,
            save_collaboration_export
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 CodeTrail 桌面应用");
}
