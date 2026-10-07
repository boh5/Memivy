//! Background update downloads with user-initiated installation and quit protection.
use crate::{
    errors::HostError,
    workspace::{HostResult, Workspace},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, atomic::Ordering},
    time::Duration,
};
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
    revision: u64,
    current_version: String,
    automatic: bool,
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
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    automatic: bool,
}
pub(crate) struct Updates {
    path: PathBuf,
    inner: Mutex<Pending>,
}
impl Updates {
    pub(crate) fn new(version: String, path: PathBuf) -> Self {
        let preference = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Preferences>(&bytes).map_err(|_| ()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Preferences { automatic: true })
            }
            Err(_) => Err(()),
        };
        let (automatic, error) = match preference {
            Ok(preference) => (preference.automatic, None),
            Err(()) => (false, Some("update_preferences_unavailable")),
        };
        Self {
            path,
            inner: Mutex::new(Pending {
                attempt: None,
                status: Status {
                    phase: "idle",
                    revision: 0,
                    current_version: version,
                    automatic,
                    version: None,
                    notes: None,
                    downloaded: 0,
                    total: None,
                    error,
                },
                update: None,
                bytes: None,
            }),
        }
    }
    fn snapshot(&self) -> Status {
        let mut pending = self.inner.lock().unwrap();
        // Native event delivery may reorder background and main-thread events.
        // Number every returned snapshot so the UI can retain the newest one.
        pending.status.revision += 1;
        pending.status.clone()
    }
    fn set_automatic(&self, automatic: bool) -> HostResult<()> {
        let mut pending = self.inner.lock().unwrap();
        memivy_core::embedding::write_json(&self.path, &Preferences { automatic })
            .map_err(|_| HostError::new("update_preferences_save_failed"))?;
        pending.status.automatic = automatic;
        if pending.status.error == Some("update_preferences_unavailable") {
            pending.status.error = None;
        }
        Ok(())
    }
}
fn emit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let status = app.state::<Updates>().snapshot();
    let _ = app.emit("update-status", status);
}
fn enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
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
    let mut status = app.state::<Updates>().snapshot();
    if !enabled(&app) {
        status.phase = "disabled";
    }
    status
}
#[tauri::command]
pub(crate) fn update_set_automatic(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    automatic: bool,
) -> HostResult<()> {
    require(&app, &window)?;
    let _work = work()?;
    app.state::<Updates>().set_automatic(automatic)?;
    emit(&app);
    Ok(())
}

pub(crate) fn start(app: tauri::AppHandle) {
    if !enabled(&app) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            automatic_update(&app).await;
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}
async fn automatic_update<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if matches!(check(app, true).await, Ok(true)) {
        // Errors are already represented by status. Busy operations wait until
        // the next interval, and installation always needs an explicit command.
        let _ = download(app, true).await;
    }
}

#[tauri::command]
pub(crate) async fn update_check(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<()> {
    require(&app, &window)?;
    check(&app, false).await.map(|_| ())
}
async fn check<R: tauri::Runtime>(app: &tauri::AppHandle<R>, automatic: bool) -> HostResult<bool> {
    let _work = work()?;
    {
        let state = app.state::<Updates>();
        let mut p = state.inner.lock().unwrap();
        if automatic && !p.status.automatic {
            return Ok(false);
        }
        if matches!(
            p.status.phase,
            "checking" | "downloading" | "ready" | "preparing" | "installing"
        ) {
            return Err(HostError::new("update_busy"));
        }
        p.update = None;
        p.bytes = None;
        p.status.phase = "checking";
        p.status.error = None;
        p.status.version = None;
        p.status.notes = None;
        p.status.downloaded = 0;
        p.status.total = None;
    }
    emit(app);
    let result = async {
        app.updater_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?
            .check()
            .await
    }
    .await;
    let available = {
        let state = app.state::<Updates>();
        let mut p = state.inner.lock().unwrap();
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
        p.status.phase == "available"
    };
    emit(app);
    Ok(available)
}
#[tauri::command]
pub(crate) async fn update_download(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> HostResult<()> {
    require(&app, &window)?;
    download(&app, false).await
}
async fn download<R: tauri::Runtime>(app: &tauri::AppHandle<R>, automatic: bool) -> HostResult<()> {
    let _work = work()?;
    let mut update = {
        let state = app.state::<Updates>();
        let mut p = state.inner.lock().unwrap();
        // Recheck under the same lock as the phase transition: disabling during a
        // check must prevent its automatic download from starting.
        if automatic && !p.status.automatic {
            return Ok(());
        }
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
    emit(app);
    update.timeout = Some(std::time::Duration::from_secs(1800));
    let mut last = std::time::Instant::now();
    let result = update
        .download(
            |count, total| {
                {
                    let state = app.state::<Updates>();
                    let mut p = state.inner.lock().unwrap();
                    p.status.downloaded += count as u64;
                    p.status.total = total;
                }
                if last.elapsed() >= std::time::Duration::from_millis(200) {
                    emit(app);
                    last = std::time::Instant::now();
                }
            },
            || {},
        )
        .await;
    {
        let state = app.state::<Updates>();
        let mut p = state.inner.lock().unwrap();
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
    emit(app);
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
        let mut p = updates.inner.lock().unwrap();
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
    let mut p = state.inner.lock().unwrap();
    if p.status.phase == "preparing" {
        p.attempt = Some(id);
    }
}
pub(crate) fn cancel_restart(app: &tauri::AppHandle) {
    let state = app.state::<Updates>();
    let mut p = state.inner.lock().unwrap();
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
    app.state::<Updates>().inner.lock().unwrap().status.phase == "installing"
}
pub(crate) fn finish_restart(app: &tauri::AppHandle) -> bool {
    let payload = {
        let state = app.state::<Updates>();
        let mut p = state.inner.lock().unwrap();
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
                let mut p = state.inner.lock().unwrap();
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

#[cfg(test)]
#[path = "updates_auto_tests.rs"]
mod auto_tests;
