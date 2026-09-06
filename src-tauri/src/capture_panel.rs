use tauri::Manager;
use tauri_nspanel::WebviewWindowExt;

tauri_nspanel::tauri_panel! {
    panel!(CapturePanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true,
            becomes_key_only_if_needed: false,
            hides_on_deactivate: false
        }
    })
}

/// Called during setup on the main thread; keep the Tauri webview and IPC label.
pub fn configure(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("capture")
        .expect("configured capture window");
    let panel = window.to_panel::<CapturePanel>()?;
    panel.set_style_mask(objc2_app_kit::NSWindowStyleMask::NonactivatingPanel);
    panel.set_level(tauri_nspanel::PanelLevel::Floating.value());
    panel.set_collection_behavior(
        objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllSpaces
            | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary
            | objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllApplications,
    );
    Ok(())
}
