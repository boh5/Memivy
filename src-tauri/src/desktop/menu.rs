//! Shared desktop actions for the menu bar and the floating icon.
use super::{
    Desktop, HostResult, Patch, change, open, request_quit, set_error, show_main, snapshot,
};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

fn item(app: &tauri::AppHandle, id: &str, label: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, id, crate::i18n::text(app, label), true, None::<&str>)
}

pub(crate) fn update_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let visible = app.state::<Desktop>().inner.lock().unwrap().prefs.visible;
    let status = MenuItem::with_id(
        app,
        "status",
        crate::i18n::text(app, "statusReady"),
        false,
        None::<&str>,
    )?;
    let input = item(app, "quick-input", "quickInput")?;
    let main = item(app, "open", "open")?;
    let leaf = item(
        app,
        "leaf",
        if visible {
            "hideCompanion"
        } else {
            "showCompanion"
        },
    )?;
    let settings = item(app, "settings", "settings")?;
    let quit = item(app, "quit", "quit")?;
    let separator = PredefinedMenuItem::separator(app)?;
    let footer = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status, &separator, &input, &main, &leaf, &settings, &footer, &quit,
        ],
    )?;
    if let Some(tray) = app.tray_by_id("memivy") {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

pub(super) fn popup(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    x: f64,
    y: f64,
) -> HostResult<()> {
    if snapshot(app).expanded {
        return Ok(());
    }
    let build = || -> tauri::Result<()> {
        let input = item(app, "leaf-input", "quickInput")?;
        let main = item(app, "open", "open")?;
        let separator = PredefinedMenuItem::separator(app)?;
        let hide = item(app, "leaf", "hideCompanion")?;
        let menu = Menu::with_items(app, &[&input, &main, &separator, &hide])?;
        window.popup_menu_at(&menu, tauri::LogicalPosition::new(x, y))
    };
    build().map_err(|_| "desktop_menu_failed".into())
}

pub(super) fn handle_event(app: &tauri::AppHandle, event: MenuEvent) {
    let h = app.clone();
    let id = event.id.as_ref().to_string();
    let _ = app.run_on_main_thread(move || {
        let result = match id.as_str() {
            "quick-input" => open(&h, true),
            "leaf-input" => open(&h, false),
            "open" => show_main(&h),
            "settings" => {
                let result = show_main(&h);
                let _ = h.emit_to("main", "desktop-settings", ());
                result
            }
            "leaf" => change(
                &h,
                Patch {
                    visible: Some(!snapshot(&h).visible),
                    ..Default::default()
                },
            ),
            "quit" => {
                request_quit(&h);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            set_error(&h, error.to_string());
        }
    });
}
