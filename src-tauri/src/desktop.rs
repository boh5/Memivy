//! Formal macOS entry points. Window state is not a second memory store.
use crate::{
    capture_panel,
    workspace::{HostResult, Workspace},
};
use memivy_core::memory::{CaptureRequest, CaptureResult, Conversation, Origin, RecordKey};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::PathBuf, sync::Mutex, time::Instant};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tauri_nspanel::ManagerExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

mod termination;

mod panel_events {
    // The pinned panel-event macro requires and repeats an explicit return type.
    #![allow(clippy::unused_unit)]
    tauri_nspanel::tauri_panel! {
        panel_event!(DesktopPanelEvents {
            window_did_resign_key(notification: &NSNotification) -> ()
        })
    }

    pub(super) fn install(app: &tauri::AppHandle) {
        use tauri::Emitter;
        use tauri_nspanel::ManagerExt;
        let handler = DesktopPanelEvents::new();
        let h = app.clone();
        handler.window_did_resign_key(move |_| {
            let _ = h.emit_to("capture", "desktop-blur", ());
        });
        app.get_webview_panel("capture")
            .unwrap()
            .set_event_handler(Some(handler.as_ref()));
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
struct Preferences {
    shortcut: String,
    visible: bool,
    paused: bool,
    pinned: bool,
    mode: String,
    topic_id: Option<String>,
    source_app: String,
    position: Option<(i32, i32)>,
    last_memory: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            shortcut: "Control+Super+KeyM".into(),
            visible: true,
            paused: false,
            pinned: false,
            mode: "capture".into(),
            topic_id: None,
            source_app: "Memivy".into(),
            position: None,
            last_memory: None,
        }
    }
}
pub struct Desktop {
    path: PathBuf,
    inner: Mutex<State>,
}
struct State {
    sequence: u64,
    topic: Option<Conversation>,
    configured: bool,
    prefs: Preferences,
    expanded: bool,
    generation: u64,
    previous_pid: Option<i32>,
    opened: Option<Instant>,
    ready_ms: Option<u128>,
    save_ms: Option<u128>,
    error: Option<String>,
    drag: Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalPosition<f64>)>,
    ready_windows: HashSet<String>,
    quit: Option<(u64, HashSet<String>)>,
    next_quit: u64,
    shortcut_down: bool,
    receipt: Option<String>,
    handoff: Option<u64>,
    modal_open: bool,
}
#[derive(Serialize, Clone)]
pub struct Snapshot {
    pub sequence: u64,
    pub expanded: bool,
    pub generation: u64,
    pub pinned: bool,
    pub visible: bool,
    pub paused: bool,
    pub shortcut: String,
    pub mode: String,
    pub configured: bool,
    pub topic: Option<Conversation>,
    pub source_app: String,
    pub last_memory: Option<String>,
    pub error: Option<String>,
    pub ready_ms: Option<u128>,
    pub save_ms: Option<u128>,
    pub receipt: bool,
}
impl Desktop {
    pub fn new(path: PathBuf) -> Self {
        let (prefs, error) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(p) => (p, None),
                Err(_) => (
                    Preferences::default(),
                    Some("快捷入口设置无法读取，请在设置中重新保存。".into()),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Preferences::default(), None),
            Err(_) => (
                Preferences::default(),
                Some("快捷入口设置无法读取，请检查本机目录权限。".into()),
            ),
        };
        Self {
            path,
            inner: Mutex::new(State {
                sequence: 0,
                topic: None,
                configured: false,
                prefs,
                expanded: false,
                generation: 0,
                previous_pid: None,
                opened: None,
                ready_ms: None,
                save_ms: None,
                error,
                drag: None,
                ready_windows: HashSet::new(),
                quit: None,
                next_quit: 0,
                shortcut_down: false,
                receipt: None,
                handoff: None,
                modal_open: false,
            }),
        }
    }
    fn persist(&self, prefs: &Preferences) -> HostResult<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let temp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(prefs).map_err(|_| "无法保存快捷入口设置")?;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| "无法保存快捷入口设置")?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "无法保存快捷入口设置")?;
        fs::rename(&temp, &self.path).map_err(|_| "无法保存快捷入口设置")?;
        if let Some(parent) = self.path.parent() {
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| "无法确认快捷入口设置已保存")?;
        }
        Ok(())
    }
}
fn diagnostic(name: &str, milliseconds: u128) {
    #[cfg(debug_assertions)]
    if std::env::var_os("MEMIVY_ENTRY_DIAGNOSTICS").is_some() {
        eprintln!("{name}={milliseconds}");
    }
    #[cfg(not(debug_assertions))]
    let _ = (name, milliseconds);
}
fn require(window: &tauri::WebviewWindow) -> HostResult<()> {
    if matches!(window.label(), "main" | "capture") {
        Ok(())
    } else {
        Err("窗口无权执行此操作".into())
    }
}
fn login_status() -> String {
    let started = Instant::now();
    // SMAppService is available on the supported macOS version. No credentials or helpers.
    unsafe {
        let service: Retained<AnyObject> = msg_send![class!(SMAppService), mainAppService];
        let status: isize = msg_send![&*service, status];
        let result = match status {
            1 => "enabled",
            2 => "requires_approval",
            3 => "not_found",
            _ => "disabled",
        }
        .into();
        diagnostic("login_status_ms", started.elapsed().as_millis());
        result
    }
}
pub fn launched_at_login() -> bool {
    if std::env::args().any(|s| s == "--background") {
        return true;
    }
    unsafe {
        let manager: Retained<AnyObject> =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let event: Option<Retained<AnyObject>> = msg_send![&*manager, currentAppleEvent];
        let Some(event) = event else {
            return false;
        };
        let props: Option<Retained<AnyObject>> =
            msg_send![&*event, paramDescriptorForKeyword: u32::from_be_bytes(*b"prdt")];
        let Some(props) = props else {
            return false;
        };
        let item: Option<Retained<AnyObject>> =
            msg_send![&*props, descriptorForKeyword: u32::from_be_bytes(*b"lgit")];
        item.is_some_and(|item| msg_send![&*item, booleanValue])
    }
}
fn snapshot(app: &tauri::AppHandle) -> Snapshot {
    let started = Instant::now();
    let desktop = app.state::<Desktop>();
    let mut s = desktop.inner.lock().unwrap();
    s.sequence += 1;
    let p = &s.prefs;
    let result = Snapshot {
        sequence: s.sequence,
        expanded: s.expanded,
        generation: s.generation,
        pinned: p.pinned,
        visible: p.visible,
        paused: p.paused,
        shortcut: p.shortcut.clone(),
        mode: p.mode.clone(),
        configured: s.configured,
        topic: s.topic.clone(),
        source_app: p.source_app.clone(),
        last_memory: p.last_memory.clone(),
        error: s.error.clone(),
        ready_ms: s.ready_ms,
        save_ms: s.save_ms,
        receipt: s.receipt.is_some(),
    };
    diagnostic("snapshot_ms", started.elapsed().as_millis());
    result
}
// Context IO is separate from geometry/focus snapshots. A failed read must not
// convert a live discussion into a new capture surface.
fn refresh_context(app: &tauri::AppHandle) {
    let desktop = app.state::<Desktop>();
    let topic_id = desktop.inner.lock().unwrap().prefs.topic_id.clone();
    let topic = topic_id
        .as_ref()
        .map(|id| app.state::<Workspace>().store.conversation(id));
    let configured = crate::workspace::model_available(&app.state::<Workspace>());
    let mut state = desktop.inner.lock().unwrap();
    state.configured = configured;
    if state.prefs.topic_id == topic_id {
        match topic {
            Some(Ok(value)) => state.topic = Some(value),
            None => state.topic = None,
            Some(Err(error)) => state.error = Some(error.to_string()),
        }
    }
}
fn publish(app: &tauri::AppHandle) {
    let _ = app.emit("desktop-state", snapshot(app));
}
pub fn clamp_position(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    area: &tauri::PhysicalRect<i32, u32>,
) -> (i32, i32) {
    let left = area.position.x + 8;
    let top = area.position.y + 8;
    (
        x.clamp(
            left,
            (area.position.x + area.size.width as i32 - width as i32 - 8).max(left),
        ),
        y.clamp(
            top,
            (area.position.y + area.size.height as i32 - height as i32 - 8).max(top),
        ),
    )
}
#[derive(Clone, Copy)]
enum Placement {
    KeepAnchor,
    Cursor,
    SavedLeaf,
}
fn panel_frame(
    requested: tauri::LogicalSize<f64>,
    placement: Placement,
    current: tauri::PhysicalRect<i32, u32>,
    saved: Option<(i32, i32)>,
    area: &tauri::PhysicalRect<i32, u32>,
    scale: f64,
) -> tauri::PhysicalRect<i32, u32> {
    let size = tauri::LogicalSize::new(
        requested.width.min(area.size.width as f64 / scale - 16.0),
        requested.height.min(area.size.height as f64 / scale - 16.0),
    )
    .to_physical::<u32>(scale);
    let (x, y) = match placement {
        Placement::KeepAnchor => (
            current.position.x + current.size.width as i32 - size.width as i32,
            current.position.y + current.size.height as i32 - size.height as i32,
        ),
        Placement::Cursor => (
            area.position.x + (area.size.width as i32 - size.width as i32) / 2,
            area.position.y + (area.size.height as i32 - size.height as i32) / 3,
        ),
        Placement::SavedLeaf => saved
            .map(|(x, y)| (x - (size.width as i32 - (72.0 * scale) as i32), y))
            .unwrap_or((
                area.position.x + area.size.width as i32 - size.width as i32 - 28,
                area.position.y + area.size.height as i32 - size.height as i32 - 72,
            )),
    };
    let (x, y) = clamp_position(x, y, size.width, size.height, area);
    tauri::PhysicalRect {
        position: tauri::PhysicalPosition::new(x, y),
        size,
    }
}
// Main thread only. Tao queues macOS frame changes, so calculate the complete
// destination before scheduling either mutation; never read back a pending size.
fn layout(app: &tauri::AppHandle, width: f64, height: f64, placement: Placement) -> HostResult<()> {
    let w = app.get_webview_window("capture").ok_or("快捷窗口不可用")?;
    let saved = app.state::<Desktop>().inner.lock().unwrap().prefs.position;
    let destination = match placement {
        Placement::Cursor => w.cursor_position().ok().map(|p| (p.x, p.y)),
        Placement::SavedLeaf => saved.map(|(x, y)| (x as f64, y as f64)),
        Placement::KeepAnchor => None,
    };
    let m = destination
        .and_then(|(x, y)| w.monitor_from_point(x, y).ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten())
        .ok_or("无法定位显示器")?;
    let frame = panel_frame(
        tauri::LogicalSize::new(width, height),
        placement,
        tauri::PhysicalRect {
            position: w.outer_position().map_err(|_| "无法读取窗口位置")?,
            size: w.outer_size().map_err(|_| "无法读取窗口尺寸")?,
        },
        saved,
        m.work_area(),
        m.scale_factor(),
    );
    w.set_size(frame.size.to_logical::<f64>(m.scale_factor()))
        .and_then(|_| w.set_position(frame.position))
        .map_err(|_| "无法调整快捷窗口".into())
}
pub fn open(app: &tauri::AppHandle, mode: Option<String>, at_cursor: bool) -> HostResult<()> {
    let started = Instant::now();
    let panel = app
        .get_webview_panel("capture")
        .map_err(|_| "快捷窗口不可用")?;
    let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication();
    let desktop = app.state::<Desktop>();
    let (was_expanded, discussion);
    {
        let mut s = desktop.inner.lock().unwrap();
        was_expanded = s.expanded;
        if !was_expanded {
            s.previous_pid = front.as_ref().map(|p| p.processIdentifier());
            if app
                .state::<Workspace>()
                .store
                .workspace_draft("quick_capture")
                .map_err(|e| e.to_string())?
                .is_none_or(|d| d.body.is_empty())
            {
                s.prefs.source_app = front
                    .as_ref()
                    .and_then(|p| p.localizedName())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "未知应用".into());
            }
        }
        if let Some(mode) = mode.filter(|_| !s.modal_open) {
            if !matches!(mode.as_str(), "capture" | "ask") {
                return Err("输入用途无效".into());
            }
            s.prefs.mode = mode;
        }
        let persist_started = Instant::now();
        desktop.persist(&s.prefs)?;
        diagnostic("entry_prefs_ms", persist_started.elapsed().as_millis());
        s.expanded = true;
        s.handoff = None;
        s.generation += 1;
        s.opened = Some(started);
        s.ready_ms = None;
        discussion = s.prefs.mode == "ask" && s.prefs.topic_id.is_some();
    }
    layout(
        app,
        500.0,
        if discussion { 620.0 } else { 310.0 },
        if !was_expanded && at_cursor {
            Placement::Cursor
        } else {
            Placement::KeepAnchor
        },
    )?;
    capture_panel::set_accepts_keyboard(true);
    panel.show_and_make_key();
    if let Some(w) = app.get_webview_window("capture") {
        let _ = w.as_ref().set_focus();
    }
    publish(app);
    diagnostic("entry_native_ms", started.elapsed().as_millis());
    Ok(())
}
fn collapse(app: &tauri::AppHandle, restore: bool) -> HostResult<()> {
    let started = Instant::now();
    let panel = app
        .get_webview_panel("capture")
        .map_err(|_| "快捷窗口不可用")?;
    let was_key = panel.as_panel().isKeyWindow();
    let (pid, visible, receipt) = {
        let desktop = app.state::<Desktop>();
        let mut s = desktop.inner.lock().unwrap();
        s.expanded = false;
        s.handoff = None;
        s.generation += 1;
        s.drag = None;
        (
            s.previous_pid,
            s.prefs.visible && !s.prefs.paused,
            s.receipt.is_some(),
        )
    };
    panel.hide();
    capture_panel::set_accepts_keyboard(false);
    layout(
        app,
        if receipt { 230.0 } else { 72.0 },
        76.0,
        Placement::SavedLeaf,
    )?;
    if visible {
        panel.order_front_regardless();
    }
    if restore && was_key {
        let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication();
        let current = front.as_ref().map(|p| p.processIdentifier());
        // Do not steal focus from a newly selected app while a disk write was pending.
        if (current == Some(std::process::id() as i32) || current == pid)
            && let Some(previous) = pid.and_then(
                objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier,
            )
        {
            previous.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        }
    }
    publish(app);
    diagnostic("collapse_native_ms", started.elapsed().as_millis());
    Ok(())
}
pub fn show_main(app: &tauri::AppHandle) -> HostResult<()> {
    let w = app.get_webview_window("main").ok_or("主窗口不可用")?;
    w.show()
        .and_then(|_| w.set_focus())
        .and_then(|_| w.as_ref().set_focus())
        .map_err(|_| "无法打开主窗口".into())
}
pub(crate) fn parse_shortcut(text: &str) -> HostResult<Shortcut> {
    if text.len() > 100 {
        return Err("快捷键格式无效".into());
    }
    let shortcut: Shortcut = text
        .parse()
        .map_err(|_| "无法识别这个快捷键，请重新录入。")?;
    if !shortcut
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::SUPER | Modifiers::ALT)
        || shortcut.mods.contains(Modifiers::CONTROL | Modifiers::ALT)
    {
        return Err("请选择带有修饰键的快捷键，并避开 VoiceOver 的 ⌃⌥ 组合。".into());
    }
    if shortcut.key == Code::Space
        || (shortcut.mods.contains(Modifiers::SUPER)
            && matches!(shortcut.key, Code::KeyQ | Code::KeyW | Code::Tab))
    {
        return Err("这个组合常用于系统或窗口操作，请换一个快捷键。".into());
    }
    Ok(shortcut)
}

fn register(app: &tauri::AppHandle, text: &str) -> HostResult<()> {
    app.global_shortcut()
        .on_shortcut(parse_shortcut(text)?, |app, _, e| {
            let app = app.clone();
            let h = app.clone();
            let _ = app.run_on_main_thread(move || {
                {
                    let d = h.state::<Desktop>();
                    let mut s = d.inner.lock().unwrap();
                    if e.state == ShortcutState::Released {
                        s.shortcut_down = false;
                        return;
                    }
                    if s.shortcut_down {
                        return;
                    }
                    s.shortcut_down = true;
                }
                let (expanded, generation) = {
                    let d = h.state::<Desktop>();
                    let s = d.inner.lock().unwrap();
                    (s.expanded, s.generation)
                };
                if expanded
                    && h.get_webview_panel("capture")
                        .is_ok_and(|p| p.as_panel().isKeyWindow())
                {
                    let _ = h.emit_to("capture", "desktop-dismiss-request", generation);
                } else if let Err(e) = open(&h, None, true) {
                    set_error(&h, e);
                }
            });
        })
        .map_err(|_| {
            "快捷键注册失败，可能已被其他应用占用。请修改快捷键；菜单栏入口仍可使用。".into()
        })
}
pub(crate) fn set_error(app: &tauri::AppHandle, error: String) {
    app.state::<Desktop>().inner.lock().unwrap().error = Some(error);
    publish(app);
}
fn update_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let p = app.state::<Desktop>().inner.lock().unwrap().prefs.clone();
    let status = MenuItem::with_id(
        app,
        "status",
        if p.paused {
            "快捷入口已暂停"
        } else {
            "本地记录可用"
        },
        false,
        None::<&str>,
    )?;
    let capture = MenuItem::with_id(app, "quick-capture", "记一下", true, None::<&str>)?;
    let ask = MenuItem::with_id(app, "quick-ask", "问一问", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "打开 Memivy", true, None::<&str>)?;
    let visible = MenuItem::with_id(
        app,
        "leaf",
        if p.visible {
            "隐藏桌面助手"
        } else {
            "显示桌面助手"
        },
        true,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(
        app,
        "pause",
        if p.paused {
            "恢复快捷入口"
        } else {
            "暂停快捷入口"
        },
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Memivy", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status, &sep, &capture, &ask, &open, &visible, &pause, &settings, &sep2, &quit,
        ],
    )?;
    if let Some(tray) = app.tray_by_id("memivy") {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let path = app
        .state::<Workspace>()
        .store
        .database_path()
        .with_file_name("desktop.json");
    app.manage(Desktop::new(path));
    refresh_context(app.handle());
    termination::install(app.handle())?;
    capture_panel::configure(app.handle())?;
    panel_events::install(app.handle());
    TrayIconBuilder::with_id("memivy")
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray-icon.png"
        ))?)
        .icon_as_template(true)
        .tooltip("Memivy")
        .on_menu_event(|app, event| {
            let h = app.clone();
            let id = event.id.as_ref().to_string();
            let _ = app.run_on_main_thread(move || {
                let result = match id.as_str() {
                    "quick-capture" => open(&h, Some("capture".into()), true),
                    "quick-ask" => open(&h, Some("ask".into()), true),
                    "open" => show_main(&h),
                    "settings" => {
                        let r = show_main(&h);
                        let _ = h.emit_to("main", "desktop-settings", ());
                        r
                    }
                    "leaf" => {
                        let p = snapshot(&h);
                        change(
                            &h,
                            Patch {
                                visible: Some(!p.visible),
                                ..Default::default()
                            },
                        )
                    }
                    "pause" => {
                        let p = snapshot(&h);
                        change(
                            &h,
                            Patch {
                                paused: Some(!p.paused),
                                ..Default::default()
                            },
                        )
                    }
                    "quit" => {
                        request_quit(&h);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(e) = result {
                    set_error(&h, e);
                }
            });
        })
        .build(app)?;
    update_menu(app.handle())?;
    let p = app.state::<Desktop>().inner.lock().unwrap().prefs.clone();
    if !p.paused
        && let Err(e) = register(app.handle(), &p.shortcut)
    {
        set_error(app.handle(), e);
    }
    layout(app.handle(), 72.0, 76.0, Placement::SavedLeaf)?;
    if p.visible && !p.paused {
        app.get_webview_panel("capture")
            .unwrap()
            .order_front_regardless();
    }
    if !launched_at_login() {
        show_main(app.handle())?;
    }
    Ok(())
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    shortcut: Option<String>,
    visible: Option<bool>,
    paused: Option<bool>,
    pinned: Option<bool>,
    mode: Option<String>,
    topic_id: Option<String>,
    clear_topic: Option<bool>,
}
fn change(app: &tauri::AppHandle, patch: Patch) -> HostResult<()> {
    let desktop = app.state::<Desktop>();
    if desktop.inner.lock().unwrap().modal_open
        && (patch.mode.is_some() || patch.topic_id.is_some() || patch.clear_topic == Some(true))
    {
        return Err("请先完成或关闭快捷窗口中的对话框，再切换话题。".into());
    }
    let old = desktop.inner.lock().unwrap().prefs.clone();
    let mut p = old.clone();
    let mut next_topic = None;
    if let Some(v) = patch.shortcut {
        parse_shortcut(&v)?;
        p.shortcut = v;
    }
    if let Some(v) = patch.visible {
        p.visible = v;
    }
    if let Some(v) = patch.paused {
        p.paused = v;
    }
    if let Some(v) = patch.pinned {
        p.pinned = v;
    }
    if let Some(v) = patch.mode {
        if !matches!(v.as_str(), "capture" | "ask") {
            return Err("输入用途无效".into());
        }
        p.mode = v;
    }
    if let Some(id) = patch.topic_id {
        let topic = app
            .state::<Workspace>()
            .store
            .conversation(&id)
            .map_err(|e| e.to_string())?;
        next_topic = Some(Some(topic));
        p.topic_id = Some(id);
        p.mode = "ask".into();
    }
    if patch.clear_topic == Some(true) {
        next_topic = Some(None);
        p.topic_id = None;
    }
    let changed = old.shortcut != p.shortcut || old.paused != p.paused;
    if changed && !p.paused {
        register(app, &p.shortcut)?;
    }
    if let Err(e) = desktop.persist(&p) {
        if changed && !p.paused {
            let _ = app
                .global_shortcut()
                .unregister(parse_shortcut(&p.shortcut)?);
        }
        return Err(e);
    }
    if changed && !old.paused && (old.shortcut != p.shortcut || p.paused) {
        let _ = app
            .global_shortcut()
            .unregister(parse_shortcut(&old.shortcut)?);
    }
    {
        let mut s = desktop.inner.lock().unwrap();
        s.prefs = p.clone();
        if let Some(topic) = next_topic {
            s.topic = topic;
        }
        s.error = None;
    }
    if p.paused && snapshot(app).expanded {
        let _ = app.emit_to(
            "capture",
            "desktop-dismiss-request",
            snapshot(app).generation,
        );
    } else if p.paused || !p.visible && !snapshot(app).expanded {
        collapse(app, false)?;
    } else if !snapshot(app).expanded {
        let receipt = desktop.inner.lock().unwrap().receipt.is_some();
        layout(
            app,
            if receipt { 230.0 } else { 72.0 },
            76.0,
            Placement::SavedLeaf,
        )?;
        app.get_webview_panel("capture")
            .map_err(|_| "快捷窗口不可用")?
            .order_front_regardless();
    }
    if snapshot(app).expanded {
        layout(
            app,
            500.0,
            if p.mode == "ask" && p.topic_id.is_some() {
                620.0
            } else {
                310.0
            },
            Placement::KeepAnchor,
        )?;
    }
    update_menu(app).map_err(|_| "菜单栏无法更新")?;
    publish(app);
    Ok(())
}
pub(crate) async fn on_main<T: Send + 'static>(
    app: &tauri::AppHandle,
    work: impl FnOnce(tauri::AppHandle) -> HostResult<T> + Send + 'static,
) -> HostResult<T> {
    let h = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(work(h));
    })
    .map_err(|_| "窗口操作未完成")?;
    rx.await.map_err(|_| "窗口操作未完成")?
}
#[tauri::command]
pub async fn desktop_state(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<Snapshot> {
    require(&window)?;
    let context_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || refresh_context(&context_app))
        .await
        .map_err(|_| "快捷入口状态未能读取")?;
    on_main(&app, |h| Ok(snapshot(&h))).await
}
#[tauri::command]
pub fn desktop_modal(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    open: bool,
) -> HostResult<()> {
    if window.label() != "capture" {
        return Err("仅快捷窗口可以更新此状态".into());
    }
    app.state::<Desktop>().inner.lock().unwrap().modal_open = open;
    Ok(())
}
#[tauri::command]
pub async fn desktop_update(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    patch: Patch,
) -> HostResult<Snapshot> {
    require(&window)?;
    on_main(&app, move |h| {
        change(&h, patch)?;
        Ok(snapshot(&h))
    })
    .await
}
#[tauri::command]
pub async fn desktop_open(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    mode: Option<String>,
) -> HostResult<()> {
    require(&window)?;
    let at_cursor = window.label() == "main";
    on_main(&app, move |h| open(&h, mode, at_cursor)).await
}
#[tauri::command]
pub async fn desktop_dismiss(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    generation: u64,
    reason: String,
    requested_at: Option<u128>,
) -> HostResult<()> {
    require(&window)?;
    on_main(&app, move |h| {
        let (active, pinned, handing_off) = {
            let d = h.state::<Desktop>();
            let s = d.inner.lock().unwrap();
            (
                s.generation == generation && s.expanded,
                s.prefs.pinned,
                s.handoff == Some(generation),
            )
        };
        if !active {
            return Ok(());
        }
        if reason == "blur"
            && (handing_off
                || pinned
                || h.get_webview_panel("capture")
                    .is_ok_and(|p| p.as_panel().isKeyWindow()))
        {
            return Ok(());
        }
        if reason == "saved" && pinned {
            return Ok(());
        }
        collapse(&h, reason != "blur")?;
        if let Some(start) = requested_at {
            diagnostic(
                "dismiss_total_ms",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .saturating_sub(start),
            );
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn desktop_composer_resize(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    generation: u64,
    height: f64,
) -> HostResult<()> {
    if window.label() != "capture" || !height.is_finite() {
        return Err("无效的快捷窗口尺寸请求".into());
    }
    on_main(&app, move |h| {
        let desktop = h.state::<Desktop>();
        let s = desktop.inner.lock().unwrap();
        if !s.expanded
            || s.generation != generation
            || (s.prefs.mode == "ask" && s.prefs.topic_id.is_some())
        {
            return Ok(());
        }
        drop(s);
        layout(&h, 500.0, height.clamp(310.0, 620.0), Placement::KeepAnchor)
    })
    .await
}
#[tauri::command]
pub async fn desktop_ready(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    generation: Option<u64>,
) -> HostResult<()> {
    require(&window)?;
    on_main(&app, move |h| {
        let native_focus = h
            .get_webview_panel("capture")
            .is_ok_and(|p| p.as_panel().isKeyWindow());
        let desktop = h.state::<Desktop>();
        let mut s = desktop.inner.lock().unwrap();
        s.ready_windows.insert(window.label().into());
        if window.label() == "capture"
            && native_focus
            && generation == Some(s.generation)
            && s.ready_ms.is_none()
        {
            s.ready_ms = s.opened.map(|t| t.elapsed().as_millis());
            diagnostic("entry_ready_ms", s.ready_ms.unwrap_or_default());
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn desktop_capture(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    request: CaptureRequest,
    submitted_at: Option<u128>,
) -> HostResult<CaptureResult> {
    require(&window)?;
    let start = Instant::now();
    let store = app.state::<Workspace>().store.clone();
    if !matches!(request.origin, Origin::User { .. }) {
        return Err("快捷入口只能保存你主动输入的记录".into());
    }
    if let Origin::User { project, uri, .. } = &request.origin
        && (project.is_some()
            || uri.as_ref().is_some_and(|v| {
                !v.starts_with("https://")
                    && !v.starts_with("http://")
                    && !v.starts_with("file://")
                    && !v.starts_with('/')
            }))
    {
        return Err("请附带网页链接或绝对文件路径，或移除来源后保存。".into());
    }
    let raw = super::workspace::blocking(move || store.capture(&request)).await?;
    let committed_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|now| submitted_at.map(|start| now.as_millis().saturating_sub(start)))
        .unwrap_or_else(|| start.elapsed().as_millis());
    diagnostic("capture_committed_ms", committed_ms);
    let memory_id = raw.memory_id.clone();
    on_main(&app, move |h| {
        let desktop = h.state::<Desktop>();
        let mut s = desktop.inner.lock().unwrap();
        s.save_ms = Some(committed_ms);
        if window.label() == "capture" {
            s.receipt = Some(memory_id.clone());
        }
        s.prefs.last_memory = Some(memory_id);
        if let Err(e) = desktop.persist(&s.prefs) {
            s.error = Some(e);
        }
        Ok(())
    })
    .await?;
    let timer_app = app.clone();
    let receipt_id = raw.memory_id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(2800)).await;
        let _ = on_main(&timer_app, move |h| {
            let d = h.state::<Desktop>();
            let clear = {
                let mut s = d.inner.lock().unwrap();
                if s.receipt.as_ref() == Some(&receipt_id) {
                    s.receipt = None;
                    true
                } else {
                    false
                }
            };
            if clear {
                if !snapshot(&h).expanded {
                    layout(&h, 72.0, 76.0, Placement::KeepAnchor)?;
                }
                publish(&h);
            }
            Ok(())
        })
        .await;
    });
    let _ = app.emit("resources-changed", ());
    on_main(&app, |h| {
        publish(&h);
        Ok(())
    })
    .await?;
    Ok(raw)
}
#[derive(Clone, Serialize)]
pub struct MainRoute {
    pub generation: u64,
    pub topic: Option<Conversation>,
    pub mode: String,
    pub quick: bool,
    pub record: Option<RecordKey>,
    pub settings: bool,
}
#[tauri::command]
pub async fn desktop_expand(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    record: Option<RecordKey>,
    settings: bool,
) -> HostResult<()> {
    require(&window)?;
    on_main(&app, move |h| {
        if !h
            .state::<Desktop>()
            .inner
            .lock()
            .unwrap()
            .ready_windows
            .contains("main")
        {
            return Err("主窗口正在准备，请稍后重试。".into());
        }
        let s = snapshot(&h);
        let route = MainRoute {
            generation: s.generation,
            topic: s.topic,
            mode: s.mode,
            quick: true,
            record,
            settings,
        };
        h.state::<Desktop>().inner.lock().unwrap().handoff = Some(s.generation);
        if let Err(error) = show_main(&h).and_then(|_| {
            h.emit_to("main", "desktop-route", route)
                .map_err(|_| "主窗口尚未准备好，请重试".to_string())
        }) {
            h.state::<Desktop>().inner.lock().unwrap().handoff = None;
            return Err(error);
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn desktop_handoff_ready(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    generation: u64,
    failed: Option<bool>,
) -> HostResult<()> {
    if window.label() != "main" {
        return Err("仅主窗口可以完成交接".into());
    }
    on_main(&app, move |h| {
        if snapshot(&h).generation == generation {
            h.state::<Desktop>().inner.lock().unwrap().handoff = None;
            if failed == Some(true) {
                Ok(())
            } else {
                collapse(&h, false)
            }
        } else {
            Ok(())
        }
    })
    .await
}
#[tauri::command]
pub async fn desktop_drag(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    phase: String,
) -> HostResult<()> {
    if window.label() != "capture" {
        return Err("仅快捷窗口可以拖动".into());
    }
    on_main(&app, move |h| {
        let d = h.state::<Desktop>();
        match phase.as_str() {
            "start" => {
                let origin = window.outer_position().map_err(|_| "无法拖动")?;
                let cursor = window.cursor_position().map_err(|_| "无法拖动")?;
                d.inner.lock().unwrap().drag = Some((origin, cursor));
            }
            "move" => {
                let drag = d.inner.lock().unwrap().drag;
                if let Some((origin, cursor)) = drag {
                    let now = window.cursor_position().map_err(|_| "无法拖动")?;
                    window
                        .set_position(tauri::PhysicalPosition::new(
                            origin.x + (now.x - cursor.x) as i32,
                            origin.y + (now.y - cursor.y) as i32,
                        ))
                        .map_err(|_| "无法拖动")?;
                }
            }
            "end" => {
                let pos = window.outer_position().map_err(|_| "无法保存位置")?;
                let mut s = d.inner.lock().unwrap();
                s.drag = None;
                let size = window.outer_size().map_err(|_| "无法保存位置")?;
                let scale = window.scale_factor().map_err(|_| "无法保存位置")?;
                s.prefs.position = Some((
                    pos.x + size.width as i32 - (72.0 * scale) as i32,
                    pos.y + size.height as i32 - (76.0 * scale) as i32,
                ));
                d.persist(&s.prefs)?;
            }
            _ => return Err("拖动操作无效".into()),
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn desktop_login_status(window: tauri::WebviewWindow) -> HostResult<String> {
    if window.label() != "main" {
        return Err("请在主窗口查询登录启动".into());
    }
    // ServiceManagement synchronously talks to a system service. Query only
    // when Settings needs it, and never block AppKit's window/event thread.
    tauri::async_runtime::spawn_blocking(|| objc2::rc::autoreleasepool(|_| login_status()))
        .await
        .map_err(|_| "无法读取登录项状态，请重试".into())
}
#[tauri::command]
pub async fn desktop_login(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    enabled: Option<bool>,
) -> HostResult<String> {
    if window.label() != "main" {
        return Err("请在主窗口设置登录启动".into());
    }
    on_main(&app,move|_|{
        unsafe {
            if let Some(enabled)=enabled {
                let service:Retained<AnyObject>=msg_send![class!(SMAppService),mainAppService];
                let mut error:Option<Retained<objc2_foundation::NSError>>=None;
                let ok:bool=if enabled {msg_send![&*service,registerAndReturnError:&mut error]} else {msg_send![&*service,unregisterAndReturnError:&mut error]};
                if !ok {return Err("macOS 未能更新登录项。请将 Memivy 放入应用程序目录后重试，或检查系统设置中的登录项。".into());}
            } else {let _:()=msg_send![class!(SMAppService),openSystemSettingsLoginItems];}
        }
        Ok(login_status())
    }).await
}
pub fn request_quit(app: &tauri::AppHandle) {
    let (id, windows) = {
        let d = app.state::<Desktop>();
        let mut s = d.inner.lock().unwrap();
        if s.quit.is_some() {
            return;
        }
        s.next_quit += 1;
        let id = s.next_quit;
        let windows = s.ready_windows.clone();
        s.quit = Some((id, windows.clone()));
        (id, windows)
    };
    if windows.is_empty() {
        finish_quit(app);
        return;
    }
    for label in windows {
        let _ = app.emit_to(&label, "desktop-exit-request", id);
    }
    let h = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let _ = on_main(&h, move |app| {
            let waiting = {
                let d = app.state::<Desktop>();
                let mut s = d.inner.lock().unwrap();
                if s.quit.as_ref().is_some_and(|(token, _)| *token == id) {
                    s.quit.take().map(|(_, windows)| windows)
                } else {
                    None
                }
            };
            if let Some(waiting) = waiting {
                termination::reply(false);
                crate::backup::cancel_restart(&app);
                set_error(
                    &app,
                    "窗口尚未确认草稿保存，已取消退出。请核对草稿后重试。".into(),
                );
                if waiting.contains("capture") {
                    open(&app, None, true)?;
                } else {
                    show_main(&app)?;
                }
            }
            Ok(())
        })
        .await;
    });
}

fn finish_quit(app: &tauri::AppHandle) {
    if crate::backup::finish_restart(app) {
        termination::reply(false);
        return;
    }
    app.state::<Workspace>()
        .exiting
        .store(true, std::sync::atomic::Ordering::Relaxed);
    if !termination::reply(true) {
        app.exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_and_collapse_use_destination_size_regardless_of_previous_frame() {
        let area = tauri::PhysicalRect {
            position: tauri::PhysicalPosition::new(0, 0),
            size: tauri::PhysicalSize::new(2880, 1800),
        };
        for (width, height) in [(72, 76), (230, 76), (500, 310), (500, 620)] {
            let current = tauri::PhysicalRect {
                position: tauri::PhysicalPosition::new(100, 100),
                size: tauri::PhysicalSize::new(width * 2, height * 2),
            };
            let open = panel_frame(
                tauri::LogicalSize::new(500.0, 310.0),
                Placement::Cursor,
                current,
                None,
                &area,
                2.0,
            );
            assert_eq!(open.position, tauri::PhysicalPosition::new(940, 393));
            assert_eq!(open.size, tauri::PhysicalSize::new(1000, 620));
            let leaf = panel_frame(
                tauri::LogicalSize::new(72.0, 76.0),
                Placement::SavedLeaf,
                current,
                Some((2600, 1600)),
                &area,
                2.0,
            );
            assert_eq!(leaf.position, tauri::PhysicalPosition::new(2600, 1600));
            assert_eq!(leaf.size, tauri::PhysicalSize::new(144, 152));
            let receipt = panel_frame(
                tauri::LogicalSize::new(230.0, 76.0),
                Placement::SavedLeaf,
                current,
                Some((2600, 1600)),
                &area,
                2.0,
            );
            let expired = panel_frame(
                tauri::LogicalSize::new(72.0, 76.0),
                Placement::KeepAnchor,
                receipt,
                None,
                &area,
                2.0,
            );
            assert_eq!(expired.position, leaf.position);
            assert_eq!(expired.size, leaf.size);
        }
    }
    #[test]
    fn panel_geometry_respects_destination_scale_work_area_and_anchor() {
        for scale in [1.0, 2.0] {
            let area = tauri::PhysicalRect {
                position: tauri::PhysicalPosition::new(-1920, 30),
                size: tauri::PhysicalSize::new(1920, 1050),
            };
            let leaf = panel_frame(
                tauri::LogicalSize::new(72.0, 76.0),
                Placement::SavedLeaf,
                area,
                None,
                &area,
                scale,
            );
            let open = panel_frame(
                tauri::LogicalSize::new(500.0, 620.0),
                Placement::KeepAnchor,
                leaf,
                None,
                &area,
                scale,
            );
            assert_eq!(
                open.position.x + open.size.width as i32,
                leaf.position.x + leaf.size.width as i32
            );
            assert!(open.position.y >= area.position.y + 8);
            assert!(open.position.y + open.size.height as i32 <= 1072);
            let centered = panel_frame(
                tauri::LogicalSize::new(500.0, 620.0),
                Placement::Cursor,
                leaf,
                None,
                &area,
                scale,
            );
            assert_eq!(
                centered.position.x,
                -1920 + (1920 - centered.size.width as i32) / 2
            );
            assert!(centered.size.height <= area.size.height - (16.0 * scale) as u32);
        }
    }
    #[test]
    fn shortcut_validation_handles_modifier_order_and_system_combinations() {
        assert!(parse_shortcut("Control+Super+KeyM").is_ok());
        assert!(parse_shortcut("Super+Shift+KeyM").is_ok());
        for reserved in [
            "KeyM",
            "Shift+KeyM",
            "Control+Alt+KeyM",
            "Alt+Control+KeyM",
            "Super+Space",
            "Super+KeyQ",
            "Shift+Super+KeyQ",
        ] {
            assert!(parse_shortcut(reserved).is_err(), "{reserved}");
        }
    }
    #[test]
    fn windows_are_clamped_to_work_area_including_negative_display_coordinates() {
        let area = tauri::PhysicalRect {
            position: tauri::PhysicalPosition::new(-1920, 30),
            size: tauri::PhysicalSize::new(1920, 1050),
        };
        assert_eq!(clamp_position(-5000, -200, 500, 620, &area), (-1912, 38));
        assert_eq!(clamp_position(4000, 2000, 500, 620, &area), (-508, 452));
        assert_eq!(clamp_position(-1000, 100, 500, 620, &area), (-1000, 100));
        assert_eq!(clamp_position(1, 1, 4000, 3000, &area), (-1912, 38));
    }
    #[test]
    fn preferences_roundtrip_without_losing_visibility_pause_or_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        let desktop = Desktop::new(path.clone());
        let prefs = Preferences {
            visible: false,
            paused: true,
            pinned: true,
            position: Some((-500, 400)),
            ..Default::default()
        };
        desktop.persist(&prefs).unwrap();
        let loaded = Desktop::new(path.clone());
        let s = loaded.inner.lock().unwrap();
        assert!(!s.prefs.visible);
        assert!(s.prefs.paused && s.prefs.pinned);
        assert_eq!(s.prefs.position, Some((-500, 400)));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[tauri::command]
pub async fn desktop_exit_ready(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    id: u64,
    error: bool,
) -> HostResult<()> {
    require(&window)?;
    on_main(&app, move |h| {
        let quit = {
            let d = h.state::<Desktop>();
            let mut s = d.inner.lock().unwrap();
            let Some(done) = acknowledge_exit(&mut s.quit, id, window.label(), error) else {
                return Ok(());
            };
            done
        };
        if error {
            termination::reply(false);
            crate::backup::cancel_restart(&h);
            set_error(
                &h,
                "请先完成当前对话框或核对未保存的草稿，再退出 Memivy。".into(),
            );
            if window.label() == "capture" {
                open(&h, None, true)?;
            } else {
                show_main(&h)?;
            }
        } else if quit {
            finish_quit(&h);
        }
        Ok(())
    })
    .await
}

// Consume each window acknowledgement once, including during asynchronous restore arming.
fn acknowledge_exit(
    quit: &mut Option<(u64, HashSet<String>)>,
    id: u64,
    window: &str,
    error: bool,
) -> Option<bool> {
    let (token, remaining) = quit.as_mut()?;
    if *token != id || !remaining.remove(window) {
        return None;
    }
    let done = remaining.is_empty() && !error;
    if error || done {
        *quit = None;
    }
    Some(done)
}
#[cfg(test)]
mod exit_ack_tests {
    use super::*;
    #[test]
    fn duplicate_or_late_acknowledgements_cannot_exit_during_restore_arming() {
        let mut quit = Some((7, HashSet::from(["main".into(), "capture".into()])));
        assert_eq!(acknowledge_exit(&mut quit, 6, "main", false), None);
        assert_eq!(acknowledge_exit(&mut quit, 7, "main", false), Some(false));
        assert_eq!(acknowledge_exit(&mut quit, 7, "main", false), None);
        assert_eq!(acknowledge_exit(&mut quit, 7, "capture", false), Some(true));
        assert_eq!(acknowledge_exit(&mut quit, 7, "capture", false), None);
        assert!(quit.is_none());
        quit = Some((8, HashSet::from(["main".into()])));
        assert_eq!(acknowledge_exit(&mut quit, 8, "main", true), Some(false));
        assert!(quit.is_none());
    }
}
