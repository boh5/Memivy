use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace, require_main};
use memivy_core::{
    model::ModelConfig,
    models::{Binding, Connection, Registry, Source},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
#[derive(Default)]
pub(crate) struct ModelTests(Mutex<Vec<Proof>>);
struct Proof {
    token: String,
    revision: String,
    kind: String,
    binding: String,
    at: Instant,
    connection: Option<Connection>,
    capabilities: Option<memivy_core::model::tools::Capabilities>,
}
#[derive(Serialize)]
pub(crate) struct View {
    revision: String,
    connections: Vec<ConnectionView>,
    llm: Option<Binding>,
    embedding: Binding,
    voice: Binding,
    auto_organize: bool,
}
#[derive(Serialize)]
struct ConnectionView {
    id: String,
    name: String,
    base_url: String,
    has_key: bool,
}
fn view(r: Registry) -> View {
    View {
        revision: r.revision,
        connections: r
            .connections
            .into_iter()
            .map(|c| ConnectionView {
                id: c.id,
                name: c.name,
                base_url: c.base_url,
                has_key: c.api_key.is_some_and(|k| !k.is_empty()),
            })
            .collect(),
        llm: r.llm,
        embedding: r.embedding,
        voice: r.voice,
        auto_organize: r.auto_organize,
    }
}
fn load(state: &Workspace) -> HostResult<Registry> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    crate::storage::validate_config_path(
        &state
            .store
            .database_path()
            .parent()
            .unwrap()
            .join("models.json"),
        home.as_deref(),
    )?;
    crate::storage::validate_config_path(&state.config, home.as_deref())?;
    Registry::with_legacy(state.store.database_path().parent().unwrap(), &state.config)
        .map_err(HostError::from)
}
pub(crate) fn read_llm(state: &Workspace) -> HostResult<ModelConfig> {
    if Registry::exists(state.store.database_path().parent().unwrap()) {
        Registry::read(state.store.database_path().parent().unwrap())?
            .llm_config()
            .map_err(HostError::from)
    } else {
        ModelConfig::read(&state.config).map_err(HostError::from)
    }
}
fn changed(window: &tauri::WebviewWindow) {
    let _ = window.app_handle().emit("settings-changed", ());
}
#[tauri::command]
pub(crate) fn models_load(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
) -> HostResult<View> {
    require_main(&window)?;
    load(&state).map(view)
}
#[derive(Deserialize)]
pub(crate) struct ConnectionEdit {
    id: String,
    name: String,
    base_url: String,
    api_key: Option<String>,
    remove: bool,
}
#[derive(Serialize)]
pub(crate) struct TestResult {
    token: String,
    binding: Binding,
    message: String,
    message_params: serde_json::Value,
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn models_test(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    tests: tauri::State<'_, ModelTests>,
    revision: String,
    kind: String,
    mut binding: Binding,
    connection: Option<ConnectionEdit>,
) -> HostResult<TestResult> {
    require_main(&window)?;
    let mut r = load(&state)?;
    if r.revision != revision {
        return Err(HostError::new("configuration_conflict"));
    }
    let candidate = if let Some(edit) = connection {
        if edit.remove {
            return Err(HostError::new("model_configuration"));
        }
        let url = edit.base_url.trim().trim_end_matches('/').to_string();
        let previous = r
            .connections
            .iter()
            .find(|c| c.id == edit.id && c.base_url.trim_end_matches('/') == url);
        let key = match edit.api_key {
            Some(k) => {
                if k.is_empty() {
                    None
                } else {
                    Some(k)
                }
            }
            None => previous.and_then(|c| c.api_key.clone()),
        };
        let c = Connection {
            id: uuid::Uuid::new_v4().to_string(),
            name: edit.name.trim().into(),
            base_url: url,
            api_key: key,
        };
        binding.connection = c.id.clone();
        r.connections.push(c.clone());
        Some(c)
    } else {
        None
    };
    let m = r.resolve(&binding)?;
    let mut capabilities = None;
    let mut message_params = serde_json::json!({});
    let message = match kind.as_str() {
        "llm" => {
            let c = memivy_core::model::tools::probe(&m)
                .await
                .map_err(HostError::from)?;
            capabilities = Some(c.clone());
            if c.supports_agent() {
                "model_test_agent"
            } else {
                "model_test_agent_unsupported"
            }
            .to_string()
        }
        "embedding" => {
            let prefix = binding.query_prefix.clone();
            let dimension = tauri::async_runtime::spawn_blocking(move || {
                let v = memivy_core::models::embed(
                    &m,
                    "Memivy connection test: a personal note.",
                    None,
                    Duration::from_secs(15),
                )?;
                memivy_core::models::embed(
                    &m,
                    &format!("{prefix}Find a personal note."),
                    Some(v.len()),
                    Duration::from_secs(15),
                )?;
                Ok::<_, String>(v.len())
            })
            .await
            .map_err(|_| HostError::new("model_test_failed"))??;
            binding.dimensions = Some(dimension);
            {
                message_params = serde_json::json!({"dimension": dimension});
                "model_test_embedding".into()
            }
        }
        "voice" => {
            tauri::async_runtime::spawn_blocking(move || {
                memivy_core::models::transcribe(&m, &vec![0.; 16000])
            })
            .await
            .map_err(|_| HostError::new("speech_test_failed"))??;
            "model_test_speech_silence".into()
        }
        _ => return Err(HostError::new("invalid")),
    };
    if load(&state)?.revision != revision {
        return Err(HostError::new("configuration_conflict"));
    }
    let token = uuid::Uuid::new_v4().to_string();
    let mut proofs = tests
        .0
        .lock()
        .map_err(|_| HostError::new("model_test_unavailable"))?;
    proofs.retain(|p| p.at.elapsed() < Duration::from_secs(600));
    if proofs.len() >= 32 {
        proofs.remove(0);
    }
    proofs.push(Proof {
        token: token.clone(),
        revision,
        kind,
        binding: serde_json::to_string(&binding)
            .map_err(|_| HostError::new("model_configuration"))?,
        at: Instant::now(),
        connection: candidate,
        capabilities,
    });
    Ok(TestResult {
        token,
        binding,
        message,
        message_params,
    })
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn models_apply(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    tests: tauri::State<'_, ModelTests>,
    revision: String,
    kind: String,
    binding: Option<Binding>,
    token: Option<String>,
    confirmed: bool,
) -> HostResult<View> {
    require_main(&window)?;
    let mut r = load(&state)?;
    if r.revision != revision {
        return Err(HostError::new("configuration_conflict"));
    }
    let previous = r.clone();
    let mut capabilities = None;
    if let Some(b) = &binding
        && b.source == Source::Service
    {
        let serialized =
            serde_json::to_string(b).map_err(|_| HostError::new("model_configuration"))?;
        let proofs = tests
            .0
            .lock()
            .map_err(|_| HostError::new("model_test_unavailable"))?;
        let proof = proofs
            .iter()
            .find(|p| {
                Some(&p.token) == token.as_ref()
                    && p.revision == revision
                    && p.kind == kind
                    && p.binding == serialized
                    && p.at.elapsed() < Duration::from_secs(600)
            })
            .ok_or(HostError::new("model_test_required"))?;
        capabilities = proof.capabilities.clone();
        if let Some(connection) = &proof.connection {
            r.connections.push(connection.clone());
        }
    }
    let root = state.store.database_path().parent().unwrap().to_owned();
    match kind.as_str() {
        "llm" => {
            if binding.as_ref().is_some_and(|b| b.source == Source::Local) {
                return Err(HostError::new("model_configuration"));
            }
            if binding.is_some()
                && !capabilities
                    .as_ref()
                    .is_some_and(|caps| caps.supports_agent())
            {
                return Err(HostError::new("model_tools_unsupported"));
            }
            r.llm = binding;
            prune_unused(&mut r, window.app_handle());
            r.save(&root, &revision)?;
            if let Some(caps) = capabilities {
                let config = r.llm_config()?;
                if let Err(e) = memivy_core::model::tools::save_capabilities(&root, &config, &caps)
                {
                    let error = rollback_config(&root, &r, previous, e.into());
                    changed(&window);
                    return Err(error);
                }
            }
        }
        "embedding" => {
            if let Some(b) = binding {
                if !confirmed {
                    return Err(HostError::new("index_confirmation_required"));
                }
                r.embedding = b;
                prune_unused(&mut r, window.app_handle());
                let store = state.store.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    store.apply_embedding_model(&mut r, &revision)?;
                    Ok::<_, String>(r)
                })
                .await
                .map_err(|_| HostError::new("embedding_settings_failed"))?;
                changed(&window);
                r = result?;
            } else {
                let store = state.store.clone();
                crate::workspace::blocking(move || store.embedding_control("disable")).await?;
            }
        }
        "voice" => {
            if let Some(b) = binding {
                r.voice = b;
                window
                    .app_handle()
                    .state::<crate::voice::Voice>()
                    .0
                    .repair_retained_credentials(&mut r)?;
                prune_unused(&mut r, window.app_handle());
                r.save(&root, &revision)?;
                if let Err(e) = crate::voice::set_enabled(window.app_handle(), true).await {
                    let error = rollback_config(&root, &r, previous, e);
                    changed(&window);
                    return Err(error);
                }
            } else {
                crate::voice::set_enabled(window.app_handle(), false).await?;
            }
        }
        _ => return Err(HostError::new("invalid")),
    }
    changed(&window);
    Ok(view(r))
}
#[tauri::command]
pub(crate) fn models_organize(
    window: tauri::WebviewWindow,
    state: tauri::State<Workspace>,
    revision: String,
    enabled: bool,
) -> HostResult<View> {
    require_main(&window)?;
    let mut r = load(&state)?;
    r.auto_organize = enabled;
    r.save(state.store.database_path().parent().unwrap(), &revision)?;
    changed(&window);
    Ok(view(r))
}

#[tauri::command]
pub(crate) async fn models_clear(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    kind: String,
) -> HostResult<()> {
    require_main(&window)?;
    if kind == "voice" {
        return crate::voice::clear_model(window.app_handle()).await;
    }
    if kind != "embedding" {
        return Err(HostError::new("invalid"));
    }
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = store.database_path().parent().unwrap().to_owned();
        let _writer = memivy_core::embedding::lock(&root, "embedding-writer.lock")?;
        if Registry::read(&root)?.embedding.source == Source::Local {
            store
                .embedding_control("disable")
                .map_err(HostError::from)?;
        }
        memivy_core::embedding::cache::HfModelCache::for_user()?.clear()
    })
    .await
    .map_err(|_| HostError::new("model_delete_failed"))?
    .map_err(|_| HostError::new("model_delete_failed"))
}

fn prune_unused(r: &mut Registry, app: &tauri::AppHandle) {
    let used: Vec<String> = r
        .llm
        .iter()
        .chain([&r.embedding, &r.voice])
        .filter(|b| b.source == Source::Service)
        .map(|b| b.connection.clone())
        .collect();
    r.connections.retain(|c| {
        used.contains(&c.id) || app.state::<crate::voice::Voice>().0.uses_connection(&c.id)
    });
}

fn rollback_config(
    root: &std::path::Path,
    candidate: &Registry,
    mut previous: Registry,
    error: HostError,
) -> HostError {
    if previous.save(root, &candidate.revision).is_ok() {
        error
    } else {
        HostError::new("configuration_restore_failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_rollback_restores_config_but_never_overwrites_a_newer_save() {
        let dir = tempfile::tempdir().unwrap();
        let mut previous = Registry::default();
        previous.save(dir.path(), "initial").unwrap();
        let mut candidate = previous.clone();
        candidate.auto_organize = false;
        candidate.save(dir.path(), &previous.revision).unwrap();
        assert_eq!(
            rollback_config(
                dir.path(),
                &candidate,
                previous.clone(),
                HostError::new("model_configuration")
            )
            .code,
            "model_configuration"
        );
        let mut newer = Registry::read(dir.path()).unwrap();
        assert!(newer.auto_organize);
        assert_ne!(newer.revision, previous.revision);
        let revision = newer.revision.clone();
        newer.auto_organize = false;
        newer.save(dir.path(), &revision).unwrap();
        assert_eq!(
            rollback_config(
                dir.path(),
                &candidate,
                previous,
                HostError::new("model_configuration")
            )
            .code,
            "configuration_restore_failed"
        );
        assert_eq!(Registry::read(dir.path()).unwrap().revision, newer.revision);
    }
}
