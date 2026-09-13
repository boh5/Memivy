//! Formal macOS entry points. Window state is not a second memory store.
use crate::{
    capture_panel,
    workspace::{HostResult, Workspace},
};
use memivy_core::memory::{Conversation, RecordKey};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSPoint, NSRect, NSSize};
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
    #[serde(default = "login_already_initialized")]
    login_initialized: bool,
    shortcut: String,
    visible: bool,
    pinned: bool,
    topic_id: Option<String>,
    source_app: String,
    position: Option<(i32, i32)>,
    last_memory: Option<String>,
}
fn login_already_initialized() -> bool {
    true
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            login_initialized: false,
            shortcut: "Alt+KeyM".into(),
            visible: true,
            pinned: false,
            topic_id: None,
            source_app: "Memivy".into(),
            position: None,
            last_memory: None,
        }
    }
}
pub struct Desktop {
    login_operation: Mutex<()>,
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
    pub shortcut: String,
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
    fn remember_topic(&self, topic: Conversation) {
        let mut state = self.inner.lock().unwrap();
        state.prefs.topic_id = Some(topic.id.clone());
        state.topic = Some(topic);
        // The conversation is already durable. A failed optional window setting
        // remains visible, but must not prevent the Agent from starting.
        if let Err(error) = self.persist(&state.prefs) {
            state.error = Some(error.to_string());
        }
    }
    fn refresh_topic_title(&self, topic: &Conversation) -> bool {
        let mut state = self.inner.lock().unwrap();
        let Some(current) = state
            .topic
            .as_mut()
            .filter(|current| current.id == topic.id)
        else {
            return false;
        };
        current.title.clone_from(&topic.title);
        true
    }
    pub fn new(path: PathBuf) -> Self {
        let (prefs, error) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(p) => (p, None),
                Err(_) => (
                    Preferences {
                        login_initialized: true,
                        ..Default::default()
                    },
                    Some("desktop_settings_invalid".into()),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Preferences::default(), None),
            Err(_) => (
                Preferences {
                    login_initialized: true,
                    ..Default::default()
                },
                Some("desktop_settings_unreadable".into()),
            ),
        };
        Self {
            login_operation: Mutex::new(()),
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
        let bytes = serde_json::to_vec(prefs).map_err(|_| "desktop_settings_save_failed")?;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| "desktop_settings_save_failed")?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "desktop_settings_save_failed")?;
        fs::rename(&temp, &self.path).map_err(|_| "desktop_settings_save_failed")?;
        if let Some(parent) = self.path.parent() {
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| "desktop_settings_confirm_failed")?;
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
        Err("forbidden".into())
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
        shortcut: p.shortcut.clone(),
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
struct DesktopContext {
    topic_id: Option<String>,
    previous_title: Option<String>,
    topic: Option<memivy_core::memory::Result<Conversation>>,
    configured: bool,
}
fn read_context(app: &tauri::AppHandle) -> DesktopContext {
    let desktop = app.state::<Desktop>();
    let (topic_id, previous_title) = {
        let state = desktop.inner.lock().unwrap();
        (
            state.prefs.topic_id.clone(),
            state.topic.as_ref().map(|topic| topic.title.clone()),
        )
    };
    let topic = topic_id
        .as_ref()
        .map(|id| app.state::<Workspace>().store.conversation(id));
    let configured = crate::workspace::model_available(&app.state::<Workspace>());
    DesktopContext {
        topic_id,
        previous_title,
        topic,
        configured,
    }
}
// Call only on the main thread, where all preference read/modify/write operations
// are serialized with change(), open(), and the window lifecycle.
fn apply_context(desktop: &Desktop, context: DesktopContext) {
    let mut state = desktop.inner.lock().unwrap();
    state.configured = context.configured;
    if state.prefs.topic_id == context.topic_id {
        match context.topic {
            Some(Ok(mut value)) => {
                if let Some(current) = state
                    .topic
                    .as_ref()
                    .filter(|current| current.id == value.id)
                    && Some(current.title.as_str()) != context.previous_title.as_deref()
                {
                    // An auxiliary title arrived after this read began. Its
                    // database write does not change the conversation timestamp.
                    value.title.clone_from(&current.title);
                }
                state.topic = Some(value);
            }
            None => state.topic = None,
            Some(Err(memivy_core::memory::DataError::Unavailable)) => {
                state.topic = None;
                state.prefs.topic_id = None;
                if let Err(error) = desktop.persist(&state.prefs) {
                    state.error = Some(error.to_string());
                }
            }
            Some(Err(error)) => state.error = Some(error.to_string()),
        }
    }
}
fn publish(app: &tauri::AppHandle) {
    let _ = app.emit("desktop-state", snapshot(app));
}
pub fn refresh_topic_title(app: &tauri::AppHandle, topic: &Conversation) {
    if app.state::<Desktop>().refresh_topic_title(topic) {
        publish(app);
    }
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
    KeepTop,
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
        Placement::KeepTop => (current.position.x, current.position.y),
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
// Main thread only. Commit size and position together; Tao's separate setters
// queue two AppKit updates and expose an intermediate frame on this clear panel.
fn layout(app: &tauri::AppHandle, width: f64, height: f64, placement: Placement) -> HostResult<()> {
    let w = app
        .get_webview_window("capture")
        .ok_or("capture_window_unavailable")?;
    let saved = app.state::<Desktop>().inner.lock().unwrap().prefs.position;
    let destination = match placement {
        Placement::Cursor => w.cursor_position().ok().map(|p| (p.x, p.y)),
        Placement::SavedLeaf => saved.map(|(x, y)| (x as f64, y as f64)),
        Placement::KeepAnchor | Placement::KeepTop => None,
    };
    let m = destination
        .and_then(|(x, y)| w.monitor_from_point(x, y).ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten())
        .ok_or("display_unavailable")?;
    let current_scale = w.scale_factor().map_err(|_| "window_geometry_failed")?;
    let current = tauri::PhysicalRect {
        position: w.outer_position().map_err(|_| "window_geometry_failed")?,
        size: w.outer_size().map_err(|_| "window_geometry_failed")?,
    };
    let target_scale = m.scale_factor();
    // The destination monitor's physical coordinate space can have another scale.
    let target_current = tauri::PhysicalRect {
        position: current
            .position
            .to_logical::<f64>(current_scale)
            .to_physical(target_scale),
        size: current
            .size
            .to_logical::<f64>(current_scale)
            .to_physical(target_scale),
    };
    let frame = panel_frame(
        tauri::LogicalSize::new(width, height),
        placement,
        target_current,
        saved,
        m.work_area(),
        target_scale,
    );
    let panel = app
        .get_webview_panel("capture")
        .map_err(|_| "capture_window_unavailable")?;
    let native = panel.as_panel();
    let previous = native.frame();
    let from = current.position.to_logical::<f64>(current_scale);
    let to = frame.position.to_logical::<f64>(target_scale);
    let size = frame.size.to_logical::<f64>(target_scale);
    // Use the existing AppKit frame as the origin reference; screen focus and
    // different display scales must not change the global coordinate baseline.
    let next = NSRect::new(
        NSPoint::new(
            previous.origin.x + to.x - from.x,
            previous.origin.y + previous.size.height - size.height - (to.y - from.y),
        ),
        NSSize::new(size.width, size.height),
    );
    if next != previous {
        native.setFrame_display_animate(next, true, false);
    }
    Ok(())
}
pub fn open(app: &tauri::AppHandle, at_cursor: bool) -> HostResult<()> {
    let started = Instant::now();
    let panel = app
        .get_webview_panel("capture")
        .map_err(|_| "capture_window_unavailable")?;
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
                .workspace_draft(
                    s.prefs
                        .topic_id
                        .as_ref()
                        .map(|id| format!("discussion:{id}"))
                        .as_deref()
                        .unwrap_or("quick_input"),
                )
                .map_err(crate::errors::HostError::from)?
                .is_none_or(|d| d.body.is_empty())
            {
                s.prefs.source_app = front
                    .as_ref()
                    .and_then(|p| p.localizedName())
                    .map(|n| n.to_string())
                    .unwrap_or_default();
            }
        }
        let persist_started = Instant::now();
        desktop.persist(&s.prefs)?;
        diagnostic("entry_prefs_ms", persist_started.elapsed().as_millis());
        s.expanded = true;
        s.handoff = None;
        s.generation += 1;
        s.opened = Some(started);
        s.ready_ms = None;
        discussion = s.prefs.topic_id.is_some();
    }
    layout(
        app,
        500.0,
        if discussion { 620.0 } else { 260.0 },
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
        .map_err(|_| "capture_window_unavailable")?;
    let was_key = panel.as_panel().isKeyWindow();
    let (pid, visible, receipt) = {
        let desktop = app.state::<Desktop>();
        let mut s = desktop.inner.lock().unwrap();
        s.expanded = false;
        s.handoff = None;
        s.generation += 1;
        s.drag = None;
        (s.previous_pid, s.prefs.visible, s.receipt.is_some())
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
    let w = app
        .get_webview_window("main")
        .ok_or("main_window_unavailable")?;
    w.show()
        .and_then(|_| w.set_focus())
        .and_then(|_| w.as_ref().set_focus())
        .map_err(|_| "main_window_unavailable".into())
}
pub(crate) fn parse_shortcut(text: &str) -> HostResult<Shortcut> {
    if text.len() > 100 {
        return Err("shortcut_invalid".into());
    }
    let shortcut: Shortcut = text.parse().map_err(|_| "shortcut_invalid")?;
    if !shortcut
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::SUPER | Modifiers::ALT)
        || shortcut.mods.contains(Modifiers::CONTROL | Modifiers::ALT)
    {
        return Err("shortcut_modifiers_required".into());
    }
    if shortcut.key == Code::Space
        || (shortcut.mods.contains(Modifiers::SUPER)
            && matches!(shortcut.key, Code::KeyQ | Code::KeyW | Code::Tab))
    {
        return Err("shortcut_reserved".into());
    }
    Ok(shortcut)
}

fn optional_shortcut(text: &str) -> HostResult<Option<Shortcut>> {
    if text.is_empty() {
        Ok(None)
    } else {
        parse_shortcut(text).map(Some)
    }
}

fn register(app: &tauri::AppHandle, text: &str) -> HostResult<()> {
    let Some(shortcut) = optional_shortcut(text)? else {
        return Ok(());
    };
    if app.global_shortcut().is_registered(shortcut) {
        return Err("shortcut_taken".into());
    }
    app.global_shortcut()
        .on_shortcut(shortcut, |app, shortcut, e| {
            let shortcut = *shortcut;
            let app = app.clone();
            let h = app.clone();
            let _ = app.run_on_main_thread(move || {
                {
                    let d = h.state::<Desktop>();
                    let mut s = d.inner.lock().unwrap();
                    if optional_shortcut(&s.prefs.shortcut).ok().flatten() != Some(shortcut) {
                        return;
                    }
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
                } else if let Err(e) = open(&h, true) {
                    set_error(&h, e.to_string());
                }
            });
        })
        .map_err(|_| "shortcut_register_failed".into())
}
pub(crate) fn set_error(app: &tauri::AppHandle, error: String) {
    app.state::<Desktop>().inner.lock().unwrap().error = Some(error);
    publish(app);
}
pub(crate) fn update_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let p = app.state::<Desktop>().inner.lock().unwrap().prefs.clone();
    let status = MenuItem::with_id(
        app,
        "status",
        crate::i18n::text(app, "statusReady"),
        false,
        None::<&str>,
    )?;
    let input = MenuItem::with_id(
        app,
        "quick-input",
        crate::i18n::text(app, "quickInput"),
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(
        app,
        "open",
        crate::i18n::text(app, "open"),
        true,
        None::<&str>,
    )?;
    let visible = MenuItem::with_id(
        app,
        "leaf",
        if p.visible {
            crate::i18n::text(app, "hideCompanion")
        } else {
            crate::i18n::text(app, "showCompanion")
        },
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        crate::i18n::text(app, "settings"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        crate::i18n::text(app, "quit"),
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status, &sep, &input, &open, &visible, &settings, &sep2, &quit,
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
    apply_context(&app.state::<Desktop>(), read_context(app.handle()));
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
                    "quick-input" => open(&h, true),
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
                    "quit" => {
                        request_quit(&h);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(e) = result {
                    set_error(&h, e.to_string());
                }
            });
        })
        .build(app)?;
    crate::i18n::update_native(app.handle());
    let p = app.state::<Desktop>().inner.lock().unwrap().prefs.clone();
    if let Err(e) = register(app.handle(), &p.shortcut) {
        set_error(app.handle(), e.to_string());
    }
    if !p.login_initialized {
        let handle = app.handle().clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) =
                objc2::rc::autoreleasepool(|_| set_login_enabled(&handle, true, true))
            {
                set_error(&handle, error.to_string());
            }
        });
    }
    layout(app.handle(), 72.0, 76.0, Placement::SavedLeaf)?;
    if p.visible {
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
    pinned: Option<bool>,
    topic_id: Option<String>,
    clear_topic: Option<bool>,
}
fn change(app: &tauri::AppHandle, patch: Patch) -> HostResult<()> {
    let desktop = app.state::<Desktop>();
    if desktop.inner.lock().unwrap().modal_open
        && (patch.topic_id.is_some() || patch.clear_topic == Some(true))
    {
        return Err("desktop_dialog_active".into());
    }
    let old = desktop.inner.lock().unwrap().prefs.clone();
    let mut p = old.clone();
    let mut next_topic = None;
    let shortcut_requested = patch.shortcut.is_some();
    if let Some(v) = patch.shortcut {
        optional_shortcut(&v)?;
        p.shortcut = v;
    }
    if let Some(v) = patch.visible {
        p.visible = v;
    }
    if let Some(v) = patch.pinned {
        p.pinned = v;
    }
    if let Some(id) = patch.topic_id {
        let topic = app
            .state::<Workspace>()
            .store
            .conversation(&id)
            .map_err(crate::errors::HostError::from)?;
        next_topic = Some(Some(topic));
        p.topic_id = Some(id);
    }
    if patch.clear_topic == Some(true) {
        next_topic = Some(None);
        p.topic_id = None;
    }
    let changed = old.shortcut != p.shortcut;
    let retry = shortcut_requested
        && !changed
        && optional_shortcut(&p.shortcut)?
            .is_some_and(|shortcut| !app.global_shortcut().is_registered(shortcut));
    if retry {
        register(app, &p.shortcut)?;
    }
    if changed {
        register(app, &p.shortcut)?;
        if let Some(shortcut) = optional_shortcut(&old.shortcut)?
            && app.global_shortcut().unregister(shortcut).is_err()
        {
            if let Some(shortcut) = optional_shortcut(&p.shortcut)? {
                let _ = app.global_shortcut().unregister(shortcut);
            }
            return Err("shortcut_remove_failed".into());
        }
    }
    let mut s = desktop.inner.lock().unwrap();
    p.login_initialized = s.prefs.login_initialized;
    if let Err(e) = desktop.persist(&p) {
        if changed {
            if let Some(shortcut) = optional_shortcut(&p.shortcut)? {
                let _ = app.global_shortcut().unregister(shortcut);
            }
            let _ = register(app, &old.shortcut);
        }
        return Err(e);
    }
    {
        s.prefs = p.clone();
        if changed {
            s.shortcut_down = false;
        }
        if let Some(topic) = next_topic {
            s.topic = topic;
        }
        s.error = None;
    }
    drop(s);
    if !p.visible && !snapshot(app).expanded {
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
            .map_err(|_| "capture_window_unavailable")?
            .order_front_regardless();
    }
    if snapshot(app).expanded && p.topic_id != old.topic_id {
        layout(
            app,
            500.0,
            if p.topic_id.is_some() { 620.0 } else { 260.0 },
            Placement::KeepAnchor,
        )?;
    }
    update_menu(app).map_err(|_| "native_menu_unavailable")?;
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
    .map_err(|_| "window_operation_failed")?;
    rx.await.map_err(|_| "window_operation_failed")?
}
#[tauri::command]
pub async fn desktop_state(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<Snapshot> {
    require(&window)?;
    let context_app = app.clone();
    let context = tauri::async_runtime::spawn_blocking(move || read_context(&context_app))
        .await
        .map_err(|_| "desktop_status_failed")?;
    on_main(&app, move |h| {
        apply_context(&h.state::<Desktop>(), context);
        Ok(snapshot(&h))
    })
    .await
}
#[tauri::command]
pub fn desktop_modal(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    open: bool,
) -> HostResult<()> {
    if window.label() != "capture" {
        return Err("capture_window_required".into());
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
pub async fn desktop_open(app: tauri::AppHandle, window: tauri::WebviewWindow) -> HostResult<()> {
    require(&window)?;
    let at_cursor = window.label() == "main";
    on_main(&app, move |h| open(&h, at_cursor)).await
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
        return Err("window_size_invalid".into());
    }
    on_main(&app, move |h| {
        let desktop = h.state::<Desktop>();
        let s = desktop.inner.lock().unwrap();
        if !s.expanded || s.generation != generation || (s.prefs.topic_id.is_some()) {
            return Ok(());
        }
        drop(s);
        layout(&h, 500.0, height.clamp(260.0, 620.0), Placement::KeepTop)
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
pub(crate) async fn remember_topic(app: &tauri::AppHandle, topic: &Conversation) -> HostResult<()> {
    let topic = topic.clone();
    on_main(app, move |app| {
        app.state::<Desktop>().remember_topic(topic);
        publish(&app);
        Ok(())
    })
    .await
}

pub(crate) async fn record_completed(
    app: &tauri::AppHandle,
    topic_id: &str,
    memory_id: String,
) -> HostResult<()> {
    let topic_id = topic_id.to_owned();
    let receipt_id = memory_id.clone();
    let accepted = on_main(app, move |h| {
        let desktop = h.state::<Desktop>();
        let mut state = desktop.inner.lock().unwrap();
        if state.prefs.topic_id.as_deref() != Some(&topic_id) {
            return Ok(false);
        }
        state.receipt = Some(memory_id.clone());
        state.prefs.last_memory = Some(memory_id);
        if let Err(error) = desktop.persist(&state.prefs) {
            state.error = Some(error.to_string());
        }
        drop(state);
        publish(&h);
        let _ = h.emit_to("capture", "desktop-record-complete", &topic_id);
        Ok(true)
    })
    .await?;
    if accepted {
        let timer_app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(2800)).await;
            let _ = on_main(&timer_app, move |h| {
                let desktop = h.state::<Desktop>();
                let mut state = desktop.inner.lock().unwrap();
                if state.receipt.as_ref() != Some(&receipt_id) {
                    return Ok(());
                }
                state.receipt = None;
                let expanded = state.expanded;
                drop(state);
                if !expanded {
                    layout(&h, 72.0, 76.0, Placement::KeepAnchor)?;
                }
                publish(&h);
                Ok(())
            })
            .await;
        });
    }
    Ok(())
}
#[derive(Clone, Serialize)]
pub struct MainRoute {
    pub generation: u64,
    pub topic: Option<Conversation>,
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
            return Err("window_not_ready".into());
        }
        let s = snapshot(&h);
        let route = MainRoute {
            generation: s.generation,
            topic: s.topic,
            quick: true,
            record,
            settings,
        };
        h.state::<Desktop>().inner.lock().unwrap().handoff = Some(s.generation);
        if let Err(error) = show_main(&h).and_then(|_| {
            h.emit_to("main", "desktop-route", route)
                .map_err(|_| crate::errors::HostError::new("window_not_ready"))
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
        return Err("main_window_required".into());
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
        return Err("capture_window_required".into());
    }
    on_main(&app, move |h| {
        let d = h.state::<Desktop>();
        match phase.as_str() {
            "start" => {
                let origin = window.outer_position().map_err(|_| "window_drag_failed")?;
                let cursor = window.cursor_position().map_err(|_| "window_drag_failed")?;
                d.inner.lock().unwrap().drag = Some((origin, cursor));
            }
            "move" => {
                let drag = d.inner.lock().unwrap().drag;
                if let Some((origin, cursor)) = drag {
                    let now = window.cursor_position().map_err(|_| "window_drag_failed")?;
                    window
                        .set_position(tauri::PhysicalPosition::new(
                            origin.x + (now.x - cursor.x) as i32,
                            origin.y + (now.y - cursor.y) as i32,
                        ))
                        .map_err(|_| "window_drag_failed")?;
                }
            }
            "end" => {
                let pos = window
                    .outer_position()
                    .map_err(|_| "window_position_save_failed")?;
                let mut s = d.inner.lock().unwrap();
                s.drag = None;
                let size = window
                    .outer_size()
                    .map_err(|_| "window_position_save_failed")?;
                let scale = window
                    .scale_factor()
                    .map_err(|_| "window_position_save_failed")?;
                s.prefs.position = Some((
                    pos.x + size.width as i32 - (72.0 * scale) as i32,
                    pos.y + size.height as i32 - (76.0 * scale) as i32,
                ));
                d.persist(&s.prefs)?;
            }
            _ => return Err("window_drag_invalid".into()),
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn desktop_login_status(window: tauri::WebviewWindow) -> HostResult<String> {
    if window.label() != "main" {
        return Err("main_window_required".into());
    }
    // ServiceManagement synchronously talks to a system service. Query only
    // when Settings needs it, and never block AppKit's window/event thread.
    tauri::async_runtime::spawn_blocking(|| objc2::rc::autoreleasepool(|_| login_status()))
        .await
        .map_err(|_| "login_status_failed".into())
}
#[tauri::command]
pub async fn desktop_login(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    enabled: Option<bool>,
) -> HostResult<String> {
    if window.label() != "main" {
        return Err("main_window_required".into());
    }
    if let Some(enabled) = enabled {
        tauri::async_runtime::spawn_blocking(move || {
            objc2::rc::autoreleasepool(|_| set_login_enabled(&app, enabled, false))
        })
        .await
        .map_err(|_| "login_update_failed")?
    } else {
        on_main(&app, move |_| {
            unsafe {
                let _: () = msg_send![class!(SMAppService), openSystemSettingsLoginItems];
            }
            Ok(login_status())
        })
        .await
    }
}
fn set_login_enabled(app: &tauri::AppHandle, enabled: bool, initial: bool) -> HostResult<String> {
    let desktop = app.state::<Desktop>();
    let _operation = desktop.login_operation.lock().unwrap();
    if initial && desktop.inner.lock().unwrap().prefs.login_initialized {
        return Ok(login_status());
    }
    unsafe {
        let service: Retained<AnyObject> = msg_send![class!(SMAppService), mainAppService];
        let mut error: Option<Retained<objc2_foundation::NSError>> = None;
        let ok: bool = if enabled {
            msg_send![&*service,registerAndReturnError:&mut error]
        } else {
            msg_send![&*service,unregisterAndReturnError:&mut error]
        };
        if !ok {
            return Err("login_update_failed".into());
        }
    }
    let mut state = desktop.inner.lock().unwrap();
    let mut prefs = state.prefs.clone();
    prefs.login_initialized = true;
    desktop.persist(&prefs)?;
    state.prefs = prefs;
    drop(state);
    let status = login_status();
    let _ = app.emit("desktop-login-changed", &status);
    Ok(status)
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
                set_error(&app, "quit_draft_unconfirmed".into());
                if waiting.contains("capture") {
                    open(&app, true)?;
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
    fn content_resize_keeps_the_top_until_the_work_area_requires_clamping() {
        for scale in [1.0, 2.0] {
            let area = tauri::PhysicalRect {
                position: tauri::PhysicalPosition::new(-2000, 30),
                size: tauri::PhysicalSize::new(2000, 1400),
            };
            let current = tauri::PhysicalRect {
                position: tauri::PhysicalPosition::new(-1500, 150),
                size: tauri::LogicalSize::new(500.0, 300.0).to_physical(scale),
            };
            let expanded = panel_frame(
                tauri::LogicalSize::new(500.0, 337.0),
                Placement::KeepTop,
                current,
                None,
                &area,
                scale,
            );
            assert_eq!(expanded.position, current.position);
            let collapsed = panel_frame(
                tauri::LogicalSize::new(500.0, 300.0),
                Placement::KeepTop,
                expanded,
                None,
                &area,
                scale,
            );
            assert_eq!(collapsed.position, current.position);
            assert_eq!(collapsed.size, current.size);
            let at_bottom = tauri::PhysicalRect {
                position: tauri::PhysicalPosition::new(-1500, 1422 - current.size.height as i32),
                ..current
            };
            let clamped = panel_frame(
                tauri::LogicalSize::new(500.0, 337.0),
                Placement::KeepTop,
                at_bottom,
                None,
                &area,
                scale,
            );
            assert_eq!(clamped.position.x, at_bottom.position.x);
            assert_eq!(clamped.position.y + clamped.size.height as i32, 1422);
        }
    }
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
        assert!(optional_shortcut("").unwrap().is_none());
        assert!(optional_shortcut(" ").is_err());
        assert!(optional_shortcut("Control+Super+KeyM").unwrap().is_some());
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
    fn new_install_enables_login_once_and_existing_preferences_keep_system_choice() {
        let fresh = Preferences::default();
        assert!(!fresh.login_initialized);
        assert!(fresh.visible);
        assert_eq!(fresh.shortcut, "Alt+KeyM");
        let existing: Preferences =
            serde_json::from_str(r#"{"shortcut":"","visible":false}"#).unwrap();
        assert!(existing.login_initialized);
        assert!(existing.shortcut.is_empty());
        assert!(!existing.visible);
    }
    #[test]
    fn failed_preferences_read_does_not_schedule_login_initialization() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        let fresh = Desktop::new(path.clone());
        assert!(!fresh.inner.lock().unwrap().prefs.login_initialized);

        fs::write(&path, "{").unwrap();
        let corrupt = Desktop::new(path.clone());
        let state = corrupt.inner.lock().unwrap();
        assert!(state.prefs.login_initialized);
        assert_eq!(state.error.as_deref(), Some("desktop_settings_invalid"));
        assert_eq!(fs::read(&path).unwrap(), b"{");
        drop(state);

        // A directory exercises a read failure without relying on user permissions.
        let unreadable = Desktop::new(dir.path().to_path_buf());
        let state = unreadable.inner.lock().unwrap();
        assert!(state.prefs.login_initialized);
        assert_eq!(state.error.as_deref(), Some("desktop_settings_unreadable"));
    }
    #[test]
    fn preferences_roundtrip_preserves_unset_shortcut_visibility_and_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.json");
        let desktop = Desktop::new(path.clone());
        let prefs = Preferences {
            visible: false,
            shortcut: String::new(),
            pinned: true,
            position: Some((-500, 400)),
            ..Default::default()
        };
        desktop.persist(&prefs).unwrap();
        let loaded = Desktop::new(path.clone());
        let s = loaded.inner.lock().unwrap();
        assert!(!s.prefs.visible);
        assert!(s.prefs.shortcut.is_empty());
        assert!(s.prefs.pinned);
        assert_eq!(s.prefs.position, Some((-500, 400)));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn delayed_context_cleanup_preserves_newer_topic_and_preference_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = memivy_core::memory::MemoryStore::open(dir.path()).unwrap();
        let previous = uuid::Uuid::new_v4().to_string();
        let next = store
            .create_conversation(&uuid::Uuid::new_v4().to_string(), "New discussion")
            .unwrap();
        let path = dir.path().join("desktop.json");
        let desktop = Desktop::new(path.clone());
        // A background read for the old topic finishes after main-thread changes
        // have selected another discussion and changed the window preferences.
        {
            let mut state = desktop.inner.lock().unwrap();
            state.prefs.topic_id = Some(next.id.clone());
            state.prefs.pinned = true;
            state.prefs.shortcut = "Super+Shift+KeyM".into();
            state.topic = Some(next.clone());
            desktop.persist(&state.prefs).unwrap();
        }
        apply_context(
            &desktop,
            DesktopContext {
                topic_id: Some(previous),
                previous_title: None,
                topic: Some(Err(memivy_core::memory::DataError::Unavailable)),
                configured: true,
            },
        );
        {
            let state = desktop.inner.lock().unwrap();
            assert_eq!(state.topic.as_ref().unwrap().id, next.id);
            assert_eq!(state.prefs.topic_id.as_deref(), Some(next.id.as_str()));
            assert!(state.prefs.pinned && state.configured);
        }
        let saved = Desktop::new(path.clone());
        assert_eq!(
            saved.inner.lock().unwrap().prefs.topic_id,
            Some(next.id.clone())
        );

        // A current read may remove only its missing topic, retaining the latest
        // pin and shortcut settings in both memory and the persisted preferences.
        apply_context(
            &desktop,
            DesktopContext {
                topic_id: Some(next.id),
                previous_title: Some(next.title),
                topic: Some(Err(memivy_core::memory::DataError::Unavailable)),
                configured: true,
            },
        );
        assert!(desktop.inner.lock().unwrap().topic.is_none());
        let saved = Desktop::new(path);
        let prefs = &saved.inner.lock().unwrap().prefs;
        assert!(prefs.topic_id.is_none());
        assert!(prefs.pinned);
        assert_eq!(prefs.shortcut, "Super+Shift+KeyM");
    }
    #[test]
    fn failed_optional_preferences_keep_the_durable_discussion_available_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = memivy_core::memory::MemoryStore::open(dir.path()).unwrap();
        let topic = store
            .create_conversation(&uuid::Uuid::new_v4().to_string(), "Durable discussion")
            .unwrap();
        let path = dir.path().join("desktop.json");
        let desktop = Desktop::new(path.clone());
        // Block only this optional preference file, leaving SQLite writable.
        fs::create_dir(path.with_extension("json.tmp")).unwrap();
        desktop.remember_topic(topic.clone());
        let state = desktop.inner.lock().unwrap();
        assert_eq!(state.topic.as_ref().unwrap().id, topic.id);
        assert_eq!(state.prefs.topic_id.as_deref(), Some(topic.id.as_str()));
        assert_eq!(state.error.as_deref(), Some("desktop_settings_save_failed"));
        assert!(
            !path.exists(),
            "the failed preference write must not be presented as durable"
        );
        assert_eq!(store.conversation(&topic.id).unwrap().id, topic.id);
    }
    #[test]
    fn delayed_context_read_cannot_replace_a_generated_title_for_the_same_topic() {
        let dir = tempfile::tempdir().unwrap();
        let store = memivy_core::memory::MemoryStore::open(dir.path()).unwrap();
        let old = store
            .create_conversation(&uuid::Uuid::new_v4().to_string(), "New discussion")
            .unwrap();
        let desktop = Desktop::new(dir.path().join("desktop.json"));
        desktop.remember_topic(old.clone());
        let pending = DesktopContext {
            topic_id: Some(old.id.clone()),
            previous_title: Some(old.title.clone()),
            topic: Some(Ok(old.clone())),
            configured: true,
        };
        let mut generated = old.clone();
        generated.title = "每周的练习安排".into();
        assert!(desktop.refresh_topic_title(&generated));
        apply_context(&desktop, pending);
        let state = desktop.inner.lock().unwrap();
        assert_eq!(state.topic.as_ref().unwrap().title, generated.title);
        assert_eq!(state.topic.as_ref().unwrap().updated_at, old.updated_at);
        assert!(state.configured);
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
            set_error(&h, "quit_dialog_active".into());
            if window.label() == "capture" {
                open(&h, true)?;
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
