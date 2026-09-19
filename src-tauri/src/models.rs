use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace, require_main};
use memivy_core::{
    model::ModelConfig,
    models::{Binding, ModelSettings, Source},
};
use serde::Serialize;
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
    candidate: Binding,
}
#[derive(Serialize)]
pub(crate) struct View {
    revision: String,
    llm: Option<BindingView>,
    embedding: BindingView,
    voice: BindingView,
    auto_organize: bool,
}
#[derive(Serialize)]
pub(crate) struct BindingView {
    #[serde(flatten)]
    binding: Binding,
    has_key: bool,
}
impl From<Binding> for BindingView {
    fn from(mut binding: Binding) -> Self {
        let has_key = binding.api_key.take().is_some_and(|k| !k.is_empty());
        Self { binding, has_key }
    }
}
fn view(r: ModelSettings) -> View {
    View {
        revision: r.revision,
        llm: r.llm.map(Into::into),
        embedding: r.embedding.into(),
        voice: r.voice.into(),
        auto_organize: r.auto_organize,
    }
}
pub(crate) fn load(state: &Workspace) -> HostResult<ModelSettings> {
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
    ModelSettings::with_legacy(state.store.database_path().parent().unwrap(), &state.config)
        .map_err(HostError::from)
}
pub(crate) fn read_llm(state: &Workspace) -> HostResult<ModelConfig> {
    if ModelSettings::exists(state.store.database_path().parent().unwrap()) {
        ModelSettings::read(state.store.database_path().parent().unwrap())?
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
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    load(&state).map(view)
}
#[derive(Serialize)]
pub(crate) struct TestResult {
    token: Option<String>,
    binding: BindingView,
    message: String,
    message_params: serde_json::Value,
}
fn prepare_binding(binding: &mut Binding, previous: Option<&Binding>) {
    binding.base_url = binding.base_url.trim().trim_end_matches('/').to_string();
    if binding.api_key.is_none() {
        binding.api_key = previous
            .filter(|b| {
                b.provider == binding.provider
                    && b.base_url.trim().trim_end_matches('/') == binding.base_url
            })
            .and_then(|b| b.api_key.clone());
    } else if binding.api_key.as_deref() == Some("") {
        binding.api_key = None;
    }
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
) -> HostResult<TestResult> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let r = load(&state)?;
    if r.revision != revision {
        return Err(HostError::new("configuration_conflict"));
    }
    let previous = match kind.as_str() {
        "llm" => r.llm.as_ref(),
        "embedding" => Some(&r.embedding),
        "voice" => Some(&r.voice),
        _ => return Err(HostError::new("invalid")),
    };
    prepare_binding(&mut binding, previous);
    let m = binding.model_config()?;
    let mut message_params = serde_json::json!({});
    let message = match kind.as_str() {
        "llm" => {
            let c = memivy_core::model::tools::probe(&m)
                .await
                .map_err(HostError::from)?;
            if c.supports_agent() {
                "model_test_agent"
            } else {
                "model_test_agent_unsupported"
            }
            .to_string()
        }
        "embedding" => {
            let dimension = crate::updates::spawn_blocking(move || {
                let v = memivy_core::models::embed(
                    &m,
                    "Memivy connection test: a personal note.",
                    None,
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
            crate::updates::spawn_blocking(move || {
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
    if kind == "llm" {
        return Ok(TestResult {
            token: None,
            binding: binding.into(),
            message,
            message_params,
        });
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
    let public = BindingView::from(binding.clone());
    proofs.push(Proof {
        token: token.clone(),
        revision,
        kind,
        binding: serde_json::to_string(&public.binding)
            .map_err(|_| HostError::new("model_configuration"))?,
        at: Instant::now(),
        candidate: binding,
    });
    Ok(TestResult {
        token: Some(token),
        binding: public,
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
    mut binding: Option<Binding>,
    token: Option<String>,
    confirmed: bool,
) -> HostResult<View> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let mut r = load(&state)?;
    if r.revision != revision {
        return Err(HostError::new("configuration_conflict"));
    }
    let previous = r.clone();
    if let Some(b) = &binding
        && b.source == Source::Service
        && kind != "llm"
    {
        let serialized = serde_json::to_string(&BindingView::from(b.clone()).binding)
            .map_err(|_| HostError::new("model_configuration"))?;
        let proofs = tests
            .0
            .lock()
            .map_err(|_| HostError::new("model_test_unavailable"))?;
        let proof = proofs.iter().find(|p| {
            Some(&p.token) == token.as_ref()
                && p.revision == revision
                && p.kind == kind
                && p.binding == serialized
                && p.at.elapsed() < Duration::from_secs(600)
        });
        let proof = proof.ok_or(HostError::new("model_test_required"))?;
        binding = Some(proof.candidate.clone());
    }
    let root = state.store.database_path().parent().unwrap().to_owned();
    match kind.as_str() {
        "llm" => {
            if binding.as_ref().is_some_and(|b| b.source == Source::Local) {
                return Err(HostError::new("model_configuration"));
            }
            if let Some(b) = &mut binding {
                prepare_binding(b, r.llm.as_ref());
                b.model_config()?;
            }
            r.llm = binding;
            r.save(&root, &revision)?;
        }
        "embedding" => {
            if let Some(b) = binding {
                if !confirmed {
                    return Err(HostError::new("index_confirmation_required"));
                }
                r.embedding = b;
                let store = state.store.clone();
                let result = crate::updates::spawn_blocking(move || {
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
                let next = b;
                window
                    .app_handle()
                    .state::<crate::voice::Voice>()
                    .0
                    .retain_recording_config(&mut r, &next)?;
                r.voice = next;
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
    let _update_work = crate::updates::work()?;
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
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    if kind == "voice" {
        return crate::voice::clear_model(window.app_handle()).await;
    }
    if kind != "embedding" {
        return Err(HostError::new("invalid"));
    }
    let store = state.store.clone();
    crate::updates::spawn_blocking(move || {
        let root = store.database_path().parent().unwrap().to_owned();
        let _writer = memivy_core::embedding::lock(&root, "embedding-writer.lock")?;
        if ModelSettings::read(&root)?.embedding.source == Source::Local {
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

fn rollback_config(
    root: &std::path::Path,
    candidate: &ModelSettings,
    mut previous: ModelSettings,
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
    fn public_views_and_test_signatures_never_contain_credentials() {
        let private = Binding {
            source: Source::Service,
            base_url: "http://localhost:1234/v1".into(),
            model: "fixture-model".into(),
            api_key: Some("private-fixture-key".into()),
            ..Binding::default()
        };
        let response = BindingView::from(private.clone());
        let bytes = serde_json::to_string(&response).unwrap();
        assert!(!bytes.contains("private-fixture-key"));
        assert!(!bytes.contains("api_key"));
        assert!(response.has_key);
        let submitted: Binding = serde_json::from_str(&bytes).unwrap();
        assert_eq!(
            serde_json::to_string(&submitted).unwrap(),
            serde_json::to_string(&response.binding).unwrap()
        );
        let all = view(ModelSettings {
            llm: Some(private.clone()),
            voice: private.clone(),
            embedding: private,
            ..ModelSettings::default()
        });
        assert!(
            !serde_json::to_string(&all)
                .unwrap()
                .contains("private-fixture-key")
        );
    }
    #[test]
    fn untested_binding_preserves_only_same_provider_endpoint_credentials() {
        let previous = Binding {
            source: Source::Service,
            base_url: "https://example.com/v1".into(),
            model: "old-model".into(),
            api_key: Some("fixture-key".into()),
            ..Binding::default()
        };
        let mut candidate = Binding {
            base_url: " https://example.com/v1/ ".into(),
            model: "new-model".into(),
            api_key: None,
            ..previous.clone()
        };
        prepare_binding(&mut candidate, Some(&previous));
        assert_eq!(candidate.api_key, previous.api_key);
        let dir = tempfile::tempdir().unwrap();
        let mut settings = ModelSettings {
            llm: Some(candidate.clone()),
            ..ModelSettings::default()
        };
        settings.save(dir.path(), "initial").unwrap();
        assert_eq!(
            ModelSettings::read(dir.path()).unwrap().llm.unwrap().model,
            "new-model"
        );
        candidate.api_key = None;
        candidate.provider = memivy_core::model::Provider::Gemini;
        prepare_binding(&mut candidate, Some(&previous));
        assert!(candidate.api_key.is_none());
        candidate.provider = previous.provider;
        candidate.base_url = "https://different.example/v1".into();
        prepare_binding(&mut candidate, Some(&previous));
        assert!(candidate.api_key.is_none());
        candidate.base_url = previous.base_url.clone();
        candidate.api_key = Some(String::new());
        prepare_binding(&mut candidate, Some(&previous));
        assert!(candidate.api_key.is_none());
    }

    #[test]
    fn activation_rollback_restores_config_but_never_overwrites_a_newer_save() {
        let dir = tempfile::tempdir().unwrap();
        let mut previous = ModelSettings::default();
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
        let mut newer = ModelSettings::read(dir.path()).unwrap();
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
        assert_eq!(
            ModelSettings::read(dir.path()).unwrap().revision,
            newer.revision
        );
    }
}
