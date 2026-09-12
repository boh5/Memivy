//! Main-window cleanup orchestration. One cancellable, read-only model job at a time.
use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace, blocking, read_model, require_main};
use memivy_core::memory::{CleanupSave, CleanupSnapshot, Receipt, propose_cleanup};
use std::sync::Mutex;
use tauri::Emitter;

struct Job {
    id: String,
    abort: Option<tokio::task::AbortHandle>,
}
#[derive(Default)]
pub(crate) struct CleanupJobs(Mutex<Option<Job>>);

#[tauri::command]
pub(crate) async fn cleanup_prepare(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    jobs: tauri::State<'_, CleanupJobs>,
    id: String,
    memory: String,
    expected: String,
) -> HostResult<CleanupSnapshot> {
    require_main(&window)?;
    if id.is_empty() || id.len() > 64 || !id.is_ascii() {
        return Err(HostError::new("invalid"));
    }
    {
        let mut slot = jobs
            .0
            .lock()
            .map_err(|_| HostError::new("cleanup_unavailable"))?;
        if let Some(old) = slot.take().and_then(|j| j.abort) {
            old.abort();
        }
        *slot = Some(Job { id, abort: None });
    }
    let store = state.store.clone();
    blocking(move || store.prepare_cleanup(&memory, &expected)).await
}

#[tauri::command]
pub(crate) async fn cleanup_generate(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    jobs: tauri::State<'_, CleanupJobs>,
    id: String,
    snapshot: CleanupSnapshot,
    previous: Option<String>,
    instruction: String,
) -> HostResult<String> {
    require_main(&window)?;
    let config = read_model(&state)?;
    let store = state.store.clone();
    let original = snapshot.clone();
    blocking(move || store.check_cleanup(&original)).await?;
    let task = {
        let mut slot = jobs
            .0
            .lock()
            .map_err(|_| HostError::new("cleanup_unavailable"))?;
        let job = slot
            .as_mut()
            .filter(|j| j.id == id && j.abort.is_none())
            .ok_or(HostError::new("cleanup_busy"))?;
        let task = tokio::spawn(async move {
            propose_cleanup(&config, &snapshot, previous.as_deref(), &instruction)
                .await
                .map_err(HostError::from)
        });
        job.abort = Some(task.abort_handle());
        task
    };
    let result = task
        .await
        .map_err(|_| HostError::new("cancelled"))
        .and_then(|r| r);
    if let Ok(mut slot) = jobs.0.lock()
        && let Some(job) = slot.as_mut().filter(|j| j.id == id)
    {
        job.abort = None;
    }
    result
}

#[tauri::command]
pub(crate) fn cleanup_cancel(
    window: tauri::WebviewWindow,
    jobs: tauri::State<CleanupJobs>,
    id: String,
) -> HostResult<()> {
    require_main(&window)?;
    let mut slot = jobs
        .0
        .lock()
        .map_err(|_| HostError::new("cleanup_unavailable"))?;
    if slot.as_ref().is_some_and(|j| j.id == id)
        && let Some(task) = slot.take().and_then(|j| j.abort)
    {
        task.abort();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn cleanup_save(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    request: CleanupSave,
) -> HostResult<Receipt> {
    require_main(&window)?;
    let store = state.store.clone();
    let receipt = blocking(move || store.save_cleanup(&request)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(receipt)
}
