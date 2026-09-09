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

pub(crate) type HostResult<T> = std::result::Result<T, String>;
pub(crate) struct Workspace {
    pub(crate) store: MemoryStore,
    config: PathBuf,
    config_lock: Mutex<()>,
    pub(crate) restore_request: Mutex<Option<crate::backup::RestartRequest>>,
    pub(crate) exiting: AtomicBool,
    recommendation_lock: tokio::sync::Mutex<()>,
    tasks: Mutex<HashMap<String, tokio::task::AbortHandle>>,
}
pub(crate) fn model_available(state: &Workspace) -> bool {
    validate_config(&state.config).is_ok() && ModelConfig::read(&state.config).is_ok()
}
fn require(window: &tauri::WebviewWindow) -> HostResult<()> {
    if matches!(window.label(), "main" | "capture") {
        Ok(())
    } else {
        Err("窗口无权执行此操作".into())
    }
}
fn require_main(window: &tauri::WebviewWindow) -> HostResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("请在主窗口完成此操作".into())
    }
}
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> memivy_core::memory::Result<T> + Send + 'static,
) -> HostResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "本地操作未完成，请重试".to_string())?
        .map_err(|e| e.to_string())
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
#[allow(clippy::too_many_arguments)] // Three framework arguments; retain the shared main/panel IPC contract.
async fn discussion_ask(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    topic_id: String,
    question: String,
    context: Vec<SourceRef>,
    collection_id: Option<String>,
) -> HostResult<Conversation> {
    require(&window)?;
    validate_config(&state.config)?;
    let config = ModelConfig::read(&state.config)
        .map_err(|_| "请先连接模型；问题草稿已保留，记录与搜索仍可使用".to_string())?;
    config.endpoint().map_err(|e| e.to_string())?;
    if context.len() > 4 {
        return Err("一次最多选择 4 条记忆".into());
    }
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "讨论暂时不可用".to_string())?;
    let store = state.store.clone();
    // Retried command delivery reuses the attempt, never starts another request.
    if let Ok(old) = store.turn(&id) {
        let _ = old;
        let previous = store.conversation(&topic_id).map_err(|e| e.to_string())?;
        if (collection_id.is_some() || id == topic_id) && previous.collection_id != collection_id {
            return Err(DataError::RequestConflict.to_string());
        }
        store
            .start_turn(&id, &topic_id, &question, &context)
            .map_err(|e| e.to_string())?;
        return store.conversation(&topic_id).map_err(|e| e.to_string());
    }
    let topic = match store.conversation(&topic_id) {
        Ok(topic) => {
            if collection_id.is_some() && topic.collection_id != collection_id {
                return Err(DataError::Conflict.to_string());
            }
            topic
        }
        Err(DataError::Unavailable) => store
            .create_scoped_conversation(
                &topic_id,
                &question.chars().take(60).collect::<String>(),
                collection_id.as_deref(),
            )
            .map_err(|e| e.to_string())?,
        Err(e) => return Err(e.to_string()),
    };
    let turn = store
        .start_turn(&id, &topic_id, &question, &context)
        .map_err(|e| e.to_string())?;
    let task_id = id.clone();
    let _ = app.emit("library-refresh", ());
    let task = tokio::spawn(async move {
        if let Err(failure) = store
            .answer_discussion(&config, &topic_id, &turn, &context)
            .await
        {
            let _ = store.fail_turn(&task_id, failure);
        }
        if let Ok(mut tasks) = app.state::<Workspace>().tasks.lock() {
            tasks.remove(&task_id);
        }
        let _ = app.emit("library-refresh", ());
    });
    tasks.insert(id, task.abort_handle());
    Ok(topic)
}
#[tauri::command]
fn discussion_cancel(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
    id: String,
) -> HostResult<()> {
    require(&window)?;
    state.store.cancel_turn(&id).map_err(|e| e.to_string())?;
    if let Some(task) = state
        .tasks
        .lock()
        .map_err(|_| "讨论暂时不可用".to_string())?
        .remove(&id)
    {
        task.abort();
    }
    let _ = app.emit("library-refresh", ());
    Ok(())
}
#[tauri::command]
async fn discussion_source(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    source: SourceRef,
    message_id: Option<String>,
) -> HostResult<Evidence> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || match message_id {
        Some(id) => s.discussion_excerpt(&id, &source),
        None => s.resolve_source(&source, 4096),
    })
    .await
}
#[tauri::command]
async fn discussion_save(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    request: ConclusionRequest,
    merged_body: Option<String>,
) -> HostResult<Receipt> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_reviewed_conclusion(&request, merged_body.as_deref())).await
}
#[tauri::command]
async fn discussion_merge(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    destination: Destination,
    text: String,
) -> HostResult<String> {
    require(&window)?;
    validate_config(&state.config)?;
    let config = ModelConfig::read(&state.config).map_err(|e| e.to_string())?;
    state
        .store
        .preview_conclusion_merge(&config, &destination, &text)
        .await
        .map_err(|_| "融合预览未完成，原文与结论仍保留。请检查模型连接或缩短内容后重试。".into())
}
#[tauri::command]
async fn discussion_targets(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    sources: Vec<SourceRef>,
) -> HostResult<Vec<(String, String)>> {
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.discussion_targets(&sources)).await
}
#[tauri::command]
async fn organization_jobs(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
) -> HostResult<Vec<OrganizationJob>> {
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.organization_jobs(&key)).await
}
#[tauri::command]
async fn organization_retry(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    capture_id: String,
) -> HostResult<()> {
    require(&window)?;
    let store = state.store.clone();
    blocking(move || store.retry_organization(&capture_id)).await?;
    let _ = app.emit("library-refresh", ());
    Ok(())
}
#[tauri::command]
async fn organization_new(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    capture_id: String,
    request_id: String,
    original_request: Option<String>,
) -> HostResult<Receipt> {
    require(&window)?;
    let store = state.store.clone();
    let receipt = blocking(move || {
        let raw = store.capture_by_id(&capture_id)?;
        let request = ChangeRequest {
            request_id,
            capture_id,
            destination: Destination::New,
            title: raw
                .text
                .lines()
                .find(|s| !s.trim().is_empty())
                .unwrap_or("新记忆")
                .chars()
                .take(60)
                .collect(),
            body: raw.text,
            actor: Actor::User,
        };
        match original_request {
            Some(id) => store.correct_assignment(&id, &request),
            None => store.capture_as_new(&request.request_id, &request.capture_id),
        }
    })
    .await?;
    let _ = app.emit("library-refresh", ());
    Ok(receipt)
}

// Retain terminal writes until SQLite accepts them. Retrying this state never
// repeats the model call; shutdown recovery handles a still-pending attempt.
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
            if pending_failure.is_some() {
                if !flush_organization_failure(state.store.clone(), &mut pending_failure).await {
                    delay = (delay * 2).min(5000);
                    continue;
                }
                delay = 750;
                let _ = app.emit("library-refresh", ());
            }
            // Interactive answers get priority before starting another background request.
            if state.tasks.lock().map_or(true, |tasks| !tasks.is_empty()) {
                continue;
            }
            if validate_config(&state.config).is_err() {
                continue;
            }
            let Ok(config) = ModelConfig::read(&state.config) else {
                continue;
            };
            let store = state.store.clone();
            let claimant = store.clone();
            let Ok(Some(mut task)) = blocking(move || claimant.claim_organization()).await else {
                continue;
            };
            let _ = app.emit("library-refresh", ());
            let attempt = task.attempt_id.clone();
            let preparer = store.clone();
            let prepared = blocking(move || {
                preparer.prepare_organization(&mut task)?;
                Ok(task)
            })
            .await;
            let failure = match prepared {
                Err(_) => Some("invalid"),
                Ok(task) => match store.propose_organization(&config, &task).await {
                    Ok(proposal) => {
                        let writer = store.clone();
                        match tauri::async_runtime::spawn_blocking(move || {
                            writer.apply_organization(&task, &proposal)
                        })
                        .await
                        {
                            Ok(Ok(receipt)) => {
                                let _ = app.emit("organization-complete", &receipt);
                                if receipt.status == "applied" {
                                    let app = app.clone();
                                    let config = config.clone();
                                    tauri::async_runtime::spawn(async move {
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
                                        let _ = app.emit("library-refresh", ());
                                    });
                                }
                                None
                            }
                            Ok(Err(DataError::Invalid)) => Some("invalid"),
                            Ok(Err(DataError::Conflict | DataError::Unavailable)) => {
                                Some("conflict")
                            }
                            _ => Some("storage"),
                        }
                    }
                    Err(memivy_core::model::ProbeError::Status(429)) => Some("rate_limit"),
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
            let _ = app.emit("library-refresh", ());
        }
    });
}
#[tauri::command]
async fn navigation_collections(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<Collection>> {
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
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.pin_record(&key, pinned)).await?;
    let _ = app.emit("library-refresh", ());
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
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.collect_record(&collection, &key, included)).await?;
    let _ = app.emit("library-refresh", ());
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
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.save_collection(&id, &name, &description, expected)).await?;
    let _ = app.emit("library-refresh", ());
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
    require_main(&window)?;
    let s = state.store.clone();
    blocking(move || s.archive_collection(&id, archived, expected)).await?;
    let _ = app.emit("library-refresh", ());
    Ok(())
}
#[tauri::command]
async fn organization_collections(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    receipt: String,
) -> HostResult<Vec<CollectionRecommendation>> {
    require_main(&window)?;
    let store = state.store.clone();
    let receipt_id = receipt.clone();
    if let Some(cached) =
        blocking(move || store.organization_collection_feedback(&receipt_id)).await?
    {
        return Ok(cached);
    }
    validate_config(&state.config)?;
    let config =
        ModelConfig::read(&state.config).map_err(|_| "连接模型后可重试专题推荐".to_string())?;
    let _guard = state.recommendation_lock.lock().await;
    state
        .store
        .recommend_organization_collections(&config, &receipt)
        .await
        .map_err(|_| "专题推荐暂未完成，记忆已保存。可重试，或在记忆页手动选择专题。".to_string())
}
#[tauri::command]
async fn organization_states(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    keys: Vec<RecordKey>,
) -> HostResult<Vec<OrganizationState>> {
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
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.dismiss_organization_collections(&receipt)).await?;
    let _ = app.emit("library-refresh", ());
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
    require_main(&window)?;
    let store = state.store.clone();
    blocking(move || store.accept_organization_collection(&receipt, &collection, revision)).await?;
    let _ = app.emit("library-refresh", ());
    Ok(())
}
#[tauri::command]
async fn navigation_suggest(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    collection: String,
) -> HostResult<Vec<LibraryRow>> {
    require_main(&window)?;
    validate_config(&state.config)?;
    let config = ModelConfig::read(&state.config)
        .map_err(|_| "连接模型后即可推荐，专题和记忆保持原样".to_string())?;
    state
        .store
        .suggest_collection(&config, &collection)
        .await
        .map_err(|_| "推荐未完成，请检查模型连接后重试".to_string())
}
#[tauri::command]
async fn library_query(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    query: LibraryQuery,
) -> HostResult<LibraryPage> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library(&query)).await
}
#[tauri::command]
async fn library_detail(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    key: RecordKey,
) -> HostResult<LibraryDetail> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library_detail(&key)).await
}
#[tauri::command]
async fn library_projects(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<String>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.library_projects()).await
}
#[tauri::command]
async fn library_topics(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Vec<Conversation>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.conversations(12)).await
}
#[tauri::command]
async fn library_messages(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    before: Option<i64>,
) -> HostResult<Vec<Message>> {
    require(&window)?;
    let s = state.store.clone();
    blocking(move || s.messages(&id, before.unwrap_or(0), 40)).await
}
#[tauri::command]
async fn discussion_messages(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    before: Option<i64>,
) -> HostResult<Vec<Message>> {
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
            (),
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
    let cleared = blocking(move || s.consume_workspace_draft(&key, &request)).await?;
    if cleared {
        let _ = app.emit_to(
            if window.label() == "main" {
                "capture"
            } else {
                "main"
            },
            "draft-changed",
            (),
        );
    }
    Ok(cleared)
}
#[tauri::command]
async fn library_capture(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    request: CaptureRequest,
) -> HostResult<RawCapture> {
    require(&window)?;
    if !matches!(&request.origin,Origin::User {app, ..} if app=="Memivy") {
        return Err("主窗口只能保存你主动输入的记录".into());
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
    let filename = format!("{}.md", if name.is_empty() { "记忆" } else { name });
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
        panel.setMessage(Some(&NSString::from_str(
            "将这一篇的标题和当前正文保存为 Markdown。",
        )));
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
    .map_err(|_| "无法打开保存窗口".to_string())?;
    let Some(target) = rx.await.map_err(|_| "文件选择未完成".to_string())? else {
        return Ok(None);
    };
    let s = state.store.clone();
    blocking(move || {
        s.export_record_markdown(&key, expected_version.as_deref(), &target)?;
        Ok(Some(target.to_string_lossy().into_owned()))
    })
    .await
}
fn validate_config(path: &std::path::Path) -> HostResult<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    crate::storage::validate_config_path(path, home.as_deref())
}
#[derive(Serialize, Default)]
struct Settings {
    base_url: String,
    model: String,
    has_key: bool,
    configured: bool,
    disable_reasoning: bool,
}
#[tauri::command]
fn workspace_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
) -> HostResult<Settings> {
    require_main(&window)?;
    validate_config(&state.config)?;
    if !state.config.exists() {
        return Ok(Settings::default());
    }
    let c = ModelConfig::read(&state.config).map_err(|e| e.to_string())?;
    Ok(Settings {
        configured: true,
        base_url: c.base_url,
        model: c.model,
        has_key: c.api_key.is_some_and(|s| !s.is_empty()),
        disable_reasoning: c.disable_reasoning,
    })
}
#[tauri::command]
fn workspace_configure(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
    base_url: String,
    model: String,
    api_key: Option<String>,
    disable_reasoning: bool,
    replace_unreadable: bool,
) -> HostResult<()> {
    require_main(&window)?;
    let _lock = state
        .config_lock
        .lock()
        .map_err(|_| "配置正忙".to_string())?;
    let path = &state.config;
    validate_config(path)?;
    let previous = if path.exists() {
        match ModelConfig::read(path) {
            Ok(c) => Some(c),
            Err(_) if replace_unreadable => None,
            Err(e) => return Err(e.to_string()),
        }
    } else {
        None
    };
    let key = api_key.or_else(|| {
        previous.and_then(|c| {
            if c.base_url == base_url {
                c.api_key
            } else {
                None
            }
        })
    });
    ModelConfig {
        base_url,
        model,
        api_key: key,
        disable_reasoning,
    }
    .save(path)
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn workspace_test_model(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<memivy_core::model::ProbeReport> {
    require_main(&window)?;
    validate_config(&state.config)?;
    let c = ModelConfig::read(&state.config).map_err(|e| e.to_string())?;
    memivy_core::model::probe(c, std::time::Duration::from_secs(45))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn workspace_close(window: tauri::WebviewWindow) -> HostResult<()> {
    if window.label() != "main" {
        return Err("仅主窗口可以隐藏主窗口".into());
    }
    window.hide().map_err(|_| "主窗口无法隐藏".into())
}
pub fn run(context: tauri::Context<tauri::Wry>) {
    let result = tauri::Builder::default()
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let store = MemoryStore::open_application(MemoryStore::environment_root()?)?;
            let config = std::env::var_os("MEMIVY_MODEL_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|| store.model_config_path());
            store.recover_interrupted_turns()?;
            store.recover_organization()?;
            // Carry forward an existing local setup only for the normal app,
            // never for an isolated QA directory or an explicit config override.
            if !config.exists()
                && std::env::var_os("MEMIVY_DATA_DIR").is_none()
                && std::env::var_os("MEMIVY_MODEL_CONFIG").is_none()
                && let Some(home) = std::env::var_os("HOME")
            {
                let previous = PathBuf::from(home)
                    .join("Library/Application Support/com.memivy.phase1/model.json");
                if validate_config(&previous).is_ok()
                    && validate_config(&config).is_ok()
                    && let Ok(settings) = ModelConfig::read(&previous)
                {
                    let _ = settings.save(&config);
                }
            }
            app.manage(Workspace {
                store,
                config,
                config_lock: Mutex::new(()),
                restore_request: Mutex::new(None),
                exiting: AtomicBool::new(false),
                tasks: Mutex::new(HashMap::new()),
                recommendation_lock: tokio::sync::Mutex::new(()),
            });
            crate::desktop::setup(app)?;
            start_organizer(app.handle().clone());
            crate::mcp::watch_library(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("workspace-close-request", ());
            }
            if let tauri::WindowEvent::Focused(true) = event {
                let _ = window.emit("library-refresh", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            library_query,
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
            discussion_messages,
            discussion_ask,
            discussion_cancel,
            discussion_source,
            discussion_save,
            discussion_merge,
            discussion_targets,
            organization_jobs,
            organization_retry,
            organization_new,
            library_detail,
            library_projects,
            library_topics,
            library_messages,
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
            workspace_configure,
            workspace_test_model,
            workspace_close,
            crate::mcp::mcp_settings,
            crate::mcp::mcp_set_enabled,
            crate::mcp::mcp_diagnose,
            crate::desktop::desktop_state,
            crate::desktop::desktop_modal,
            crate::desktop::desktop_update,
            crate::desktop::desktop_open,
            crate::desktop::desktop_dismiss,
            crate::desktop::desktop_ready,
            crate::desktop::desktop_capture,
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
            eprintln!("Memivy 启动失败，请检查本地环境和数据目录");
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
                text: "合成锁冲突".into(),
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
                    kind: "capture".into(),
                    id: raw.id.clone()
                })
                .unwrap()[0]
                .status,
            "failed"
        );
        store.retry_organization(&raw.id).unwrap();
        assert!(store.claim_organization().unwrap().is_some());
    }
}
