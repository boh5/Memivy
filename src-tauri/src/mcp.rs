use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::Ordering,
    time::Duration,
};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

fn require_main(window: &tauri::WebviewWindow) -> HostResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(HostError::new("main_window_required"))
    }
}
fn executable() -> Option<PathBuf> {
    // The bundled sidecar is adjacent to the app binary. Development uses the
    // same target directory. Never resolve an executable from a caller's PATH.
    let path = std::env::current_exe().ok()?.parent()?.join("memivy-mcp");
    path.is_file().then_some(path)
}
#[derive(Serialize)]
pub(crate) struct McpSettings {
    enabled: bool,
    executable_available: bool,
    configuration: Option<String>,
    setup_prompt: Option<String>,
}

fn configuration(path: &Path, data: Option<&Path>) -> String {
    serde_json::to_string_pretty(&json!({
        "mcpServers": { "memivy": { "command": path, "args": [], "env": {
            "MEMIVY_DATA_DIR": data
        } } }
    }))
    .expect("fixed JSON serializes")
}

fn setup_prompt(configuration: &str) -> String {
    format!(
        r#"Set up Memivy, my personal memory app, as a local MCP server for this agent on my Mac.

Memivy is already installed. Use its bundled stdio server with the configuration below.

1. Find the MCP configuration method and location supported by this agent. Adapt the JSON to its required format, keeping the command, arguments, and environment values unchanged.
2. Back up the existing configuration, then add or update only the "memivy" entry. Preserve all other servers and settings. If an existing Memivy entry uses a different executable or memory library, ask me which to keep before replacing it.
3. Reload or reconnect the MCP server and verify that this agent can access both "memory_capture" and "memory_search". Do not create memories just to test the connection. Only report success after verifying the tools are available. If a restart, permission, or manual step is needed, tell me exactly what to do. If memory access is disabled, ask me to enable it in Memivy under Settings > External access.
4. Tell me what you changed and the verification result, using the language of our conversation. If you cannot configure MCP in this environment, explain what is missing.

Configuration:
```json
{configuration}
```"#
    )
}

#[tauri::command]
pub(crate) fn mcp_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<McpSettings> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let binary = executable();
    let configuration = binary
        .as_deref()
        .map(|path| configuration(path, state.store.database_path().parent()));
    Ok(McpSettings {
        enabled: state.store.mcp_enabled(),
        executable_available: binary.is_some(),
        setup_prompt: configuration.as_deref().map(setup_prompt),
        configuration,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpCopyKind {
    Prompt,
    Configuration,
}

#[tauri::command]
pub(crate) async fn mcp_copy_setup(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    kind: McpCopyKind,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let binary = executable().ok_or(HostError::new("mcp_missing"))?;
    let configuration = configuration(&binary, state.store.database_path().parent());
    let text = match kind {
        McpCopyKind::Prompt if !state.store.mcp_enabled() => {
            return Err(HostError::new("mcp_disabled"));
        }
        McpCopyKind::Prompt => setup_prompt(&configuration),
        McpCopyKind::Configuration => configuration,
    };
    crate::clipboard::write_text(window.app_handle(), text, "mcp_clipboard").await
}

#[tauri::command]
pub(crate) async fn mcp_set_enabled(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    enabled: bool,
) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.set_mcp_enabled(enabled))
        .await
        .map_err(|_| HostError::new("mcp_settings_failed"))?
        .map_err(HostError::from)
}
#[derive(Debug, Serialize)]
pub(crate) struct McpDiagnostic {
    pub(crate) server_version: String,
    pub(crate) protocol_version: String,
    pub(crate) tools: Vec<String>,
    pub(crate) enabled: bool,
    pub(crate) scope: &'static str,
}
async fn response(
    reader: &mut tokio::io::BufReader<tokio::process::ChildStdout>,
    id: i64,
) -> HostResult<Value> {
    for _ in 0..8 {
        let mut line = Vec::new();
        (&mut *reader)
            .take(64 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| HostError::new("mcp_diagnostic_failed"))?;
        if line.is_empty() || line.len() > 64 * 1024 {
            return Err(HostError::new("mcp_invalid_response"));
        }
        let value: Value =
            serde_json::from_slice(&line).map_err(|_| HostError::new("mcp_invalid_response"))?;
        if value["id"] == id {
            if value.get("error").is_some() {
                return Err(HostError::new("mcp_protocol_failed"));
            }
            return Ok(value["result"].clone());
        }
    }
    Err(HostError::new("mcp_invalid_response"))
}
pub(crate) async fn diagnose(
    binary: PathBuf,
    data: PathBuf,
    enabled: bool,
) -> HostResult<McpDiagnostic> {
    let mut child = tokio::process::Command::new(binary)
        .env("MEMIVY_DATA_DIR", data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| HostError::new("mcp_start_failed"))?;
    let checked = tokio::time::timeout(Duration::from_secs(8), async {
        let mut input = child.stdin.take().ok_or(HostError::new("mcp_diagnostic_failed"))?;
        let mut output = tokio::io::BufReader::new(child.stdout.take().ok_or(HostError::new("mcp_diagnostic_failed"))?);
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"memivy-local-check\",\"version\":\"0.1.0\"}}}\n").await.map_err(|_| HostError::new("mcp_diagnostic_failed"))?;
        let info = response(&mut output, 1).await?;
        if info["serverInfo"]["name"] != "memivy" || info["serverInfo"]["version"] != env!("CARGO_PKG_VERSION") {
            return Err(HostError::new("mcp_version_mismatch"));
        }
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n").await.map_err(|_| HostError::new("mcp_diagnostic_failed"))?;
        let list = response(&mut output, 2).await?;
        let mut tools: Vec<String> = list["tools"].as_array().ok_or(HostError::new("mcp_invalid_response"))?.iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string()).collect();
        tools.sort();
        if tools != ["memory_capture", "memory_search"] { return Err(HostError::new("mcp_tools_mismatch")); }
        Ok(McpDiagnostic {
            server_version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: info["protocolVersion"].as_str().ok_or(HostError::new("mcp_invalid_response"))?.into(),
            tools, enabled, scope: "local_stdio_only",
        })
    }).await.map_err(|_| HostError::new("mcp_timeout")).and_then(|r| r);
    let _ = child.kill().await;
    let _ = child.wait().await;
    checked
}
#[tauri::command]
pub(crate) async fn mcp_diagnose(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<McpDiagnostic> {
    let _update_work = crate::updates::work()?;
    require_main(&window)?;
    let binary = executable().ok_or(HostError::new("mcp_missing"))?;
    let root = state
        .store
        .database_path()
        .parent()
        .ok_or(HostError::new("io"))?
        .to_path_buf();
    diagnose(binary, root, state.store.mcp_enabled()).await
}

pub(crate) fn watch_library(app: tauri::AppHandle) {
    let store = app.state::<Workspace>().store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut watcher = None;
        while !app.state::<Workspace>().exiting.load(Ordering::Relaxed) {
            // Independent of the generative model configuration.
            if watcher.is_none() {
                watcher = store.change_watcher().ok();
            }
            if let Some(watch) = watcher.as_mut() {
                match watch.changed() {
                    Ok(true) => {
                        let _ = app.emit("resources-changed", ());
                    }
                    Ok(false) => (),
                    Err(_) => watcher = None,
                }
            }
            std::thread::sleep(Duration::from_millis(750));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_prompt_embeds_the_exact_library_configuration() {
        let executable = "/Users/test/Applications/Memory 测试.app/Contents/MacOS/memivy-mcp";
        let library = "/private/tmp/memivy test/记忆库";
        let config = configuration(Path::new(executable), Some(Path::new(library)));
        let prompt = setup_prompt(&config);
        let embedded = prompt
            .split_once("```json\n")
            .and_then(|(_, suffix)| suffix.strip_suffix("\n```"))
            .expect("prompt includes a complete JSON code block");
        assert_eq!(embedded, config);
        let json: Value = serde_json::from_str(embedded).expect("valid MCP configuration");
        assert_eq!(
            json,
            json!({
                "mcpServers": { "memivy": { "command": executable, "args": [], "env": {
                    "MEMIVY_DATA_DIR": library
                } } }
            })
        );
        assert!(prompt.contains("memory_capture"));
        assert!(prompt.contains("memory_search"));
        assert!(prompt.contains("Preserve all other servers and settings"));
        assert!(prompt.contains("Only report success after verifying"));
    }
}
