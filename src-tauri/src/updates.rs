//! User-initiated updates. A small admission gate protects the existing quit protocol.
use crate::{
    errors::HostError,
    workspace::{HostResult, Workspace},
};
use serde::Serialize;
use std::sync::{Mutex, atomic::Ordering};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
struct Activity {
    preparing: bool,
    active: usize,
}
static ACTIVITY: Mutex<Activity> = Mutex::new(Activity {
    preparing: false,
    active: 0,
});
pub(crate) struct Work;
pub(crate) fn work() -> HostResult<Work> {
    let mut gate = ACTIVITY.lock().unwrap();
    if gate.preparing {
        return Err(HostError::new("update_busy"));
    }
    gate.active += 1;
    Ok(Work)
}
pub(crate) fn spawn_blocking<F, T>(work: F) -> tauri::async_runtime::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // Keep counting the actual closure even if its awaiting command is cancelled.
    ACTIVITY.lock().unwrap().active += 1;
    let guard = Work;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        work()
    })
}
impl Drop for Work {
    fn drop(&mut self) {
        ACTIVITY.lock().unwrap().active -= 1;
    }
}
pub(crate) fn preparing() -> bool {
    ACTIVITY.lock().unwrap().preparing
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    phase: &'static str,
    current_version: String,
    version: Option<String>,
    notes: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    error: Option<&'static str>,
}
struct Pending {
    attempt: Option<u64>,
    status: Status,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}
pub(crate) struct Updates(Mutex<Pending>);
impl Updates {
    pub(crate) fn new(version: String) -> Self {
        Self(Mutex::new(Pending {
            attempt: None,
            status: Status {
                phase: "idle",
                current_version: version,
                version: None,
                notes: None,
                downloaded: 0,
                total: None,
                error: None,
            },
            update: None,
            bytes: None,
        }))
    }
}
fn emit(app: &tauri::AppHandle) {
    let status = app.state::<Updates>().0.lock().unwrap().status.clone();
    let _ = app.emit("update-status", status);
}
fn enabled(app: &tauri::AppHandle) -> bool {
    !cfg!(debug_assertions) && app.config().identifier == "com.memivy.app"
}
fn require(app: &tauri::AppHandle, window: &tauri::WebviewWindow) -> HostResult<()> {
    crate::workspace::require_main(window)?;
    if !enabled(app) {
        return Err(HostError::new("update_unavailable"));
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn update_status(app: tauri::AppHandle) -> Status {
    let mut status = app.state::<Updates>().0.lock().unwrap().status.clone();
    if !enabled(&app) {
        status.phase = "disabled";
    }
    status
}
#[tauri::command]
pub(crate) async fn update_check(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<()> {
    require(&app, &window)?;
    let _work = work()?;
    {
        let state = app.state::<Updates>();
        let mut p = state.0.lock().unwrap();
        if matches!(
            p.status.phase,
            "checking" | "downloading" | "preparing" | "installing"
        ) {
            return Err(HostError::new("update_busy"));
        }
        p.update = None;
        p.bytes = None;
        p.status.phase = "checking";
        p.status.error = None;
        p.status.version = None;
        p.status.notes = None;
    }
    emit(&app);
    let result = async {
        app.updater_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?
            .check()
            .await
    }
    .await;
    {
        let state = app.state::<Updates>();
        let mut p = state.0.lock().unwrap();
        match result {
            Ok(Some(update)) => {
                p.status.version = Some(update.version.clone());
                p.status.notes = update.body.clone();
                p.status.phase = "available";
                p.update = Some(update);
            }
            Ok(None) => p.status.phase = "current",
            Err(_) => {
                p.status.phase = "idle";
                p.status.error = Some("update_check_failed");
            }
        }
    }
    emit(&app);
    Ok(())
}
#[tauri::command]
pub(crate) async fn update_download(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<()> {
    require(&app, &window)?;
    let _work = work()?;
    let mut update = {
        let state = app.state::<Updates>();
        let mut p = state.0.lock().unwrap();
        if p.status.phase != "available" {
            return Err(HostError::new("update_busy"));
        }
        let update = p
            .update
            .clone()
            .ok_or(HostError::new("update_unavailable"))?;
        p.status.phase = "downloading";
        p.status.error = None;
        p.status.downloaded = 0;
        p.status.total = None;
        update
    };
    emit(&app);
    update.timeout = Some(std::time::Duration::from_secs(1800));
    let mut last = std::time::Instant::now();
    let result = update
        .download(
            |count, total| {
                {
                    let state = app.state::<Updates>();
                    let mut p = state.0.lock().unwrap();
                    p.status.downloaded += count as u64;
                    p.status.total = total;
                }
                if last.elapsed() >= std::time::Duration::from_millis(200) {
                    emit(&app);
                    last = std::time::Instant::now();
                }
            },
            || {},
        )
        .await;
    {
        let state = app.state::<Updates>();
        let mut p = state.0.lock().unwrap();
        match result {
            Ok(bytes) => {
                p.bytes = Some(bytes);
                p.status.phase = "ready";
            }
            Err(_) => {
                p.status.phase = "available";
                p.status.error = Some("update_download_failed");
            }
        }
    }
    emit(&app);
    Ok(())
}
#[tauri::command]
pub(crate) async fn update_install(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<()> {
    require(&app, &window)?;
    let state = app.state::<Workspace>();
    // Use the same ordering as restore reservation; never hold this guard across await.
    {
        let restore = state.restore_request.lock().unwrap();
        let mut gate = ACTIVITY.lock().unwrap();
        let updates = app.state::<Updates>();
        let mut p = updates.0.lock().unwrap();
        if restore.is_some() || gate.preparing || gate.active != 0 || p.status.phase != "ready" {
            return Err(HostError::new("update_busy"));
        }
        gate.preparing = true;
        p.attempt = None;
        p.status.phase = "preparing";
        p.status.error = None;
    }
    if app.state::<crate::voice::Voice>().0.update_busy() {
        cancel_restart(&app);
        return Err(HostError::new("update_busy"));
    }
    let result = crate::desktop::on_main(&app, |h| {
        if !crate::desktop::ready_for_update(&h) {
            return Err(HostError::new("update_busy"));
        }
        crate::desktop::request_quit(&h);
        Ok(())
    })
    .await;
    if result.is_err() {
        cancel_restart(&app);
    }
    emit(&app);
    result
}
pub(crate) fn quit_requested(app: &tauri::AppHandle, id: u64) {
    let state = app.state::<Updates>();
    let mut p = state.0.lock().unwrap();
    if p.status.phase == "preparing" {
        p.attempt = Some(id);
    }
}
pub(crate) fn cancel_restart(app: &tauri::AppHandle) {
    let state = app.state::<Updates>();
    let mut p = state.0.lock().unwrap();
    if p.status.phase != "preparing" {
        return;
    }
    p.status.phase = "ready";
    p.status.error = Some("update_busy");
    let attempt = p.attempt.take();
    drop(p);
    ACTIVITY.lock().unwrap().preparing = false;
    if let Some(id) = attempt {
        let _ = app.emit("update-resumed", id);
    }
    emit(app);
}
pub(crate) fn installing(app: &tauri::AppHandle) -> bool {
    app.state::<Updates>().0.lock().unwrap().status.phase == "installing"
}
pub(crate) fn finish_restart(app: &tauri::AppHandle) -> bool {
    let payload = {
        let state = app.state::<Updates>();
        let mut p = state.0.lock().unwrap();
        if p.status.phase == "installing" {
            return true;
        }
        if p.status.phase != "preparing" {
            return false;
        }
        p.status.phase = "installing";
        (p.update.clone().unwrap(), p.bytes.take().unwrap())
    };
    emit(app);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (update, bytes) = payload;
        let result = (|| -> HostResult<()> {
            let _locks = app
                .state::<Workspace>()
                .store
                .lock_for_app_update()
                .map_err(|_| HostError::new("update_mcp_busy"))?;
            // Lifetime locks cover new MCP clients; the process check also covers older clients.
            let output = std::process::Command::new("/bin/ps")
                .args(["-axo", "comm="])
                .output()
                .map_err(|_| HostError::new("update_mcp_busy"))?;
            if !output.status.success()
                || String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    std::path::Path::new(line.trim())
                        .file_name()
                        .is_some_and(|s| s.to_string_lossy().starts_with("memivy-mcp"))
                })
            {
                return Err(HostError::new("update_mcp_busy"));
            }

            update
                .install(&bytes)
                .map_err(|_| HostError::new("update_install_failed"))?;
            app.state::<Workspace>()
                .exiting
                .store(true, Ordering::SeqCst);
            app.request_restart();
            // Keep both cross-process locks until the process actually exits.
            std::mem::forget(_locks);
            Ok(())
        })();
        if let Err(error) = result {
            let attempt = {
                let state = app.state::<Updates>();
                let mut p = state.0.lock().unwrap();
                p.bytes = Some(bytes);
                p.status.phase = "ready";
                p.status.error = Some(error.code);
                p.attempt.take()
            };
            ACTIVITY.lock().unwrap().preparing = false;
            if let Some(id) = attempt {
                let _ = app.emit("update-resumed", id);
            }
            emit(&app);
            crate::desktop::set_error(&app, error.to_string());
        }
    });
    true
}
