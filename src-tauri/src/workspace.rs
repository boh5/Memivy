use crate::errors::HostError;
use memivy_core::{memory::*, model::ModelConfig};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager};

pub(crate) type HostResult<T> = std::result::Result<T, HostError>;
pub(crate) struct Workspace {
    pub(crate) store: MemoryStore,
    pub(crate) config: PathBuf,
    pub(crate) restore_request: Mutex<Option<crate::backup::RestartRequest>>,
    pub(crate) exiting: AtomicBool,
    recommendation_lock: tokio::sync::Mutex<()>,
    tasks: Mutex<HashMap<String, (String, tokio::task::AbortHandle)>>,
}
pub(crate) fn model_available(state: &Workspace) -> bool {
    validate_config(&state.config).is_ok() && crate::models::read_llm(state).is_ok()
}
fn require(window: &tauri::WebviewWindow) -> HostResult<()> {
    if matches!(window.label(), "main" | "capture") {
        Ok(())
    } else {
        Err(HostError::new("forbidden"))
    }
}
pub(crate) fn require_main(window: &tauri::WebviewWindow) -> HostResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(HostError::new("main_window_required"))
    }
}
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> memivy_core::memory::Result<T> + Send + 'static,
) -> HostResult<T> {
    crate::updates::spawn_blocking(work)
        .await
        .map_err(|_| HostError::new("operation_failed"))?
        .map_err(HostError::from)
}
#[tauri::command]
async fn discussion_open(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    title: String,
    context: Vec<SourceRef>,
    collection_id: Option<String>,
) -> HostResult<Conversation> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || {
        let topic = s.create_scoped_conversation(
            &id,
            &title.chars().take(60).collect::<String>(),
            collection_id.as_deref(),
        )?;
        let draft_key = format!("discussion:{id}");
        if s.workspace_draft(&draft_key)?.is_none() {
            s.save_workspace_draft(&WorkspaceDraft {
                destination: None,
                key: draft_key,
                request_id: id.clone(),
                title: String::new(),
                body: String::new(),
                expected_version: None,
                origin: None,
                context,
            })?;
        }
        Ok(topic)
    })
    .await
}
#[tauri::command]
async fn discussion_topic(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
) -> HostResult<Conversation> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.conversation(&id)).await
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscussionUpdate {
    topic_id: String,
    input_id: String,
}

fn emit_discussion(app: &tauri::AppHandle, execution: &AgentExecution) {
    let _ = app.emit(
        "discussion-updated",
        DiscussionUpdate {
            topic_id: execution.conversation_id.clone(),
            input_id: execution.input_id.clone(),
        },
    );
}

fn launch_discussion(
    app: tauri::AppHandle,
    state: &Workspace,
    execution: AgentExecution,
    quick: bool,
    tasks: &mut HashMap<String, (String, tokio::task::AbortHandle)>,
) -> HostResult<()> {
    if execution.state != "processing" || tasks.contains_key(&execution.input_id) {
        return Ok(());
    }
    let config = validate_config(&state.config).and_then(|_| crate::models::read_llm(state));
    let config = match config {
        Ok(config) => config,
        Err(error) => {
            state.store.stop_agent_input(
                &execution.input_id,
                &execution.attempt_id,
                "failed",
                Some(error.code),
            )?;
            emit_discussion(&app, &execution);
            let _ = app.emit("resources-changed", ());
            return Ok(());
        }
    };
    let store = state.store.clone();
    let input_id = execution.input_id.clone();
    let attempt_id = execution.attempt_id.clone();
    let answer_language = crate::i18n::language(&app);
    let update_work = crate::updates::work()?;
    let task = tokio::spawn(async move {
        let _update_work = update_work;
        let update_app = app.clone();
        let update_execution = execution.clone();
        if let Err(failure) = store
            .run_discussion(
                &config,
                &execution.input_id,
                &execution.attempt_id,
                &answer_language,
                move |memory_changed| {
                    emit_discussion(&update_app, &update_execution);
                    if memory_changed {
                        let _ = update_app.emit("resources-changed", ());
                    }
                },
            )
            .await
        {
            let code = match failure {
                Failure::Network => "network",
                Failure::RateLimit => "rate_limit",
                Failure::InvalidAnswer => "invalid_answer",
                Failure::SourceUnavailable => "source_unavailable",
                Failure::ToolsUnsupported => "tools_unsupported",
                Failure::Budget => "agent_budget",
            };
            let _ = store.stop_agent_input(
                &execution.input_id,
                &execution.attempt_id,
                "failed",
                Some(code),
            );
        }
        if let Ok(mut tasks) = app.state::<Workspace>().tasks.lock()
            && tasks
                .get(&execution.input_id)
                .is_some_and(|(attempt, _)| attempt == &execution.attempt_id)
        {
            tasks.remove(&execution.input_id);
        }
        emit_discussion(&app, &execution);
        let _ = app.emit("resources-changed", ());
        if quick
            && let Ok(result) = store.agent_execution(&execution.input_id)
            && result.state == "complete"
            && let Ok(receipts) = store.agent_input_receipts(&execution.input_id)
            && let Some(memory) = receipts
                .iter()
                .find(|r| r.status == "applied")
                .and_then(|r| r.memory_id.clone())
        {
            let _ =
                crate::desktop::record_completed(&app, &execution.conversation_id, memory).await;
        }
        // The answer is already terminal and its task is released. Naming must
        // not delay sending another message or turn a successful answer into a failure.
        if matches!(
            store
                .generate_agent_title(&config, &execution.input_id, &execution.attempt_id)
                .await,
            Ok(true)
        ) {
            emit_discussion(&app, &execution);
            let _ = app.emit("resources-changed", ());
            if let Ok(topic) = store.conversation(&execution.conversation_id) {
                crate::desktop::refresh_topic_title(&app, &topic);
            }
        }
    });
    tasks.insert(input_id, (attempt_id, task.abort_handle()));
    Ok(())
}

// A redelivered IPC message uses its durable execution identity even when a
// previously selected source has since been deleted or changed.
fn persist_discussion_input(
    store: &MemoryStore,
    id: &str,
    topic_id: &str,
    text: &str,
    context: &[SourceRef],
    collection_id: Option<&str>,
    origin: Option<&Origin>,
) -> memivy_core::memory::Result<(Conversation, AgentExecution)> {
    match store.agent_execution(id) {
        Ok(saved) => {
            if saved.conversation_id != topic_id || saved.input_text != text {
                return Err(DataError::RequestConflict);
            }
            return Ok((store.conversation(topic_id)?, saved));
        }
        Err(DataError::Unavailable) => {}
        Err(error) => return Err(error),
    }
    let topic = match store.conversation(topic_id) {
        Ok(topic) => topic,
        Err(DataError::Unavailable) => store.create_scoped_conversation(
            topic_id,
            &text.chars().take(60).collect::<String>(),
            collection_id,
        )?,
        Err(error) => return Err(error),
    };
    let focus = store.agent_focused_memories(context)?;
    let execution = store.begin_agent_input(
        id,
        &uuid::Uuid::new_v4().to_string(),
        &topic.id,
        text,
        &focus,
        origin,
    )?;
    Ok((topic, execution))
}

fn fail_unlaunched_input(
    store: &MemoryStore,
    tasks: &HashMap<String, (String, tokio::task::AbortHandle)>,
    input: &str,
    code: &str,
) -> memivy_core::memory::Result<AgentExecution> {
    let execution = store.agent_execution(input)?;
    let running = tasks
        .get(input)
        .is_some_and(|(attempt, task)| attempt == &execution.attempt_id && !task.is_finished());
    if execution.state == "processing" && !running {
        store.stop_agent_input(input, &execution.attempt_id, "failed", Some(code))?;
    }
    Ok(execution)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Preserve the shared input envelope at the IPC boundary.
async fn discussion_submit(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    topic_id: Option<String>,
    text: String,
    context: Vec<SourceRef>,
    collection_id: Option<String>,
    origin: Option<Origin>,
    quick: Option<bool>,
) -> HostResult<Conversation> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let initial = topic_id.is_none();
    let topic_id = topic_id.unwrap_or_else(|| id.clone());
    let topic = {
        let _tasks = state
            .tasks
            .lock()
            .map_err(|_| HostError::new("discussion_unavailable"))?;
        let (topic, _) = persist_discussion_input(
            &state.store,
            &id,
            &topic_id,
            &text,
            &context,
            collection_id.as_deref(),
            origin.as_ref(),
        )?;
        let draft_key = format!("discussion:{}", topic.id);
        if initial && state.store.workspace_draft(&draft_key)?.is_none() {
            state.store.save_workspace_draft(&WorkspaceDraft {
                key: draft_key,
                request_id: uuid::Uuid::new_v4().to_string(),
                title: String::new(),
                body: String::new(),
                expected_version: None,
                context,
                origin,
                destination: None,
            })?;
        }
        topic
    };
    let is_quick = quick.unwrap_or(false);
    if is_quick && let Err(error) = crate::desktop::remember_topic(&app, &topic).await {
        let tasks = state
            .tasks
            .lock()
            .map_err(|_| HostError::new("discussion_unavailable"))?;
        // No task may be left processing when the required main-thread handoff
        // cannot be scheduled. An independently launched retry keeps its owner.
        let execution = fail_unlaunched_input(&state.store, &tasks, &id, error.code)?;
        emit_discussion(&app, &execution);
        let _ = app.emit("resources-changed", ());
        return Err(error);
    }
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| HostError::new("discussion_unavailable"))?;
    // Cancellation or an explicit retry may complete while waiting for the
    // main thread. Launch only the latest durable attempt under the task lock.
    let execution = state.store.agent_execution(&id)?;
    launch_discussion(app.clone(), &state, execution, is_quick, &mut tasks)?;
    let _ = app.emit("resources-changed", ());
    Ok(topic)
}

#[tauri::command]
async fn discussion_retry(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    input_id: String,
) -> HostResult<Conversation> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| HostError::new("discussion_unavailable"))?;
    if let Some((_, task)) = tasks.remove(&input_id) {
        task.abort();
    }
    let attempt = uuid::Uuid::new_v4().to_string();
    let execution = state.store.retry_agent_input(&input_id, &attempt)?;
    let topic = state.store.conversation(&execution.conversation_id)?;
    launch_discussion(
        app.clone(),
        &state,
        execution,
        window.label() == "capture",
        &mut tasks,
    )?;
    let _ = app.emit("resources-changed", ());
    Ok(topic)
}

#[tauri::command]
async fn discussion_cancel(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| HostError::new("discussion_unavailable"))?;
    let execution = state.store.agent_execution(&id)?;
    if execution.state == "processing" {
        state
            .store
            .stop_agent_input(&id, &execution.attempt_id, "cancelled", None)?;
    }
    if let Some((_, task)) = tasks.remove(&id) {
        task.abort();
    }
    emit_discussion(&app, &execution);
    let _ = app.emit("resources-changed", ());
    Ok(())
}

#[tauri::command]
async fn discussion_source(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    source: SourceRef,
    message_id: Option<String>,
) -> std::result::Result<Evidence, ReadError> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    crate::updates::spawn_blocking(move || match message_id {
        Some(id) => store.discussion_excerpt(&id, &source),
        None => store.resolve_source(&source, 4096),
    })
    .await
    .map_err(|_| ReadError::from(DataError::Database))?
    .map_err(ReadError::from)
}
#[tauri::command]
async fn discussion_save_text(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    input_id: String,
    text: String,
    title: String,
    destination: Destination,
) -> HostResult<Receipt> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.save_agent_text(&id, &input_id, &text, &title, &destination)).await
}
#[tauri::command]
async fn discussion_changes(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    input_id: String,
) -> HostResult<Vec<AgentInputChange>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.agent_input_changes(&input_id)).await
}
#[tauri::command]
async fn library_agent_changes(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    memory_id: String,
) -> HostResult<Vec<AgentChangeGroup>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.memory_agent_changes(&memory_id)).await
}
#[tauri::command]
async fn discussion_undo(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    input_id: String,
    request_id: String,
) -> HostResult<AgentUndoResult> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.undo_agent_input(&request_id, &input_id)).await
}
#[tauri::command]
async fn organization_jobs(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
) -> HostResult<Vec<OrganizationJob>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.organization_jobs(&key)).await
}
#[tauri::command]
async fn organization_retry(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    memory_id: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.retry_organization(&memory_id)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
async fn flush_organization_failure(
    store: MemoryStore,
    pending: &mut Option<(String, &'static str)>,
) -> bool {
    let Some((attempt, reason)) = pending.clone() else {
        return true;
    };
    if blocking(move || store.fail_organization(&attempt, reason))
        .await
        .is_err()
    {
        return false;
    }
    *pending = None;
    true
}

fn start_organizer(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut pending_failure = None;
        let mut delay = 750;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            let state = app.state::<Workspace>();
            if state.exiting.load(Ordering::Relaxed) {
                return;
            }
            let Ok(_update_work) = crate::updates::work() else {
                continue;
            };
            if pending_failure.is_some() {
                if !flush_organization_failure(state.store.clone(), &mut pending_failure).await {
                    delay = (delay * 2).min(5000);
                    continue;
                }
                delay = 750;
                let _ = app.emit("resources-changed", ());
            }
            // Interactive answers get priority before starting another background request.
            if state.tasks.lock().map_or(true, |tasks| !tasks.is_empty()) {
                continue;
            }
            if validate_config(&state.config).is_err() {
                continue;
            }
            let Ok(config) = crate::models::read_llm(&state) else {
                continue;
            };
            let store = state.store.clone();
            let claimant = store.clone();
            let Ok(Some(mut task)) = blocking(move || claimant.claim_organization()).await else {
                continue;
            };
            let _ = app.emit("resources-changed", ());
            let attempt = task.attempt_id.clone();
            let preparer = store.clone();
            let prepared = blocking(move || {
                preparer.prepare_organization(&mut task)?;
                Ok(task)
            })
            .await;
            let failure = match prepared {
                Err(_) => Some("invalid"),
                Ok(task) => match store.run_organization(&config, &task).await {
                    Ok(Some(receipt)) => {
                        let _ = app.emit("organization-complete", &receipt);
                        if receipt.status == "applied" {
                            let app = app.clone();
                            let config = config.clone();
                            tauri::async_runtime::spawn(async move {
                                let Ok(_update_work) = crate::updates::work() else {
                                    return;
                                };
                                let state = app.state::<Workspace>();
                                let _guard = state.recommendation_lock.lock().await;
                                if state.exiting.load(Ordering::Relaxed) {
                                    return;
                                }
                                let _ = state
                                    .store
                                    .recommend_organization_collections(
                                        &config,
                                        &receipt.request_id,
                                    )
                                    .await;
                                let _ = app.emit("resources-changed", ());
                            });
                        }
                        None
                    }
                    Ok(None) => None,
                    Err(memivy_core::model::ProbeError::Status(429)) => Some("rate_limit"),
                    Err(memivy_core::model::ProbeError::ToolsUnsupported) => {
                        Some("tools_unsupported")
                    }
                    Err(
                        memivy_core::model::ProbeError::InvalidResponse
                        | memivy_core::model::ProbeError::TooLarge,
                    ) => Some("invalid"),
                    Err(_) => Some("unavailable"),
                },
            };
            if let Some(reason) = failure {
                pending_failure = Some((attempt, reason));
                let _ = flush_organization_failure(store, &mut pending_failure).await;
            }
            let _ = app.emit("resources-changed", ());
        }
    });
}
#[tauri::command]
async fn navigation_collections(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<Collection>> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.collections()).await
}
#[tauri::command]
async fn navigation_record(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
) -> HostResult<RecordNavigation> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.record_navigation(&key)).await
}
#[tauri::command]
async fn navigation_pin(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
    pinned: bool,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.pin_record(&key, pinned)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
#[tauri::command]
async fn navigation_collect(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
    collection: String,
    included: bool,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.collect_record(&collection, &key, included)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
#[tauri::command]
async fn navigation_save_collection(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    name: String,
    description: String,
    expected: Option<i64>,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_collection(&id, &name, &description, expected)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
#[tauri::command]
async fn navigation_archive_collection(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    archived: bool,
    expected: i64,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.archive_collection(&id, archived, expected)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
#[tauri::command]
async fn organization_collections(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    receipt: String,
) -> HostResult<Vec<CollectionRecommendation>> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    let receipt_id = receipt.clone();
    if let Some(cached) =
        blocking(move || store.organization_collection_feedback(&receipt_id)).await?
    {
        return Ok(cached);
    }
    validate_config(&state.config)?;
    let config = crate::models::read_llm(&state).map_err(|_| HostError::new("model_required"))?;
    let _guard = state.recommendation_lock.lock().await;
    state
        .store
        .recommend_organization_collections(&config, &receipt)
        .await
        .map_err(|_| HostError::new("recommendation_failed"))
}
#[tauri::command]
async fn organization_states(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    keys: Vec<RecordKey>,
) -> HostResult<Vec<OrganizationState>> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.organization_states(&keys)).await
}
#[tauri::command]
async fn organization_dismiss(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    receipt: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.dismiss_organization_collections(&receipt)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
#[tauri::command]
async fn organization_collect(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    receipt: String,
    collection: String,
    revision: i64,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.accept_organization_collection(&receipt, &collection, revision)).await?;
    let _ = app.emit("resources-changed", ());
    Ok(())
}
#[tauri::command]
async fn navigation_suggest(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    collection: String,
) -> HostResult<Vec<LibraryRow>> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    validate_config(&state.config)?;
    let config = crate::models::read_llm(&state).map_err(|_| HostError::new("model_required"))?;
    state
        .store
        .suggest_collection(&config, &collection)
        .await
        .map_err(|_| HostError::new("recommendation_failed"))
}
#[tauri::command]
async fn library_changes(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    cursor: Option<ChangeCursor>,
) -> HostResult<LibraryChanges> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.library_changes(cursor.as_ref())).await
}
#[tauri::command]
async fn library_query(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    query: LibraryQuery,
) -> HostResult<LibraryPage> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library(&query)).await
}
#[tauri::command]
async fn activity_summary(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<ActivitySummary> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.activity_summary()).await
}
#[tauri::command]
async fn activity_records(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    since: i64,
    until: i64,
    offset: usize,
) -> HostResult<ActivityRecords> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.activity_records(since, until, offset)).await
}
type ReadError = HostError;
#[tauri::command]
async fn library_detail(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
    archives: Option<bool>,
) -> std::result::Result<LibraryDetail, ReadError> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let store = state.store.clone();
    crate::updates::spawn_blocking(move || {
        store.library_detail_view(&key, archives.unwrap_or(true))
    })
    .await
    .map_err(|_| ReadError::from(DataError::Database))?
    .map_err(ReadError::from)
}
#[tauri::command]
async fn library_projects(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<String>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library_projects()).await
}
#[tauri::command]
async fn library_topics(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<Conversation>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.conversations(12)).await
}
#[tauri::command]
async fn discussion_messages(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    before: Option<i64>,
) -> HostResult<Vec<Message>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.recent_messages(&id, before, 40)).await
}
#[tauri::command]
async fn draft_read(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: String,
) -> HostResult<Option<WorkspaceDraft>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.workspace_draft(&key)).await
}
#[tauri::command]
async fn draft_write(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    draft: WorkspaceDraft,
    expected_request: Option<String>,
) -> HostResult<bool> {
    require(&window)?;
    let s = state.store.clone();
    let changed_key = draft.key.clone();
    let written =
        blocking(move || s.compare_workspace_draft(&draft, expected_request.as_deref())).await?;
    if written {
        let _ = app.emit_to(
            if window.label() == "main" {
                "capture"
            } else {
                "main"
            },
            "draft-changed",
            &changed_key,
        );
    }
    Ok(written)
}
#[tauri::command]
async fn draft_clear(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: String,
    request: String,
) -> HostResult<bool> {
    require(&window)?;
    let s = state.store.clone();
    let changed_key = key.clone();
    let cleared = blocking(move || s.consume_workspace_draft(&key, &request)).await?;
    if cleared {
        let _ = app.emit_to(
            if window.label() == "main" {
                "capture"
            } else {
                "main"
            },
            "draft-changed",
            &changed_key,
        );
    }
    Ok(cleared)
}
#[tauri::command]
async fn library_capture(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    request: CaptureRequest,
) -> HostResult<CaptureResult> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    if !matches!(&request.origin,Origin::User {app, ..} if app=="Memivy") {
        return Err(HostError::new("capture_origin_invalid"));
    }
    let s = state.store.clone();
    blocking(move || s.capture(&request)).await
}
#[tauri::command]
async fn library_edit(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    draft: WorkspaceDraft,
) -> HostResult<Receipt> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_library_edit(&draft)).await
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Trash {
        key: RecordKey,
        expected: Option<String>,
    },
    Restore {
        key: RecordKey,
    },
    Purge {
        key: RecordKey,
    },
    RestoreArchive {
        request_id: String,
        memory_id: String,
        expected: String,
        capture_id: String,
    },
    RestoreVersion {
        request_id: String,
        memory_id: String,
        expected: String,
        version_id: String,
    },
    Undo {
        request_id: String,
        original_request: String,
    },
}
#[tauri::command]
async fn library_action(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    action: Action,
) -> HostResult<Option<Receipt>> {
    let _update_work = crate::updates::work()?;
    require(&window)?;
    let s = state.store.clone();
    blocking(move || {
        match action {
            Action::Trash { key, expected } => match key.kind.as_str() {
                "memory" => s.trash_memory(&key.id, &expected.ok_or(DataError::Invalid)?)?,
                "capture" => s.trash_capture(&key.id)?,
                _ => return Err(DataError::Invalid),
            },
            Action::Restore { key } => match key.kind.as_str() {
                "memory" => s.restore_memory(&key.id)?,
                "capture" => s.restore_capture(&key.id)?,
                _ => return Err(DataError::Invalid),
            },
            Action::Purge { key } => match key.kind.as_str() {
                "memory" => s.purge_memory(&key.id)?,
                "capture" => s.purge_capture(&key.id)?,
                _ => return Err(DataError::Invalid),
            },
            Action::RestoreArchive {
                request_id,
                memory_id,
                expected,
                capture_id,
            } => {
                return s
                    .restore_archive(&request_id, &memory_id, &expected, &capture_id)
                    .map(Some);
            }
            Action::RestoreVersion {
                request_id,
                memory_id,
                expected,
                version_id,
            } => {
                return s
                    .restore_version(&request_id, &memory_id, &expected, &version_id)
                    .map(Some);
            }
            Action::Undo {
                request_id,
                original_request,
            } => return s.undo(&request_id, &original_request).map(Some),
        }
        Ok(None)
    })
    .await
}
#[tauri::command]
async fn library_rebuild(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.rebuild_search_index()).await
}
#[tauri::command]
async fn memory_related(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    memory_id: String,
    expected_version: String,
    collection_id: Option<String>,
) -> HostResult<Vec<RelatedMemory>> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || {
        store.related_memories_in_collection(
            &memory_id,
            &expected_version,
            collection_id.as_deref(),
        )
    })
    .await
}
#[tauri::command]
async fn memory_export(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
    expected_version: Option<String>,
) -> HostResult<Option<String>> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let s = state.store.clone();
    let selected = key.clone();
    let detail = blocking(move || s.library_detail(&selected)).await?;
    let name: String = detail
        .title
        .chars()
        .take(60)
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    let default_name = crate::i18n::text(&app, "exportDefaultName");
    let filename = format!("{}.md", if name.is_empty() { &default_name } else { name });
    let export_message = crate::i18n::text(&app, "exportMessage");
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
        use objc2_foundation::{NSArray, NSString};
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        let panel = NSSavePanel::savePanel(mtm);
        panel.setCanCreateDirectories(true);
        panel.setNameFieldStringValue(&NSString::from_str(&filename));
        // Keep the native extension/overwrite UI without another framework dependency.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str(
            "md",
        )])));
        panel.setExtensionHidden(false);
        panel.setMessage(Some(&NSString::from_str(&export_message)));
        let path = if panel.runModal() == NSModalResponseOK {
            panel
                .URL()
                .and_then(|u| u.path())
                .map(|s| PathBuf::from(s.to_string()))
        } else {
            None
        };
        let _ = tx.send(path);
    })
    .map_err(|_| HostError::new("file_dialog_failed"))?;
    let Some(target) = rx.await.map_err(|_| HostError::new("file_dialog_failed"))? else {
        return Ok(None);
    };
    let s = state.store.clone();
    blocking(move || {
        s.export_record_markdown(&key, expected_version.as_deref(), &target)?;
        Ok(Some(target.to_string_lossy().into_owned()))
    })
    .await
}
pub(crate) fn read_model(state: &Workspace) -> HostResult<ModelConfig> {
    validate_config(&state.config)?;
    crate::models::read_llm(state).map_err(|_| HostError::new("model_required"))
}
fn validate_config(path: &std::path::Path) -> HostResult<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    crate::storage::validate_config_path(path, home.as_deref()).map_err(HostError::from)
}
#[derive(Serialize)]
struct Settings {
    configured: bool,
}
#[tauri::command]
fn workspace_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
) -> HostResult<Settings> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let settings = crate::models::load(&state)?;
    if settings.llm.is_some() {
        settings.llm_config()?;
    }
    Ok(Settings {
        configured: settings.llm.is_some(),
    })
}
#[tauri::command]
fn workspace_close(window: tauri::WebviewWindow) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    if window.label() != "main" {
        return Err(HostError::new("main_window_required"));
    }
    window
        .hide()
        .map_err(|_| HostError::new("window_hide_failed"))
}
#[tauri::command]
async fn embedding_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<EmbeddingStatus> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.embedding_status()).await
}
#[tauri::command]
async fn embedding_control(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    action: String,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.embedding_control(&action)).await
}
fn start_embedding(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let state = app.state::<Workspace>();
            if state.exiting.load(Ordering::SeqCst) {
                break;
            }
            let Ok(_update_work) = crate::updates::work() else {
                continue;
            };
            // Interactive requests take precedence over starting another batch.
            if state.tasks.lock().map_or(true, |tasks| !tasks.is_empty()) {
                continue;
            }
            if app.state::<crate::voice::Voice>().0.active() {
                continue;
            }
            state.store.embedding_tick().await;
        }
    });
}
pub fn run(context: tauri::Context<tauri::Wry>) {
    if let Err(error) = crate::storage::validate_runtime_identity(&context.config().identifier) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    let root = (|| {
        let home = std::env::var_os("HOME").ok_or("HOME is unavailable.")?;
        crate::storage::application_root(
            &context.config().identifier,
            &PathBuf::from(home),
            std::env::var_os("MEMIVY_DATA_DIR")
                .map(PathBuf::from)
                .as_deref(),
        )
    })()
    .unwrap_or_else(|error: String| {
        eprintln!("Memivy could not select the library: {error}");
        std::process::exit(1);
    });
    let version = context.package_info().version.to_string();
    let builder = tauri::Builder::default().manage(crate::updates::Updates::new(version));
    // Development has no release signing configuration and must never replace an app.
    let builder = if !cfg!(debug_assertions) && context.config().identifier == "com.memivy.app" {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    let result = builder
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(move |app| {
            // Setup errors inside the native event loop become non-unwinding panics.
            // Handle library failures explicitly, after single-instance initialization.
            let store = match (|| -> Result<MemoryStore> {
                let store = MemoryStore::open_application(&root)?;
                store.recover_interrupted_turns()?;
                store.recover_organization()?;
                Ok(store)
            })() {
                Ok(store) => store,
                Err(error) => {
                    eprintln!("Memivy could not open the library: {error}");
                    eprintln!(
                        "The library was not reset. Check its format and access permissions."
                    );
                    std::process::exit(1);
                }
            };
            app.manage(crate::models::ModelTests::default());
            let config = std::env::var_os("MEMIVY_MODEL_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|| store.model_config_path());
            app.manage(Workspace {
                store,
                config,
                restore_request: Mutex::new(None),
                exiting: AtomicBool::new(false),
                tasks: Mutex::new(HashMap::new()),
                recommendation_lock: tokio::sync::Mutex::new(()),
            });
            crate::i18n::setup(app.handle());
            crate::desktop::setup(app)?;
            crate::voice::setup(app.handle())?;
            start_organizer(app.handle().clone());
            start_embedding(app.handle().clone());
            crate::mcp::watch_library(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("workspace-close-request", ());
            }
            if let tauri::WindowEvent::Focused(true) = event {
                crate::i18n::refresh(window.app_handle());
                let _ = window.emit("resources-changed", ());
            }
        })
        .manage(crate::cleanup::CleanupJobs::default())
        .invoke_handler(tauri::generate_handler![
            crate::updates::update_status,
            crate::updates::update_check,
            crate::updates::update_download,
            crate::updates::update_install,
            crate::i18n::ui_language_snapshot,
            crate::i18n::ui_language_set,
            crate::voice::voice_status,
            crate::voice::voice_control,
            crate::voice::voice_start,
            crate::voice::voice_stop,
            crate::voice::voice_clear,
            crate::voice::voice_applied,
            crate::voice::voice_retry,
            crate::voice::voice_take_shortcut,
            embedding_status,
            embedding_control,
            crate::cleanup::cleanup_prepare,
            crate::cleanup::cleanup_generate,
            crate::cleanup::cleanup_cancel,
            crate::cleanup::cleanup_save,
            library_changes,
            library_agent_changes,
            library_query,
            activity_summary,
            activity_records,
            navigation_collections,
            navigation_record,
            navigation_pin,
            navigation_collect,
            navigation_save_collection,
            navigation_archive_collection,
            navigation_suggest,
            organization_collections,
            organization_collect,
            organization_states,
            organization_dismiss,
            discussion_open,
            discussion_topic,
            discussion_messages,
            discussion_submit,
            discussion_retry,
            discussion_cancel,
            discussion_source,
            discussion_save_text,
            discussion_changes,
            discussion_undo,
            organization_jobs,
            organization_retry,
            library_detail,
            library_projects,
            library_topics,
            draft_read,
            draft_write,
            draft_clear,
            library_capture,
            library_edit,
            library_action,
            library_rebuild,
            memory_export,
            memory_related,
            crate::backup::backup_create,
            crate::backup::backup_prepare,
            crate::backup::backup_discard,
            crate::backup::backup_restore,
            crate::backup::backup_result,
            workspace_settings,
            workspace_close,
            crate::models::models_clear,
            crate::models::models_load,
            crate::models::models_test,
            crate::models::models_apply,
            crate::models::models_organize,
            crate::mcp::mcp_settings,
            crate::mcp::mcp_set_enabled,
            crate::mcp::mcp_diagnose,
            crate::desktop::desktop_state,
            crate::desktop::desktop_modal,
            crate::desktop::desktop_update,
            crate::desktop::desktop_open,
            crate::desktop::desktop_dismiss,
            crate::desktop::desktop_ready,
            crate::desktop::desktop_composer_resize,
            crate::desktop::desktop_expand,
            crate::desktop::desktop_handoff_ready,
            crate::desktop::desktop_drag,
            crate::desktop::desktop_login,
            crate::desktop::desktop_login_status,
            crate::desktop::desktop_exit_ready
        ])
        .build(context);
    let app = match result {
        Ok(app) => app,
        Err(_) => {
            eprintln!("Memivy failed to start; check the local environment and data directory");
            std::process::exit(1);
        }
    };
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { ref api, .. } = event
            && let Some(state) = app.try_state::<Workspace>()
            && !state.exiting.load(Ordering::Relaxed)
        {
            api.prevent_exit();
            crate::desktop::request_quit(app);
        }
        if let tauri::RunEvent::Reopen { .. } = event {
            let _ = crate::desktop::show_main(app);
        }
    });
}

#[cfg(test)]
mod organization_writeback_tests {
    use super::*;
    #[tokio::test]
    async fn failed_terminal_write_is_retained_until_writer_lock_releases() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let raw = store
            .capture(&CaptureRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                text: "Synthetic lock conflict".into(),
                origin: memivy_core::memory::Origin::User {
                    app: "QA".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
        let task = store.claim_organization().unwrap().unwrap();
        let lock = rusqlite::Connection::open(store.database_path()).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut pending = Some((task.attempt_id.clone(), "storage"));
        assert!(!flush_organization_failure(store.clone(), &mut pending).await);
        assert_eq!(pending.as_ref().unwrap().0, task.attempt_id);
        lock.execute_batch("ROLLBACK").unwrap();
        assert!(flush_organization_failure(store.clone(), &mut pending).await);
        assert!(pending.is_none());
        assert_eq!(
            store
                .organization_jobs(&RecordKey {
                    kind: "memory".into(),
                    id: raw.memory_id.clone()
                })
                .unwrap()[0]
                .status,
            "failed"
        );
        store.retry_organization(&raw.memory_id).unwrap();
        assert!(store.claim_organization().unwrap().is_some());
    }
}

#[cfg(test)]
mod discussion_input_tests {
    use super::*;
    #[tokio::test]
    async fn failed_handoff_marks_only_an_unlaunched_input_terminal() {
        let root = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(root.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let (_, first) = persist_discussion_input(
            &store,
            &id,
            &id,
            "Preserve my original words",
            &[],
            None,
            None,
        )
        .unwrap();
        fail_unlaunched_input(&store, &HashMap::new(), &id, "window_operation_failed").unwrap();
        let failed = store.turn(&id).unwrap();
        assert_eq!(failed.user.text, "Preserve my original words");
        assert_eq!(failed.assistant.status, "failed");
        assert_eq!(
            failed.assistant.error_code.as_deref(),
            Some("window_operation_failed")
        );

        let retry = store
            .retry_agent_input(&id, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        let task = tokio::spawn(std::future::pending::<()>());
        let tasks = HashMap::from([(id.clone(), (retry.attempt_id.clone(), task.abort_handle()))]);
        fail_unlaunched_input(&store, &tasks, &id, "window_operation_failed").unwrap();
        assert_eq!(store.agent_execution(&id).unwrap().state, "processing");
        assert_ne!(retry.attempt_id, first.attempt_id);
        task.abort();

        store
            .stop_agent_input(&id, &retry.attempt_id, "cancelled", None)
            .unwrap();
        fail_unlaunched_input(&store, &HashMap::new(), &id, "window_operation_failed").unwrap();
        assert_eq!(store.agent_execution(&id).unwrap().state, "cancelled");
        assert_eq!(store.messages(&id, 0, 10).unwrap().len(), 2);
    }
    #[test]
    fn redelivery_reuses_durable_input_after_its_selected_source_is_deleted() {
        let root = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(root.path()).unwrap();
        let source = store
            .capture(&CaptureRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                text: "The budget is 80 yuan".into(),
                origin: Origin::User {
                    app: "QA".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
        let context = [SourceRef::Version(source.version_id.clone())];
        let id = uuid::Uuid::new_v4().to_string();
        let (_, first) = persist_discussion_input(
            &store,
            &id,
            &id,
            "  Does this plan make sense?\n",
            &context,
            None,
            None,
        )
        .unwrap();
        store
            .stop_agent_input(&id, &first.attempt_id, "failed", Some("network"))
            .unwrap();
        store
            .trash_memory(&source.memory_id, &source.version_id)
            .unwrap();
        assert!(store.agent_focused_memories(&context).unwrap().is_empty());
        let (_, replay) = persist_discussion_input(
            &store,
            &id,
            &id,
            "  Does this plan make sense?\n",
            &context,
            None,
            None,
        )
        .unwrap();
        assert_eq!(replay.attempt_id, first.attempt_id);
        assert_eq!(replay.user_message_id, first.user_message_id);
        assert_eq!(replay.focused_memory_ids, vec![source.memory_id]);
        assert_eq!(replay.state, "failed");
        assert_eq!(store.messages(&id, 0, 10).unwrap().len(), 2);
        assert!(matches!(
            persist_discussion_input(&store, &id, &id, "Another sentence", &context, None, None),
            Err(DataError::RequestConflict)
        ));
        assert!(matches!(
            persist_discussion_input(
                &store,
                &id,
                &uuid::Uuid::new_v4().to_string(),
                "  Does this plan make sense?\n",
                &context,
                None,
                None
            ),
            Err(DataError::RequestConflict)
        ));
    }
}
