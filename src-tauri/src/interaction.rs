use super::*;
use crate::storage::validate_config_path;
use memivy_core::{
    conversation::{self, Receipt, Thread, Topic},
    model::ModelConfig,
};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    pub topic_id: Option<String>,
    pub mode: String,
    pub draft: String,
    pub pinned: Vec<String>,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            topic_id: None,
            mode: "capture".into(),
            draft: String::new(),
            pinned: Vec::new(),
        }
    }
}
pub fn resize_panel(app: &tauri::AppHandle, width: f64, height: f64) {
    let Some(w) = app.get_webview_window("capture") else {
        return;
    };
    let Ok(Some(monitor)) = w.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let old_size = w.outer_size().unwrap_or(tauri::PhysicalSize::new(72, 76));
    let pos = w.outer_position().unwrap_or(*monitor.position());
    let origin = monitor.position();
    let bounds = monitor.size();
    let width = width.min(bounds.width as f64 / scale - 24.0);
    let height = height.min(bounds.height as f64 / scale - 80.0);
    let x = (pos.x + old_size.width as i32 - (width * scale) as i32).clamp(
        origin.x + 8,
        (origin.x + bounds.width as i32 - (width * scale) as i32 - 8).max(origin.x + 8),
    );
    let y = (pos.y + old_size.height as i32 - (height * scale) as i32).clamp(
        origin.y + (36.0 * scale) as i32,
        (origin.y + bounds.height as i32 - (height * scale) as i32 - 16)
            .max(origin.y + (36.0 * scale) as i32),
    );
    let _ = w.set_size(tauri::LogicalSize::new(width, height));
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}
pub fn place_companion(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("capture") {
        if let Ok(Some(m)) = w.current_monitor() {
            let s = m.scale_factor();
            let _ = w.set_position(tauri::PhysicalPosition::new(
                m.position().x + m.size().width as i32 - (100.0 * s) as i32,
                m.position().y + m.size().height as i32 - (155.0 * s) as i32,
            ));
        }
        if let Ok(p) = app.get_webview_panel("capture") {
            p.order_front_regardless();
        }
    }
}

/// Main thread only. orderOut lets AppKit release nonactivating-panel key focus;
/// resignKeyWindow is a notification override, not a focus-transfer operation.
pub fn collapse_companion(app: &tauri::AppHandle) -> HostResult<()> {
    let panel = app
        .get_webview_panel("capture")
        .map_err(|_| "捕捉面板不可用")?;
    panel.hide();
    capture_panel::set_accepts_keyboard(false);
    {
        let state = app.state::<Runtime>();
        let mut windows = state.windows.lock().unwrap();
        windows.expanded = false;
        windows.drag_origin = None;
    }
    resize_panel(app, 72.0, 76.0);
    if let Some(window) = app.get_webview_window("capture") {
        let _ = window.emit("companion-collapsed", ());
    }
    panel.order_front_regardless();
    Ok(())
}

/// Main thread only. Finish dismissing the panel before activating the editor.
pub fn focus_workspace(app: &tauri::AppHandle) -> HostResult<()> {
    collapse_companion(app)?;
    let window = app.get_webview_window("main").ok_or("主窗口不可用")?;
    window
        .show()
        .and_then(|_| window.set_focus())
        .and_then(|_| window.as_ref().set_focus())
        .map_err(|_| "无法打开工作区".into())
}
#[tauri::command]
pub fn companion_open(app: tauri::AppHandle, window: tauri::WebviewWindow) -> HostResult<()> {
    require(&window, &["capture"])?;
    reveal_capture(&app, "companion")
}
#[tauri::command]
pub fn companion_resize(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    answer: bool,
) -> HostResult<()> {
    require(&window, &["capture"])?;
    let h = app.clone();
    app.run_on_main_thread(move || {
        if h.state::<Runtime>().windows.lock().unwrap().expanded {
            resize_panel(&h, 440.0, if answer { 590.0 } else { 300.0 });
        }
    })
    .map_err(|_| "窗口无法展开".into())
}
#[tauri::command]
pub fn companion_drag_start(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<()> {
    require(&window, &["capture"])?;
    let position = window.outer_position().map_err(|_| "无法读取助手位置")?;
    let cursor = window.cursor_position().map_err(|_| "无法读取拖动位置")?;
    state.windows.lock().unwrap().drag_origin = Some((position, cursor));
    Ok(())
}
#[tauri::command]
pub fn companion_drag(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<()> {
    require(&window, &["capture"])?;
    let Some((position, origin)) = state.windows.lock().unwrap().drag_origin else {
        return Ok(());
    };
    // Use global physical coordinates for both the cursor and native window.
    // NSPanel/WebKit IPC may outlive the mouse-down event required by AppKit's
    // performWindowDragWithEvent. Pointer capture supplies this gesture instead.
    let cursor = window.cursor_position().map_err(|_| "无法读取拖动位置")?;
    window
        .set_position(tauri::PhysicalPosition::new(
            position.x + (cursor.x - origin.x).round() as i32,
            position.y + (cursor.y - origin.y).round() as i32,
        ))
        .map_err(|_| "暂时无法移动助手".into())
}
#[tauri::command]
pub fn companion_drag_end(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<()> {
    require(&window, &["capture"])?;
    state.windows.lock().unwrap().drag_origin = None;
    Ok(())
}
#[tauri::command]
pub fn companion_hide(app: tauri::AppHandle, window: tauri::WebviewWindow) -> HostResult<()> {
    require(&window, &["capture", "main"])?;
    let h = app.clone();
    app.run_on_main_thread(move || {
        if let Ok(p) = h.get_webview_panel("capture") {
            p.hide();
        }
    })
    .map_err(|_| "暂时无法隐藏".into())
}
#[tauri::command]
pub fn open_workspace(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    view: ViewState,
) -> HostResult<()> {
    require(&window, &["capture", "main"])?;
    if view.draft.len() > 16_384
        || view.pinned.len() > 4
        || !matches!(view.mode.as_str(), "capture" | "ask")
    {
        return Err("输入过长".into());
    }
    *state.view.lock().unwrap() = view.clone();
    if let Some(id) = &view.topic_id {
        state
            .store
            .save_draft(id, &view.draft)
            .map_err(|e| e.to_string())?;
    }
    let h = app.clone();
    app.run_on_main_thread(move || {
        if focus_workspace(&h).is_ok()
            && let Some(w) = h.get_webview_window("main")
        {
            let _ = w.emit("view-open", &view);
        }
    })
    .map_err(|_| "无法打开工作区".into())
}
#[tauri::command]
pub fn view_state(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<ViewState> {
    require(&window, &["main", "capture"])?;
    Ok(state.view.lock().unwrap().clone())
}

#[tauri::command]
pub fn focus_editor(window: tauri::WebviewWindow) -> HostResult<()> {
    require(&window, &["main", "capture"])?;
    // Restore WebKit's native first responder before focusing a DOM editor.
    // This does not activate the app or change the key window.
    window
        .as_ref()
        .set_focus()
        .map_err(|_| "无法聚焦输入区".into())
}
fn changed(app: &tauri::AppHandle) {
    let _ = app.emit("workspace-changed", ());
}
#[tauri::command]
pub fn workspace_topics(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<Vec<Topic>> {
    require(&window, &["main", "capture"])?;
    state.store.topics().map_err(|e| e.to_string())
}
#[tauri::command]
pub fn workspace_thread(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    id: String,
) -> HostResult<Thread> {
    require(&window, &["main", "capture"])?;
    state.store.thread(&id).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn workspace_draft(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    view: ViewState,
) -> HostResult<()> {
    require(&window, &["main", "capture"])?;
    if view.draft.len() > 16_384 || view.pinned.len() > 4 {
        return Err("草稿过长".into());
    }
    if let Some(id) = &view.topic_id {
        state
            .store
            .save_draft(id, &view.draft)
            .map_err(|e| e.to_string())?;
    }
    *state.view.lock().unwrap() = view;
    Ok(())
}
fn config_path(state: &Runtime) -> PathBuf {
    std::env::var_os("MEMIVY_PHASE1_MODEL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| state.store.paths.model_config())
}

#[derive(Serialize)]
pub struct Settings {
    base_url: String,
    model: String,
    has_key: bool,
    configured: bool,
    local: bool,
    disable_reasoning: bool,
}
#[tauri::command]
pub fn model_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<Settings> {
    require(&window, &["main", "capture"])?;
    match ModelConfig::read(&config_path(&state)) {
        Ok(c) => Ok(Settings {
            local: c.endpoint().is_ok_and(|(_, local)| local),
            base_url: c.base_url,
            model: c.model,
            has_key: c.api_key.is_some_and(|k| !k.is_empty()),
            configured: true,
            disable_reasoning: c.disable_reasoning,
        }),
        Err(_) => Ok(Settings {
            base_url: String::new(),
            model: String::new(),
            has_key: false,
            configured: false,
            local: false,
            disable_reasoning: false,
        }),
    }
}
#[tauri::command]
pub fn save_model_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    base_url: String,
    model: String,
    api_key: Option<String>,
    disable_reasoning: bool,
) -> HostResult<()> {
    require(&window, &["main"])?;
    let path = config_path(&state);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    validate_config_path(&path, home.as_deref())?;
    let previous = ModelConfig::read(&path).ok();
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
    .save(&path)
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn test_model(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
) -> HostResult<memivy_core::model::ProbeReport> {
    require(&window, &["main"])?;
    let config = ModelConfig::read(&config_path(&state)).map_err(|e| e.to_string())?;
    memivy_core::model::probe(config, std::time::Duration::from_secs(45))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn ask_memory(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
    id: String,
    topic_id: String,
    question: String,
    pinned: Vec<String>,
) -> HostResult<()> {
    require(&window, &["main", "capture"])?;
    let config = ModelConfig::read(&config_path(&state))
        .map_err(|_| "先在模型设置中连接一个模型；原话仍可正常保存".to_string())?;
    config.endpoint().map_err(|e| e.to_string())?;
    if pinned.len() > 4 {
        return Err("一次最多选择 4 条记忆".into());
    }
    state
        .store
        .create_topic(&topic_id, &question)
        .map_err(|e| e.to_string())?;
    let turn = state
        .store
        .begin_turn(&id, &topic_id, &question)
        .map_err(|e| e.to_string())?;
    let store = state.store.clone();
    let task_id = id.clone();
    let h = app.clone();
    let task = tokio::spawn(async move {
        match conversation::answer_question(&store, &config, &turn, &pinned).await {
            Ok((answer, evidence)) => {
                if let Err(e) = store.finish_turn(&task_id, &answer, &evidence) {
                    let _ = store.stop_turn(&task_id, false, &e.to_string());
                }
            }
            Err(e) => {
                let _ = store.stop_turn(&task_id, false, &e);
            }
        }
        h.state::<Runtime>().tasks.lock().unwrap().remove(&task_id);
        changed(&h);
    });
    state.tasks.lock().unwrap().insert(id, task.abort_handle());
    changed(&app);
    Ok(())
}
#[tauri::command]
pub fn cancel_answer(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    id: String,
) -> HostResult<()> {
    require(&window, &["main", "capture"])?;
    state
        .store
        .stop_turn(&id, true, "已停止，问题已保留")
        .map_err(|e| e.to_string())?;
    if let Some(task) = state.tasks.lock().unwrap().remove(&id) {
        task.abort();
    }
    changed(&app);
    Ok(())
}
#[tauri::command]
pub fn confirm_conclusion(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    request_id: String,
    turn_id: String,
    title: String,
    text: String,
) -> HostResult<Receipt> {
    require(&window, &["main", "capture"])?;
    let receipt = state
        .store
        .save_conclusion(&request_id, &turn_id, &title, &text)
        .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(receipt)
}
#[tauri::command]
pub fn undo_conclusion(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    id: String,
) -> HostResult<()> {
    require(&window, &["main", "capture"])?;
    state
        .store
        .undo_conclusion(&id)
        .map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}
#[tauri::command]
pub fn memory_source(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    id: String,
) -> HostResult<Capture> {
    require(&window, &["main", "capture"])?;
    state.store.capture_by_id(&id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod model_path_tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn home_dotfiles_repository_allows_app_settings_to_save_and_reopen() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(".git")).unwrap();
        let path = home
            .path()
            .join("Library/Application Support/com.memivy.phase1/model.json");
        validate_config_path(&path, Some(home.path())).unwrap();
        let config = ModelConfig {
            base_url: "http://127.0.0.1:11435/v1".into(),
            model: "test-only".into(),
            api_key: Some("synthetic-test-key".into()),
            disable_reasoning: false,
        };
        config.save(&path).unwrap();
        assert_eq!(ModelConfig::read(&path).unwrap().api_key, config.api_key);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        validate_config_path(&path, Some(home.path())).unwrap();
    }

    #[test]
    fn repositories_and_worktrees_inside_app_data_are_still_rejected() {
        let home = tempfile::tempdir().unwrap();
        let app_data = home
            .path()
            .join("Library/Application Support/com.memivy.phase1");
        for repo in [home.path().join("Developer/project"), app_data] {
            fs::create_dir_all(&repo).unwrap();
            // Linked worktrees use a .git file rather than a directory.
            fs::write(repo.join(".git"), "gitdir: /unused-test-path").unwrap();
            assert!(
                validate_config_path(&repo.join("research/model.json"), Some(home.path())).is_err()
            );
        }
        fs::create_dir(home.path().join(".git")).unwrap();
        assert!(validate_config_path(&home.path().join("model.json"), Some(home.path())).is_err());
    }

    #[test]
    fn symlink_into_repository_cannot_use_the_app_data_exception() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("Developer/project");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir(repo.join("research")).unwrap();
        let support = home.path().join("Library/Application Support");
        fs::create_dir_all(&support).unwrap();
        std::os::unix::fs::symlink(repo.join("research"), support.join("com.memivy.phase1"))
            .unwrap();
        assert!(
            validate_config_path(
                &support.join("com.memivy.phase1/model.json"),
                Some(home.path())
            )
            .is_err()
        );
    }

    #[test]
    fn external_config_accepts_new_directories_but_not_relative_or_parent_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_config_path(&dir.path().join("new/private/model.json"), None).is_ok());
        assert!(validate_config_path(Path::new("model.json"), None).is_err());
        assert!(validate_config_path(&dir.path().join("new/../model.json"), None).is_err());
    }
}
