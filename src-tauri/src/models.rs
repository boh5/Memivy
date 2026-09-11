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
}
pub(crate) fn read_llm(state: &Workspace) -> HostResult<ModelConfig> {
    if Registry::exists(state.store.database_path().parent().unwrap()) {
        Registry::read(state.store.database_path().parent().unwrap())?.llm_config()
    } else {
        ModelConfig::read(&state.config).map_err(|e| e.to_string())
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
        return Err("设置已变化，请重新加载后测试".into());
    }
    let candidate = if let Some(edit) = connection {
        if edit.remove {
            return Err("模型连接草稿无效".into());
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
    let message = match kind.as_str() {
        "llm" => {
            let c = memivy_core::model::tools::probe(&m)
                .await
                .map_err(|e| e.to_string())?;
            capabilities = Some(c.clone());
            if c.multi_turn && c.structured_json {
                "连接通过：支持增强问答和整理"
            } else if c.single_tool {
                "连接可用：支持基本工具调用，部分增强能力不可用"
            } else {
                "连接可用：自动整理所需工具调用未通过"
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
            .map_err(|_| "模型测试未完成")??;
            binding.dimensions = Some(dimension);
            format!("连接通过：正文与查询编码可用，{dimension} 维")
        }
        "voice" => {
            tauri::async_runtime::spawn_blocking(move || {
                memivy_core::models::transcribe(&m, &vec![0.; 16000])
            })
            .await
            .map_err(|_| "语音测试未完成")??;
            "音频请求通过。测试使用合成静音；识别效果请实际试说。".into()
        }
        _ => return Err("未知模型能力".into()),
    };
    if load(&state)?.revision != revision {
        return Err("测试期间配置已变化，请重新测试".into());
    }
    let token = uuid::Uuid::new_v4().to_string();
    let mut proofs = tests.0.lock().map_err(|_| "测试状态不可用")?;
    proofs.retain(|p| p.at.elapsed() < Duration::from_secs(600));
    if proofs.len() >= 32 {
        proofs.remove(0);
    }
    proofs.push(Proof {
        token: token.clone(),
        revision,
        kind,
        binding: serde_json::to_string(&binding).map_err(|_| "配置无效")?,
        at: Instant::now(),
        connection: candidate,
        capabilities,
    });
    Ok(TestResult {
        token,
        binding,
        message,
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
        return Err("设置已变化，请重新加载".into());
    }
    let previous = r.clone();
    let mut capabilities = None;
    if let Some(b) = &binding
        && b.source == Source::Service
    {
        let serialized = serde_json::to_string(b).map_err(|_| "配置无效")?;
        let proofs = tests.0.lock().map_err(|_| "测试状态不可用")?;
        let proof = proofs
            .iter()
            .find(|p| {
                Some(&p.token) == token.as_ref()
                    && p.revision == revision
                    && p.kind == kind
                    && p.binding == serialized
                    && p.at.elapsed() < Duration::from_secs(600)
            })
            .ok_or("请先测试当前模型配置")?;
        capabilities = proof.capabilities.clone();
        if let Some(connection) = &proof.connection {
            r.connections.push(connection.clone());
        }
    }
    let root = state.store.database_path().parent().unwrap().to_owned();
    match kind.as_str() {
        "llm" => {
            if binding.as_ref().is_some_and(|b| b.source == Source::Local) {
                return Err("问答请连接本机或远程模型服务".into());
            }
            r.llm = binding;
            prune_unused(&mut r, window.app_handle());
            r.save(&root, &revision)?;
            if let Some(caps) = capabilities {
                let config = r.llm_config()?;
                if let Err(e) = memivy_core::model::tools::save_capabilities(&root, &config, &caps)
                {
                    let error = rollback_config(&root, &r, previous, e.to_string());
                    changed(&window);
                    return Err(error);
                }
            }
        }
        "embedding" => {
            if let Some(b) = binding {
                if !confirmed {
                    return Err("请先确认索引处理范围".into());
                }
                r.embedding = b;
                prune_unused(&mut r, window.app_handle());
                let store = state.store.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    store.apply_embedding_model(&mut r, &revision)?;
                    Ok::<_, String>(r)
                })
                .await
                .map_err(|_| "语义检索设置未完成")?;
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
        _ => return Err("未知模型能力".into()),
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
        return Err("未知模型".into());
    }
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = store.database_path().parent().unwrap().to_owned();
        let _writer = memivy_core::embedding::lock(&root, "embedding-writer.lock")?;
        if Registry::read(&root)?.embedding.source == Source::Local {
            store
                .embedding_control("disable")
                .map_err(|e| e.to_string())?;
        }
        memivy_core::embedding::cache::HfModelCache::for_user()?.clear()
    })
    .await
    .map_err(|_| "清理模型未完成")?
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
    error: String,
) -> String {
    if previous.save(root, &candidate.revision).is_ok() {
        error
    } else {
        format!("{error}；配置恢复未完成，请重新读取设置后检查")
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
                "activation failed".into()
            ),
            "activation failed"
        );
        let mut newer = Registry::read(dir.path()).unwrap();
        assert!(newer.auto_organize);
        assert_ne!(newer.revision, previous.revision);
        let revision = newer.revision.clone();
        newer.auto_organize = false;
        newer.save(dir.path(), &revision).unwrap();
        assert!(
            rollback_config(dir.path(), &candidate, previous, "activation failed".into())
                .contains("恢复未完成")
        );
        assert_eq!(Registry::read(dir.path()).unwrap().revision, newer.revision);
    }
}
