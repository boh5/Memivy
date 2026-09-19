//! Native file selection and restart coordination; database rules stay in core.
use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace, blocking};
use memivy_core::memory::{PreparedRestore, RestoreResult};
use std::{path::PathBuf, sync::atomic::Ordering};
use tauri::{Emitter, Manager};

pub(crate) struct RestartRequest {
    pub id: String,
    pub committing: bool,
}

fn main_only(window: &tauri::WebviewWindow) -> HostResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(HostError::new("main_window_required"))
    }
}
async fn select_file(app: &tauri::AppHandle, save: bool) -> HostResult<Option<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSSavePanel};
        use objc2_foundation::{NSArray, NSString};
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        let path = if save {
            let panel = NSSavePanel::savePanel(mtm);
            panel.setCanCreateDirectories(true);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            panel.setNameFieldStringValue(&NSString::from_str(&format!(
                "Memivy-backup-{timestamp}.db"
            )));
            panel.setMessage(Some(&NSString::from_str(&crate::i18n::text(
                &handle,
                "backupSave",
            ))));
            if panel.runModal() == NSModalResponseOK {
                panel
                    .URL()
                    .and_then(|u| u.path())
                    .map(|p| PathBuf::from(p.to_string()))
            } else {
                None
            }
        } else {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseDirectories(false);
            panel.setCanChooseFiles(true);
            panel.setAllowsMultipleSelection(false);
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str(
                "db",
            )])));
            panel.setMessage(Some(&NSString::from_str(&crate::i18n::text(
                &handle,
                "backupOpen",
            ))));
            if panel.runModal() == NSModalResponseOK {
                panel
                    .URL()
                    .and_then(|u| u.path())
                    .map(|p| PathBuf::from(p.to_string()))
            } else {
                None
            }
        };
        let _ = tx.send(path);
    })
    .map_err(|_| HostError::new("file_dialog_failed"))?;
    rx.await.map_err(|_| HostError::new("file_dialog_failed"))
}
#[tauri::command]
pub async fn backup_create(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Option<String>> {
    let _update_work = crate::updates::work()?;
    main_only(&window)?;
    let Some(path) = select_file(&app, true).await? else {
        return Ok(None);
    };
    let store = state.store.clone();
    blocking(move || {
        store.backup(&path)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}
#[tauri::command]
pub async fn backup_prepare(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Option<PreparedRestore>> {
    let _update_work = crate::updates::work()?;
    main_only(&window)?;
    let Some(path) = select_file(&app, false).await? else {
        return Ok(None);
    };
    let store = state.store.clone();
    blocking(move || store.prepare_restore(&path).map(Some)).await
}
#[tauri::command]
pub async fn backup_discard(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    main_only(&window)?;
    let store = state.store.clone();
    blocking(move || store.discard_prepared_restore(&id)).await
}
#[tauri::command]
pub async fn backup_restore(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    id: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    main_only(&window)?;
    {
        let state = app.state::<Workspace>();
        let mut request = state.restore_request.lock().unwrap();
        if request.is_some() {
            return Err(HostError::new("restore_busy"));
        }
        *request = Some(RestartRequest {
            id,
            committing: false,
        });
    }
    crate::desktop::on_main(&app, |h| {
        crate::desktop::request_quit(&h);
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn backup_result(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Option<RestoreResult>> {
    let _update_work = crate::updates::work()?;
    main_only(&window)?;
    let store = state.store.clone();
    blocking(move || store.last_restore_result()).await
}
pub(crate) fn cancel_restart(app: &tauri::AppHandle) {
    let state = app.state::<Workspace>();
    if let Some(request) = state.restore_request.lock().unwrap().take() {
        let id = request.id;
        let store = state.store.clone();
        tauri::async_runtime::spawn(async move {
            let _ = blocking(move || store.discard_prepared_restore(&id)).await;
        });
        let _ = app.emit("restore-cancelled", ());
    }
}
pub(crate) fn finish_restart(app: &tauri::AppHandle) -> bool {
    let state = app.state::<Workspace>();
    let id = {
        let mut pending = state.restore_request.lock().unwrap();
        let Some(request) = pending.as_mut() else {
            return false;
        };
        if request.committing {
            return true;
        }
        request.committing = true;
        request.id.clone()
    };
    let store = state.store.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let staged = id.clone();
        match blocking(move || store.arm_restore(&staged)).await {
            Ok(()) => {
                app.state::<Workspace>()
                    .exiting
                    .store(true, Ordering::Relaxed);
                app.request_restart();
            }
            Err(message) => {
                app.state::<Workspace>()
                    .restore_request
                    .lock()
                    .unwrap()
                    .take();
                let store = app.state::<Workspace>().store.clone();
                let _ = blocking(move || store.discard_prepared_restore(&id)).await;
                let _ = app.emit("restore-cancelled", ());
                crate::desktop::set_error(&app, message.to_string());
            }
        }
    });
    true
}
