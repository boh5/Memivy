//! Explicit, read-only context export from the main window.
use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace, blocking, read_model, require_main};
use memivy_core::memory::{ContextSelection, ContextSnapshot, propose_context_export};
use std::sync::Mutex;

struct Job {
    id: String,
    abort: Option<tokio::task::AbortHandle>,
}

#[derive(Default)]
pub(crate) struct ContextExportJobs(Mutex<Option<Job>>);

impl ContextExportJobs {
    fn start(&self, id: String) -> HostResult<()> {
        if uuid::Uuid::parse_str(&id).is_err() {
            return Err(HostError::new("invalid"));
        }
        let mut slot = self
            .0
            .lock()
            .map_err(|_| HostError::new("context_export_unavailable"))?;
        if let Some(old) = slot.take().and_then(|job| job.abort) {
            old.abort();
        }
        *slot = Some(Job { id, abort: None });
        Ok(())
    }

    fn cancel(&self, id: &str) -> HostResult<()> {
        let mut slot = self
            .0
            .lock()
            .map_err(|_| HostError::new("context_export_unavailable"))?;
        if slot.as_ref().is_some_and(|job| job.id == id)
            && let Some(task) = slot.take().and_then(|job| job.abort)
        {
            task.abort();
        }
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn context_export_prepare(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    selections: Vec<ContextSelection>,
) -> HostResult<ContextSnapshot> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.prepare_context_export(&selections)).await
}

#[tauri::command]
pub(crate) fn context_export_start(
    window: tauri::WebviewWindow,
    jobs: tauri::State<'_, ContextExportJobs>,
    id: String,
) -> HostResult<()> {
    require_main(&window)?;
    jobs.start(id)
}

#[tauri::command]
pub(crate) async fn context_export_generate(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    jobs: tauri::State<'_, ContextExportJobs>,
    id: String,
    snapshot: ContextSnapshot,
    language: String,
) -> HostResult<String> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let config = read_model(&state)?;
    let store = state.store.clone();
    let original = snapshot.clone();
    blocking(move || store.check_context_export(&original)).await?;
    let model_snapshot = snapshot.clone();
    let task = {
        let mut slot = jobs
            .0
            .lock()
            .map_err(|_| HostError::new("context_export_unavailable"))?;
        let job = slot
            .as_mut()
            .filter(|job| job.id == id && job.abort.is_none())
            .ok_or(HostError::new("cancelled"))?;
        let task = tokio::spawn(async move {
            propose_context_export(&config, &model_snapshot, &language)
                .await
                .map_err(HostError::from)
        });
        job.abort = Some(task.abort_handle());
        task
    };
    let result = task
        .await
        .map_err(|_| HostError::new("cancelled"))
        .and_then(|result| result);
    let store = state.store.clone();
    let result = match result {
        Ok(body) => blocking(move || store.check_context_export(&snapshot))
            .await
            .map(|()| body),
        Err(error) => Err(error),
    };
    let mut slot = jobs
        .0
        .lock()
        .map_err(|_| HostError::new("context_export_unavailable"))?;
    if !slot.as_ref().is_some_and(|job| job.id == id) {
        return Err(HostError::new("cancelled"));
    }
    slot.take();
    result
}

#[tauri::command]
pub(crate) fn context_export_cancel(
    window: tauri::WebviewWindow,
    jobs: tauri::State<'_, ContextExportJobs>,
    id: String,
) -> HostResult<()> {
    require_main(&window)?;
    jobs.cancel(&id)
}

#[tauri::command]
pub(crate) async fn context_export_validate(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    snapshot: ContextSnapshot,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.check_context_export(&snapshot)).await
}

#[tauri::command]
pub(crate) async fn context_export_copy(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    snapshot: ContextSnapshot,
    body: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    if body.trim().is_empty() || body.len() > 160 * 1024 {
        return Err(HostError::new("context_export_budget"));
    }
    let store = state.store.clone();
    blocking(move || store.check_context_export(&snapshot)).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        use objc2_foundation::NSString;
        let clipboard = NSPasteboard::generalPasteboard();
        clipboard.clearContents();
        // The AppKit constant has process lifetime; clipboard access stays on
        // the main thread and does not depend on WebView gesture activation.
        let copied = clipboard.setString_forType(&NSString::from_str(&body), unsafe {
            NSPasteboardTypeString
        });
        let _ = tx.send(copied);
    })
    .map_err(|_| HostError::new("context_export_clipboard"))?;
    if rx
        .await
        .map_err(|_| HostError::new("context_export_clipboard"))?
    {
        Ok(())
    } else {
        Err(HostError::new("context_export_clipboard"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_removes_reservations_before_model_start() {
        let jobs = ContextExportJobs::default();
        let id = uuid::Uuid::new_v4().to_string();
        jobs.start(id.clone()).unwrap();
        jobs.cancel(&id).unwrap();
        assert!(jobs.0.lock().unwrap().is_none());
    }

    #[test]
    fn old_cancellation_does_not_remove_new_job() {
        let jobs = ContextExportJobs::default();
        let old = uuid::Uuid::new_v4().to_string();
        let new = uuid::Uuid::new_v4().to_string();
        jobs.start(old.clone()).unwrap();
        jobs.start(new.clone()).unwrap();
        jobs.cancel(&old).unwrap();
        assert_eq!(jobs.0.lock().unwrap().as_ref().unwrap().id, new);
    }
}
