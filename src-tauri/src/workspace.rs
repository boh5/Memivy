use memivy_core::{memory::*, model::ModelConfig};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager};

type HostResult<T> = std::result::Result<T, String>;
struct Workspace {
    store: MemoryStore,
    config: PathBuf,
    config_lock: Mutex<()>,
    exiting: AtomicBool,
    tasks: Mutex<HashMap<String, tokio::task::AbortHandle>>,
}
fn require(window: &tauri::WebviewWindow) -> HostResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("窗口无权执行此操作".into())
    }
}
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> memivy_core::memory::Result<T> + Send + 'static,
) -> HostResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "本地操作未完成，请重试".to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn discussion_open(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    title: String,
    context: Vec<SourceRef>,
) -> HostResult<Conversation> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || {
        let topic = s.create_conversation(&id, &title.chars().take(60).collect::<String>())?;
        let draft_key = format!("discussion:{id}");
        if s.workspace_draft(&draft_key)?.is_none() {
            s.save_workspace_draft(&WorkspaceDraft {
                key: draft_key,
                request_id: id.clone(),
                title: String::new(),
                body: String::new(),
                expected_version: None,
                context,
            })?;
        }
        Ok(topic)
    })
    .await
}
#[tauri::command]
async fn discussion_ask(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    topic_id: String,
    question: String,
    context: Vec<SourceRef>,
) -> HostResult<Conversation> {
    require(&window)?;
    validate_config(&state.config)?;
    let config = ModelConfig::read(&state.config)
        .map_err(|_| "请先连接模型；问题草稿已保留，记录与搜索仍可使用".to_string())?;
    config.endpoint().map_err(|e| e.to_string())?;
    if context.len() > 4 {
        return Err("一次最多选择 4 条记忆".into());
    }
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "讨论暂时不可用".to_string())?;
    let store = state.store.clone();
    // Retried command delivery reuses the attempt, never starts another request.
    if let Ok(old) = store.turn(&id) {
        let _ = old;
        store
            .start_turn(&id, &topic_id, &question, &context)
            .map_err(|e| e.to_string())?;
        return store.conversation(&topic_id).map_err(|e| e.to_string());
    }
    let topic = match store.conversation(&topic_id) {
        Ok(topic) => topic,
        Err(DataError::Unavailable) => store
            .create_conversation(&topic_id, &question.chars().take(60).collect::<String>())
            .map_err(|e| e.to_string())?,
        Err(e) => return Err(e.to_string()),
    };
    let turn = store
        .start_turn(&id, &topic_id, &question, &context)
        .map_err(|e| e.to_string())?;
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        if let Err(failure) = store
            .answer_discussion(&config, &topic_id, &turn, &context)
            .await
        {
            let _ = store.fail_turn(&task_id, failure);
        }
        if let Ok(mut tasks) = app.state::<Workspace>().tasks.lock() {
            tasks.remove(&task_id);
        }
        let _ = app.emit("library-refresh", ());
    });
    tasks.insert(id, task.abort_handle());
    Ok(topic)
}
#[tauri::command]
fn discussion_cancel(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
    id: String,
) -> HostResult<()> {
    require(&window)?;
    state.store.cancel_turn(&id).map_err(|e| e.to_string())?;
    if let Some(task) = state
        .tasks
        .lock()
        .map_err(|_| "讨论暂时不可用".to_string())?
        .remove(&id)
    {
        task.abort();
    }
    let _ = app.emit("library-refresh", ());
    Ok(())
}
#[tauri::command]
async fn discussion_source(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    source: SourceRef,
) -> HostResult<Evidence> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.resolve_source(&source, 4096)).await
}
#[tauri::command]
async fn discussion_save(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    request: ConclusionRequest,
) -> HostResult<Receipt> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_conclusion(&request)).await
}
#[tauri::command]
async fn library_query(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    query: LibraryQuery,
) -> HostResult<LibraryPage> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library(&query)).await
}
#[tauri::command]
async fn library_detail(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
) -> HostResult<LibraryDetail> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library_detail(&key)).await
}
#[tauri::command]
async fn library_projects(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<String>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library_projects()).await
}
#[tauri::command]
async fn library_topics(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<Conversation>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.conversations(12)).await
}
#[tauri::command]
async fn library_messages(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    before: Option<i64>,
) -> HostResult<Vec<Message>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.messages(&id, before.unwrap_or(0), 40)).await
}
#[tauri::command]
async fn discussion_messages(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    before: Option<i64>,
) -> HostResult<Vec<Message>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.recent_messages(&id, before, 40)).await
}
#[tauri::command]
async fn draft_read(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: String,
) -> HostResult<Option<WorkspaceDraft>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.workspace_draft(&key)).await
}
#[tauri::command]
async fn draft_write(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    draft: WorkspaceDraft,
) -> HostResult<()> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_workspace_draft(&draft)).await
}
#[tauri::command]
async fn draft_clear(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: String,
) -> HostResult<()> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.delete_workspace_draft(&key)).await
}
#[tauri::command]
async fn library_capture(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    request: CaptureRequest,
) -> HostResult<RawCapture> {
    require(&window)?;
    if !matches!(&request.origin,Origin::User {app, ..} if app=="Memivy") {
        return Err("主窗口只能保存你主动输入的记录".into());
    }
    let s = state.store.clone();
    blocking(move || s.capture(&request)).await
}
#[tauri::command]
async fn library_edit(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    draft: WorkspaceDraft,
) -> HostResult<Receipt> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_library_edit(&draft)).await
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Trash {
        key: RecordKey,
        expected: Option<String>,
    },
    Restore {
        key: RecordKey,
    },
    Purge {
        key: RecordKey,
    },
    RestoreVersion {
        request_id: String,
        memory_id: String,
        expected: String,
        version_id: String,
    },
    Undo {
        request_id: String,
        original_request: String,
    },
}
#[tauri::command]
async fn library_action(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    action: Action,
) -> HostResult<Option<Receipt>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || {
        match action {
            Action::Trash { key, expected } => match key.kind.as_str() {
                "memory" => s.trash_memory(&key.id, &expected.ok_or(DataError::Invalid)?)?,
                "capture" => s.trash_capture(&key.id)?,
                _ => return Err(DataError::Invalid),
            },
            Action::Restore { key } => match key.kind.as_str() {
                "memory" => s.restore_memory(&key.id)?,
                "capture" => s.restore_capture(&key.id)?,
                _ => return Err(DataError::Invalid),
            },
            Action::Purge { key } => match key.kind.as_str() {
                "memory" => s.purge_memory(&key.id)?,
                "capture" => s.purge_capture(&key.id)?,
                _ => return Err(DataError::Invalid),
            },
            Action::RestoreVersion {
                request_id,
                memory_id,
                expected,
                version_id,
            } => {
                return s
                    .restore_version(&request_id, &memory_id, &expected, &version_id)
                    .map(Some);
            }
            Action::Undo {
                request_id,
                original_request,
            } => return s.undo(&request_id, &original_request).map(Some),
        }
        Ok(None)
    })
    .await
}
#[tauri::command]
async fn library_rebuild(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<()> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.rebuild_search_index()).await
}
#[tauri::command]
async fn memory_export(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
    expected_version: Option<String>,
) -> HostResult<Option<String>> {
    require(&window)?;
    let s = state.store.clone();
    let selected = key.clone();
    let detail = blocking(move || s.library_detail(&selected)).await?;
    let name: String = detail
        .title
        .chars()
        .take(60)
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    let filename = format!("{}.md", if name.is_empty() { "记忆" } else { name });
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
        use objc2_foundation::{NSArray, NSString};
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        let panel = NSSavePanel::savePanel(mtm);
        panel.setCanCreateDirectories(true);
        panel.setNameFieldStringValue(&NSString::from_str(&filename));
        // Keep the native extension/overwrite UI without another framework dependency.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str(
            "md",
        )])));
        panel.setExtensionHidden(false);
        panel.setMessage(Some(&NSString::from_str(
            "将这一篇的标题和当前正文保存为 Markdown。",
        )));
        let path = if panel.runModal() == NSModalResponseOK {
            panel
                .URL()
                .and_then(|u| u.path())
                .map(|s| PathBuf::from(s.to_string()))
        } else {
            None
        };
        let _ = tx.send(path);
    })
    .map_err(|_| "无法打开保存窗口".to_string())?;
    let Some(target) = rx.await.map_err(|_| "文件选择未完成".to_string())? else {
        return Ok(None);
    };
    let s = state.store.clone();
    blocking(move || {
        s.export_record_markdown(&key, expected_version.as_deref(), &target)?;
        Ok(Some(target.to_string_lossy().into_owned()))
    })
    .await
}
fn validate_config(path: &std::path::Path) -> HostResult<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    crate::storage::validate_config_path(path, home.as_deref())
}
#[derive(Serialize, Default)]
struct Settings {
    base_url: String,
    model: String,
    has_key: bool,
    configured: bool,
    disable_reasoning: bool,
}
#[tauri::command]
fn workspace_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
) -> HostResult<Settings> {
    require(&window)?;
    validate_config(&state.config)?;
    if !state.config.exists() {
        return Ok(Settings::default());
    }
    let c = ModelConfig::read(&state.config).map_err(|e| e.to_string())?;
    Ok(Settings {
        configured: true,
        base_url: c.base_url,
        model: c.model,
        has_key: c.api_key.is_some_and(|s| !s.is_empty()),
        disable_reasoning: c.disable_reasoning,
    })
}
#[tauri::command]
fn workspace_configure(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
    base_url: String,
    model: String,
    api_key: Option<String>,
    disable_reasoning: bool,
    replace_unreadable: bool,
) -> HostResult<()> {
    require(&window)?;
    let _lock = state
        .config_lock
        .lock()
        .map_err(|_| "配置正忙".to_string())?;
    let path = &state.config;
    validate_config(path)?;
    let previous = if path.exists() {
        match ModelConfig::read(path) {
            Ok(c) => Some(c),
            Err(_) if replace_unreadable => None,
            Err(e) => return Err(e.to_string()),
        }
    } else {
        None
    };
    let key = api_key.or_else(|| {
        previous.and_then(|c| {
            if c.base_url == base_url {
                c.api_key
            } else {
                None
            }
        })
    });
    ModelConfig {
        base_url,
        model,
        api_key: key,
        disable_reasoning,
    }
    .save(path)
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn workspace_test_model(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<memivy_core::model::ProbeReport> {
    require(&window)?;
    validate_config(&state.config)?;
    let c = ModelConfig::read(&state.config).map_err(|e| e.to_string())?;
    memivy_core::model::probe(c, std::time::Duration::from_secs(45))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn workspace_close(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
) -> HostResult<()> {
    require(&window)?;
    state.exiting.store(true, Ordering::Relaxed);
    app.exit(0);
    Ok(())
}
pub fn run(context: tauri::Context<tauri::Wry>) {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let store = match std::env::var_os("MEMIVY_DATA_DIR") {
                Some(p) => MemoryStore::open(PathBuf::from(p))?,
                None => MemoryStore::open_default()?,
            };
            let config = std::env::var_os("MEMIVY_MODEL_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|| store.model_config_path());
            store.recover_interrupted_turns()?;
            // Carry forward an existing local setup only for the normal app,
            // never for an isolated QA directory or an explicit config override.
            if !config.exists()
                && std::env::var_os("MEMIVY_DATA_DIR").is_none()
                && std::env::var_os("MEMIVY_MODEL_CONFIG").is_none()
                && let Some(home) = std::env::var_os("HOME")
            {
                let previous = PathBuf::from(home)
                    .join("Library/Application Support/com.memivy.phase1/model.json");
                if validate_config(&previous).is_ok()
                    && validate_config(&config).is_ok()
                    && let Ok(settings) = ModelConfig::read(&previous)
                {
                    let _ = settings.save(&config);
                }
            }
            app.manage(Workspace {
                store,
                config,
                config_lock: Mutex::new(()),
                exiting: AtomicBool::new(false),
                tasks: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("workspace-close-request", ());
            }
            if let tauri::WindowEvent::Focused(true) = event {
                let _ = window.emit("library-refresh", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            library_query,
            discussion_open,
            discussion_messages,
            discussion_ask,
            discussion_cancel,
            discussion_source,
            discussion_save,
            library_detail,
            library_projects,
            library_topics,
            library_messages,
            draft_read,
            draft_write,
            draft_clear,
            library_capture,
            library_edit,
            library_action,
            library_rebuild,
            memory_export,
            workspace_settings,
            workspace_configure,
            workspace_test_model,
            workspace_close
        ])
        .build(context);
    let app = match result {
        Ok(app) => app,
        Err(_) => {
            eprintln!("Memivy 启动失败，请检查本地环境和数据目录");
            std::process::exit(1);
        }
    };
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && let Some(state) = app.try_state::<Workspace>()
            && !state.exiting.load(Ordering::Relaxed)
        {
            api.prevent_exit();
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.emit("workspace-close-request", ());
            }
        }
    });
}
