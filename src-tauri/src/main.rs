use memivy_core::{Capture, CaptureInput, DataPaths, SearchPage, Store};
use serde::Serialize;
use std::{sync::Mutex, time::Instant};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_nspanel::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
mod capture_panel;

#[derive(Default)]
struct WindowState {
    source: String,
    previous_pid: Option<i32>,
    opened: Option<Instant>,
    last_focus_ms: Option<f64>,
    last_commit_ms: Option<f64>,
    last_submit_roundtrip_ms: Option<f64>,
    shortcut_error: Option<String>,
    last_open_kind: Option<&'static str>,
    last_ready_native_focus: Option<bool>,
    capture_events: Vec<String>,
}
struct Runtime {
    store: Store,
    windows: Mutex<WindowState>,
}
type HostResult<T> = Result<T, String>;

impl WindowState {
    fn remember_origin(&mut self, kind: &str, visible: bool, source: String, pid: Option<i32>) {
        // A main-window action owns its return target even if the capture window
        // was already open, or macOS has not updated frontmostApplication yet.
        if kind == "button" {
            self.source = "Memivy Phase 1".into();
            self.previous_pid = Some(std::process::id() as i32);
        } else if !visible {
            self.source = source;
            self.previous_pid = pid;
        }
    }
}

fn require(window: &tauri::WebviewWindow, labels: &[&str]) -> HostResult<()> {
    if labels.contains(&window.label()) {
        Ok(())
    } else {
        Err("窗口无权执行此操作".into())
    }
}
fn reveal_capture(app: &tauri::AppHandle, kind: &'static str) -> HostResult<()> {
    let started = Instant::now();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window("capture") else {
            return;
        };
        let Ok(panel) = handle.get_webview_panel("capture") else {
            return;
        };
        let state = handle.state::<Runtime>();
        let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication();
        let visible = panel.is_visible();
        {
            let mut context = state.windows.lock().unwrap();
            context.remember_origin(
                kind,
                visible,
                front
                    .as_ref()
                    .and_then(|app| app.localizedName())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "未知应用".into()),
                front.as_ref().map(|app| app.processIdentifier()),
            );
            context.opened = Some(started);
            context.last_focus_ms = None;
            context.last_ready_native_focus = None;
            context.last_open_kind = Some(kind);
            context.capture_events.clear();
        }
        if let Ok(point) = window.cursor_position()
            && let Ok(Some(monitor)) = handle.monitor_from_point(point.x, point.y)
        {
            let size = monitor.size();
            let origin = monitor.position();
            let scale = monitor.scale_factor();
            let x = origin.x + ((size.width as f64 - 560.0 * scale) / 2.0) as i32;
            let y = origin.y + ((size.height as f64 - 350.0 * scale) / 3.0) as i32;
            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        }
        // A nonactivating panel takes keyboard focus without activating the
        // whole app or moving away from the user's current fullscreen Space.
        panel.show_and_make_key();
        if window.as_ref().set_focus().is_err() {
            state.windows.lock().unwrap().shortcut_error =
                Some("捕捉窗口未能获得焦点，请从菜单栏重试".into());
        }
        let _ = window.emit("capture-open", ());
    })
    .map_err(|_| "无法唤起捕捉窗口".into())
}

#[tauri::command]
fn show_capture(app: tauri::AppHandle, window: tauri::WebviewWindow) -> HostResult<()> {
    require(&window, &["main"])?;
    reveal_capture(&app, "button")
}
#[tauri::command]
async fn verify_external_capture(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<()> {
    require(&window, &["main"])?;
    // Phase 1 diagnostic entry: gives native UI automation time to activate an
    // external fullscreen app. Does not claim to exercise the global hotkey.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    reveal_capture(&app, "verification")
}
#[tauri::command]
fn capture_context(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
) -> HostResult<String> {
    require(&window, &["capture"])?;
    Ok(state.windows.lock().unwrap().source.clone())
}
#[tauri::command]
fn capture_ready(app: tauri::AppHandle, window: tauri::WebviewWindow) -> HostResult<()> {
    require(&window, &["capture"])?;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let focused = handle
            .get_webview_panel("capture")
            .is_ok_and(|panel| panel.as_panel().isKeyWindow());
        let state = handle.state::<Runtime>();
        let mut context = state.windows.lock().unwrap();
        eprintln!("phase1-capture-ready: native_focus={focused}");
        context.last_ready_native_focus = Some(focused);
        if focused && let Some(start) = context.opened.take() {
            context.last_focus_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
        }
    })
    .map_err(|_| "无法检查输入焦点".into())
}
#[tauri::command]
fn capture_trace(
    window: tauri::WebviewWindow,
    state: tauri::State<Runtime>,
    event: String,
    at_ms: f64,
) -> HostResult<()> {
    require(&window, &["capture"])?;
    // Prototype-only, bounded event names in RAM; never collect keys or text.
    if !matches!(
        event.as_str(),
        "esc-down" | "esc-up" | "esc-click" | "input-blur" | "input-focus" | "dismiss"
    ) || !at_ms.is_finite()
        || at_ms < 0.0
    {
        return Err("无效的窗口事件".into());
    }
    let entry = format!("{at_ms:.3} {event}");
    eprintln!("phase1-capture-event: {entry}");
    let mut context = state.windows.lock().unwrap();
    if context.capture_events.len() == 24 {
        context.capture_events.remove(0);
    }
    context.capture_events.push(entry);
    Ok(())
}
#[tauri::command]
async fn capture_hide(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
    submit_roundtrip_ms: Option<f64>,
) -> HostResult<()> {
    require(&window, &["capture"])?;
    if let Some(ms) = submit_roundtrip_ms.filter(|v| v.is_finite() && *v >= 0.0 && *v < 60_000.0) {
        state.windows.lock().unwrap().last_submit_roundtrip_ms = Some(ms);
    }
    let pid = state.windows.lock().unwrap().previous_pid;
    let handle = app.clone();
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    app.run_on_main_thread(move || {
        let Ok(panel) = handle.get_webview_panel("capture") else {
            let _ = sender.try_send(Err("捕捉面板不可用".into()));
            return;
        };
        panel.hide();
        let result =
            if pid == Some(std::process::id() as i32) {
                if let Some(main) = handle.get_webview_window("main") {
                    main.show().and_then(|_| main.set_focus())
                } else {
                    Ok(())
                }
            } else {
                let front_pid = objc2_app_kit::NSWorkspace::sharedWorkspace()
                    .frontmostApplication()
                    .map(|app| app.processIdentifier());
                if front_pid != pid && let Some(previous) = pid.and_then(
                objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier,
            ) {
                previous
                    .activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
            }
                Ok(())
            };
        let _ = sender.try_send(result.map_err(|_| "收起小窗或恢复窗口失败".to_string()));
    })
    .map_err(|_| "无法恢复原应用焦点".to_string())?;
    receiver
        .recv()
        .await
        .ok_or_else(|| "窗口任务未完成".to_string())?
}

#[tauri::command]
async fn capture_save(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
    request_id: String,
    text: String,
    attach_source: bool,
) -> HostResult<Capture> {
    require(&window, &["capture"])?;
    let start = Instant::now();
    let store = state.store.clone();
    let source_app = if attach_source {
        state.windows.lock().unwrap().source.clone()
    } else {
        "Memivy Phase 1".into()
    };
    let capture = tauri::async_runtime::spawn_blocking(move || {
        store.capture(CaptureInput {
            request_id,
            text,
            source_app,
            project: None,
            session_uri: None,
        })
    })
    .await
    .map_err(|_| "本地保存任务未完成".to_string())?
    .map_err(|e| e.to_string())?;
    state.windows.lock().unwrap().last_commit_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
    let _ = app.emit_to("main", "capture-saved", &capture);
    Ok(capture)
}
#[tauri::command]
async fn capture_search(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
    query: String,
) -> HostResult<SearchPage> {
    require(&window, &["main"])?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.search(&query, 50))
        .await
        .map_err(|_| "搜索任务未完成".to_string())?
        .map_err(|e| e.to_string())
}
#[derive(Serialize)]
struct HostDiagnostics {
    #[serde(flatten)]
    database: memivy_core::Diagnostics,
    shortcut_error: Option<String>,
    last_focus_ms: Option<f64>,
    last_commit_ms: Option<f64>,
    last_submit_roundtrip_ms: Option<f64>,
    monitors: usize,
    last_open_kind: Option<&'static str>,
    last_ready_native_focus: Option<bool>,
    capture_events: Vec<String>,
    capture_monitor: Option<String>,
    capture_position: Option<tauri::PhysicalPosition<i32>>,
}
#[tauri::command]
async fn diagnostics(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
) -> HostResult<HostDiagnostics> {
    require(&window, &["main"])?;
    let store = state.store.clone();
    let database = tauri::async_runtime::spawn_blocking(move || store.diagnostics())
        .await
        .map_err(|_| "诊断读取未完成".to_string())?
        .map_err(|e| e.to_string())?;
    // Native window calls may dispatch to the main thread. Never hold shared state
    // while waiting: the shortcut handler on that thread also needs this lock.
    let monitors = app.available_monitors().map(|m| m.len()).unwrap_or(0);
    let capture = app.get_webview_window("capture");
    let capture_monitor = capture
        .as_ref()
        .and_then(|w| w.current_monitor().ok().flatten())
        .and_then(|m| m.name().cloned());
    let capture_position = capture.as_ref().and_then(|w| w.outer_position().ok());
    let context = state.windows.lock().unwrap();
    Ok(HostDiagnostics {
        database,
        shortcut_error: context.shortcut_error.clone(),
        last_focus_ms: context.last_focus_ms,
        last_commit_ms: context.last_commit_ms,
        last_submit_roundtrip_ms: context.last_submit_roundtrip_ms,
        monitors,
        last_open_kind: context.last_open_kind,
        last_ready_native_focus: context.last_ready_native_focus,
        capture_events: context.capture_events.clone(),
        capture_monitor,
        capture_position,
    })
}
#[tauri::command]
async fn set_mcp_enabled(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Runtime>,
    enabled: bool,
) -> HostResult<()> {
    require(&window, &["main"])?;
    let paths = state.store.paths.clone();
    tauri::async_runtime::spawn_blocking(move || paths.set_mcp_enabled(enabled))
        .await
        .map_err(|_| "配置保存未完成".to_string())?
        .map_err(|e| e.to_string())
}

fn main() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_nspanel::init())
        .setup(|app| {
            let store = Store::open(DataPaths::resolve()?)?;
            app.manage(Runtime {
                store,
                windows: Mutex::new(WindowState::default()),
            });
            capture_panel::configure(app.handle())?;
            let open = MenuItem::with_id(app, "open", "打开阶段一样机", true, None::<&str>)?;
            let capture = MenuItem::with_id(app, "capture", "记一下 · ⌃⌥ M", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "彻底退出样机", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &capture, &quit])?;
            TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../../design-demo/brand/memivy-icon-1024.png"
                ))?)
                .tooltip("Memivy · 阶段 1 技术样机")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "capture" => {
                        let _ = reveal_capture(app, "tray");
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            if app
                .global_shortcut()
                .on_shortcut("Control+Alt+KeyM", |app, _, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = reveal_capture(app, "shortcut");
                    }
                })
                .is_err()
            {
                app.state::<Runtime>()
                    .windows
                    .lock()
                    .unwrap()
                    .shortcut_error = Some("⌃⌥ M 注册失败，可能被占用；请使用菜单栏入口".into());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            show_capture,
            verify_external_capture,
            capture_context,
            capture_ready,
            capture_trace,
            capture_hide,
            capture_save,
            capture_search,
            diagnostics,
            set_mcp_enabled
        ])
        .run(tauri::generate_context!());
    if result.is_err() {
        eprintln!("Memivy 阶段一样机启动失败，请检查本地环境和数据目录");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_button_replaces_external_return_target_even_while_capture_is_visible() {
        let mut state = WindowState::default();
        state.remember_origin("shortcut", false, "Other app".into(), Some(-1));
        state.remember_origin("button", true, "Stale foreground app".into(), Some(-2));
        assert_eq!(state.previous_pid, Some(std::process::id() as i32));
        assert_eq!(state.source, "Memivy Phase 1");
    }

    #[test]
    fn repeated_shortcut_keeps_origin_until_a_new_capture_session() {
        let mut state = WindowState::default();
        state.remember_origin("shortcut", false, "First app".into(), Some(-1));
        state.remember_origin(
            "shortcut",
            true,
            "Memivy".into(),
            Some(std::process::id() as i32),
        );
        assert_eq!(state.previous_pid, Some(-1));
        state.remember_origin("shortcut", false, "Next app".into(), Some(-2));
        assert_eq!(state.previous_pid, Some(-2));
        assert_eq!(state.source, "Next app");
    }
}
