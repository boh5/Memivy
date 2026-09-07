use crate::workspace::{HostResult, Workspace};
use serde::Serialize;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, sync::atomic::Ordering, time::Duration};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

fn require_main(window: &tauri::WebviewWindow) -> HostResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("仅主窗口可以配置 MCP".into())
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
}
#[tauri::command]
pub(crate) fn mcp_settings(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<McpSettings> {
    require_main(&window)?;
    let binary = executable();
    let configuration = binary.as_ref().map(|path| {
        serde_json::to_string_pretty(&json!({
            "mcpServers": { "memivy": { "command": path, "args": [], "env": {
                "MEMIVY_DATA_DIR": state.store.database_path().parent()
            } } }
        }))
        .expect("fixed JSON serializes")
    });
    Ok(McpSettings {
        enabled: state.store.mcp_enabled(),
        executable_available: binary.is_some(),
        configuration,
    })
}
#[tauri::command]
pub(crate) async fn mcp_set_enabled(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    enabled: bool,
) -> HostResult<()> {
    require_main(&window)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.set_mcp_enabled(enabled))
        .await
        .map_err(|_| "MCP 设置任务未完成".to_string())?
        .map_err(|e| e.to_string())
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
            .map_err(|_| "MCP 诊断读取失败")?;
        if line.is_empty() || line.len() > 64 * 1024 {
            return Err("MCP 诊断响应无效".into());
        }
        let value: Value = serde_json::from_slice(&line).map_err(|_| "MCP 诊断响应无效")?;
        if value["id"] == id {
            if value.get("error").is_some() {
                return Err("MCP 协议检查未通过".into());
            }
            return Ok(value["result"].clone());
        }
    }
    Err("MCP 诊断响应无效".into())
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
        .map_err(|_| "MCP 程序无法启动，请检查安装位置")?;
    let checked = tokio::time::timeout(Duration::from_secs(8), async {
        let mut input = child.stdin.take().ok_or("MCP 诊断输入不可用")?;
        let mut output = tokio::io::BufReader::new(child.stdout.take().ok_or("MCP 诊断输出不可用")?);
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"memivy-local-check\",\"version\":\"0.1.0\"}}}\n").await.map_err(|_| "MCP 诊断发送失败")?;
        let info = response(&mut output, 1).await?;
        if info["serverInfo"]["name"] != "memivy" || info["serverInfo"]["version"] != env!("CARGO_PKG_VERSION") {
            return Err("MCP 程序版本与应用不一致，请重新安装同一版本".into());
        }
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n").await.map_err(|_| "MCP 诊断发送失败")?;
        let list = response(&mut output, 2).await?;
        let mut tools: Vec<String> = list["tools"].as_array().ok_or("MCP 工具清单无效")?.iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string()).collect();
        tools.sort();
        if tools != ["memory_capture", "memory_search"] { return Err("MCP 工具清单与应用不一致".into()); }
        Ok(McpDiagnostic {
            server_version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: info["protocolVersion"].as_str().ok_or("MCP 协议版本无效")?.into(),
            tools, enabled, scope: "local_stdio_only",
        })
    }).await.map_err(|_| "MCP 本地检查超时".to_string()).and_then(|r| r);
    let _ = child.kill().await;
    let _ = child.wait().await;
    checked
}
#[tauri::command]
pub(crate) async fn mcp_diagnose(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<McpDiagnostic> {
    require_main(&window)?;
    let binary =
        executable().ok_or("未找到同包 MCP 程序，请重新安装完整应用；开发环境先构建 memivy-mcp")?;
    let root = state
        .store
        .database_path()
        .parent()
        .ok_or("数据位置无效")?
        .to_path_buf();
    diagnose(binary, root, state.store.mcp_enabled()).await
}

pub(crate) fn watch_library(app: tauri::AppHandle) {
    let store = app.state::<Workspace>().store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut watcher = None;
        while !app.state::<Workspace>().exiting.load(Ordering::Relaxed) {
            // Independent of model configuration and the organizer's job loop.
            if watcher.is_none() {
                watcher = store.change_watcher().ok();
            }
            if let Some(watch) = watcher.as_mut() {
                match watch.changed() {
                    Ok(true) => {
                        let _ = app.emit("library-refresh", ());
                    }
                    Ok(false) => (),
                    Err(_) => watcher = None,
                }
            }
            std::thread::sleep(Duration::from_millis(750));
        }
    });
}
