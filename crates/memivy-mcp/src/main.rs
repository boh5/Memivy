use memivy_core::{CaptureInput, DataPaths, Store};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CaptureArgs {
    /// Stable UUID for this explicitly requested save. Reuse it when retrying.
    request_id: String,
    /// The user's exact original words, never a summary or inferred preference.
    text: String,
    /// Name of the calling agent application, for provenance.
    source_app: String,
    /// Optional project explicitly supplied by the user.
    project: Option<String>,
    /// Optional conversation URI explicitly supplied by the user.
    session_uri: Option<String>,
}

#[derive(Clone)]
struct Prototype {
    store: Store,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl Prototype {
    #[tool(
        description = "Save exact original text to the isolated Memivy Phase 1 prototype ONLY when the user explicitly asks to save or remember it. Do not infer consent from conversation content. This writes local data. No AI organization occurs; the receipt is pending. Reuse request_id for retries.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn memory_capture(&self, Parameters(args): Parameters<CaptureArgs>) -> CallToolResult {
        let store = self.store.clone();
        let result = tokio::task::spawn_blocking(move || {
            store.mcp_capture(CaptureInput {
                request_id: args.request_id,
                text: args.text,
                source_app: args.source_app,
                project: args.project,
                session_uri: args.session_uri,
            })
        })
        .await;
        match result {
            Ok(Ok(capture)) => CallToolResult::success(vec![ContentBlock::text(serde_json::json!({
                "action":"raw_capture_saved", "capture_id":capture.id, "request_id":capture.request_id,
                "ai_state":"pending", "source_app":capture.source_app, "created_at":capture.created_at,
                "receipt":"原话已保存到阶段一测试库，等待后续整理。"
            }).to_string())]),
            Ok(Err(error)) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
            Err(_) => CallToolResult::error(vec![ContentBlock::text("本地保存任务未完成")]),
        }
    }
}

#[tool_handler(router=self.tool_router, name="memivy-phase1", version="0.1.0", instructions="Technical prototype with isolated data. memory_capture requires an explicit user request to save. Never silently retain conversation content. There is no AI processing and no search tool in this prototype.")]
impl ServerHandler for Prototype {}

#[tokio::main]
async fn main() {
    let result = async {
        let paths = DataPaths::resolve().map_err(|e| e.to_string())?;
        let store = Store::open(paths).map_err(|e| e.to_string())?;
        let server = Prototype {
            store,
            tool_router: Prototype::tool_router(),
        };
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|_| "MCP 握手失败".to_string())?;
        service
            .waiting()
            .await
            .map_err(|_| "MCP 连接结束异常".to_string())?;
        Ok::<_, String>(())
    }
    .await;
    // Stdout belongs exclusively to rmcp's JSON-RPC transport.
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
