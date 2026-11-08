//! Background update downloads with user-initiated installation and quit protection.
use crate::{
    errors::HostError,
    workspace::{HostResult, Workspace},
};
use memivy_core::memory::{AppUpdateGuard, DataError};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{Mutex, atomic::Ordering},
    time::{Duration, Instant},
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
    preference_error: Option<&'static str>,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}
impl Pending {
    fn prepare_install(&mut self, gate: &mut Activity, restoring: bool) -> HostResult<()> {
        if self.status.phase != "ready" || gate.preparing {
            return Err(HostError::new("update_busy"));
        }
        if restoring || gate.active != 0 {
            self.status.error = Some("update_busy");
            return Err(HostError::new("update_busy"));
        }
        gate.preparing = true;
        self.attempt = None;
        self.status.phase = "preparing";
        self.status.error = None;
        Ok(())
    }
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
        let (automatic, preference_error) = match preference {
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
                    error: None,
                },
                preference_error,
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
        let mut status = pending.status.clone();
        // Update activity must not erase a preference problem. A transient
        // operation error takes priority until the next operation clears it.
        status.error = status.error.or(pending.preference_error);
        status
    }
    fn set_automatic(&self, automatic: bool) -> HostResult<()> {
        let mut pending = self.inner.lock().unwrap();
        memivy_core::embedding::write_json(&self.path, &Preferences { automatic })
            .map_err(|_| HostError::new("update_preferences_save_failed"))?;
        pending.status.automatic = automatic;
        pending.preference_error = None;
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
            "checking" | "downloading" | "ready" | "preparing" | "closing_mcp" | "installing"
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
    let admission = {
        let restore = state.restore_request.lock().unwrap();
        let mut gate = ACTIVITY.lock().unwrap();
        let updates = app.state::<Updates>();
        let mut p = updates.inner.lock().unwrap();
        p.prepare_install(&mut gate, restore.is_some())
    };
    if let Err(error) = admission {
        emit(&app);
        return Err(error);
    }
    if app.state::<crate::voice::Voice>().0.update_busy() {
        cancel_restart(&app, "update_busy");
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
    if let Err(error) = &result {
        cancel_restart(&app, error.code);
    } else {
        emit(&app);
    }
    result
}
pub(crate) fn quit_requested(app: &tauri::AppHandle, id: u64) {
    let state = app.state::<Updates>();
    let mut p = state.inner.lock().unwrap();
    if p.status.phase == "preparing" {
        p.attempt = Some(id);
    }
}
pub(crate) fn cancel_restart(app: &tauri::AppHandle, reason: &'static str) {
    let attempt = {
        let mut gate = ACTIVITY.lock().unwrap();
        let state = app.state::<Updates>();
        let mut pending = state.inner.lock().unwrap();
        if pending.status.phase != "preparing" {
            return;
        }
        pending.status.phase = "ready";
        pending.status.error = Some(reason);
        gate.preparing = false;
        pending.attempt.take()
    };
    if let Some(id) = attempt {
        let _ = app.emit("update-resumed", id);
    }
    emit(app);
}
pub(crate) fn installing(app: &tauri::AppHandle) -> bool {
    matches!(
        app.state::<Updates>().inner.lock().unwrap().status.phase,
        "closing_mcp" | "installing"
    )
}

fn bundled_mcp_path(executable: &Path) -> io::Result<PathBuf> {
    let executable = executable.canonicalize()?;
    let directory = executable
        .parent()
        .filter(|directory| {
            directory.ends_with("Contents/MacOS")
                && directory
                    .ancestors()
                    .nth(2)
                    .and_then(Path::extension)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .ok_or_else(|| io::Error::other("Could not locate the installed app bundle"))?;
    Ok(directory.join("memivy-mcp"))
}

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, buffer_size: u32) -> i32;
}

unsafe extern "C" {
    fn geteuid() -> u32;
    fn kill(pid: i32, sig: i32) -> i32;
}

fn process_executable(pid: i32) -> io::Result<PathBuf> {
    use std::{ffi::CStr, os::unix::ffi::OsStrExt};

    // PROC_PIDPATHINFO_MAXSIZE from the macOS SDK. Use the kernel path, since
    // `ps comm` can report a relative or caller-supplied argv[0].
    const MAX_PATH: u32 = 4096;
    let mut buffer = [0u8; MAX_PATH as usize];
    // The writable buffer has the advertised size and lives through the call.
    if unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), MAX_PATH) } <= 0 {
        return Err(io::Error::last_os_error());
    }
    let path = CStr::from_bytes_until_nul(&buffer).map_err(io::Error::other)?;
    let path = Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
    if !path.is_absolute() {
        return Err(io::Error::other("Process executable path is not absolute"));
    }
    Ok(path.to_path_buf())
}

fn session_processes(lock: &Path, pid: Option<i32>, deadline: Instant) -> io::Result<Vec<i32>> {
    use std::process::{Command, Stdio};

    // Match the actual open lifetime lock file and the effective user. An
    // installed helper configured to serve another library is out of scope.
    if Instant::now() >= deadline {
        return Err(io::ErrorKind::TimedOut.into());
    }
    let uid = unsafe { geteuid() }.to_string();
    let mut command = Command::new("/usr/sbin/lsof");
    command.args(["-nP", "-t", "-a", "-u", &uid]);
    if let Some(pid) = pid {
        command.arg("-p").arg(pid.to_string());
    }
    let mut child = command
        .arg("--")
        .arg(lock)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output()?;
    if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(Vec::new());
    }
    if !output.status.success() {
        return Err(io::Error::other("Could not list MCP sessions"));
    }
    std::str::from_utf8(&output.stdout)
        .map_err(io::Error::other)?
        .lines()
        .map(|line| {
            line.parse::<i32>()
                .ok()
                .filter(|pid| *pid > 0)
                .ok_or_else(|| io::Error::other("Invalid MCP process ID"))
        })
        .collect()
}

fn stop_legacy_sessions(executable: &Path, guard: &AppUpdateGuard) -> io::Result<()> {
    const ESRCH: i32 = 3;
    const SIGTERM: i32 = 15;
    let sidecar = bundled_mcp_path(executable)?;
    let lock = guard.session_lock_path().canonicalize()?;
    let update_lock = guard.update_lock_path().canonicalize()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    for pid in session_processes(&lock, None, deadline)? {
        // Check the binary both before and immediately after checking the open
        // file to avoid signaling an unrelated process after a PID is reused.
        let is_sidecar = || match process_executable(pid) {
            Ok(path) => Ok(path == sidecar),
            Err(error) if error.raw_os_error() == Some(ESRCH) => Ok(false),
            Err(error) => Err(error),
        };
        if is_sidecar()?
            && !session_processes(&update_lock, Some(pid), deadline)?.contains(&pid)
            && session_processes(&lock, Some(pid), deadline)?.contains(&pid)
            && is_sidecar()?
        {
            // The update guard has drained database work and closed admission.
            // This is only for older sidecars that do not observe that guard.
            if unsafe { kill(pid, SIGTERM) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ESRCH) {
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn update_gate_error(error: DataError) -> HostError {
    match error {
        DataError::Busy => HostError::new("update_mcp_busy"),
        _ => HostError::new("update_install_failed"),
    }
}

pub(super) fn close_mcp_sessions(guard: &AppUpdateGuard, executable: &Path) -> HostResult<File> {
    // Give cooperative sidecars time to flush replies before falling back to
    // SIGTERM for old sessions whose database work has already finished.
    let started = Instant::now();
    loop {
        match guard.wait_for_sessions() {
            Ok(sessions) => return Ok(sessions),
            Err(DataError::Busy) if started.elapsed() < Duration::from_secs(3) => continue,
            Err(DataError::Busy) => break,
            Err(error) => return Err(update_gate_error(error)),
        }
    }
    stop_legacy_sessions(executable, guard).map_err(|_| HostError::new("update_install_failed"))?;
    let started = Instant::now();
    loop {
        match guard.wait_for_sessions() {
            Err(DataError::Busy) if started.elapsed() < Duration::from_secs(2) => continue,
            result => return result.map_err(update_gate_error),
        }
    }
}

pub(crate) fn finish_restart(app: &tauri::AppHandle) -> bool {
    let payload = {
        let state = app.state::<Updates>();
        let mut p = state.inner.lock().unwrap();
        if matches!(p.status.phase, "closing_mcp" | "installing") {
            return true;
        }
        if p.status.phase != "preparing" {
            return false;
        }
        p.status.phase = "closing_mcp";
        (p.update.clone().unwrap(), p.bytes.take().unwrap())
    };
    emit(app);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (update, bytes) = payload;
        let result = (|| -> HostResult<()> {
            let guard = app
                .state::<Workspace>()
                .store
                .prepare_app_update()
                .map_err(update_gate_error)?;
            let executable =
                std::env::current_exe().map_err(|_| HostError::new("update_install_failed"))?;
            let sessions = close_mcp_sessions(&guard, &executable)?;
            app.state::<Updates>().inner.lock().unwrap().status.phase = "installing";
            emit(&app);

            update
                .install(&bytes)
                .map_err(|_| HostError::new("update_install_failed"))?;
            app.state::<Workspace>()
                .exiting
                .store(true, Ordering::SeqCst);
            app.request_restart();
            // Keep admission closed through replacement and until this process exits.
            std::mem::forget((guard, sessions));
            Ok(())
        })();
        if let Err(error) = result {
            let attempt = {
                let mut gate = ACTIVITY.lock().unwrap();
                let state = app.state::<Updates>();
                let mut p = state.inner.lock().unwrap();
                p.bytes = Some(bytes);
                p.status.phase = "ready";
                p.status.error = Some(error.code);
                gate.preparing = false;
                p.attempt.take()
            };
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
mod legacy_process_tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_the_sidecar_slot_in_the_running_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let release_dir = dir.path().join("Memivy 测试.app/Contents/MacOS");
        fs::create_dir_all(&release_dir).unwrap();
        let executable = release_dir.join("memivy");
        fs::write(&executable, b"").unwrap();
        let alias = dir.path().join("Installed.app");
        std::os::unix::fs::symlink(dir.path().join("Memivy 测试.app"), &alias).unwrap();
        let sidecar = bundled_mcp_path(&alias.join("Contents/MacOS/memivy")).unwrap();
        assert_eq!(
            sidecar,
            executable
                .canonicalize()
                .unwrap()
                .with_file_name("memivy-mcp")
        );
        assert!(bundled_mcp_path(&dir.path().join("missing/memivy")).is_err());
    }

    #[test]
    fn kernel_process_path_resolves_the_current_executable() {
        let pid = i32::try_from(std::process::id()).unwrap();
        assert_eq!(
            process_executable(pid).unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
    }
}

#[cfg(test)]
#[path = "updates_auto_tests.rs"]
mod auto_tests;
