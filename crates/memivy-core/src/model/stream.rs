use super::*;
use futures_util::StreamExt;
pub use rig_core::streaming::StreamedAssistantContent as StreamDelta;
use rig_core::{
    client::CompletionClient,
    completion::{CompletionError, CompletionModel, CompletionRequest, FinishReason},
    providers::{anthropic, gemini, openai},
    streaming::StreamingCompletionResponse,
};

fn request(
    config: &ModelConfig,
    messages: Vec<Message>,
    tools: &[ToolDefinition],
) -> CompletionRequest {
    let mut options = json!({});
    let mut max_tokens = config.max_output_tokens.map(u64::from);
    match config.provider {
        Provider::OpenaiCompatible => {
            if !tools.is_empty() {
                options["parallel_tool_calls"] = json!(false);
            }
            if config.disable_reasoning {
                options["reasoning_effort"] = json!("none");
            }
            if matches!(
                config.output_token_parameter,
                OutputTokenParameter::MaxCompletionTokens
            ) && let Some(limit) = max_tokens.take()
            {
                options["max_completion_tokens"] = json!(limit);
            }
        }
        Provider::OpenaiResponses => {
            if !tools.is_empty() {
                options["parallel_tool_calls"] = json!(false);
            }
            if config.disable_reasoning {
                options["reasoning"] = json!({"effort":"none"});
            }
        }
        Provider::Anthropic => {
            // Anthropic requires an explicit cap, including for unknown model IDs.
            max_tokens = Some(max_tokens.unwrap_or(4096));
            if config.disable_reasoning {
                options["thinking"] = json!({"type":"disabled"});
            }
        }
        Provider::Gemini => {
            if config.disable_reasoning {
                options["generationConfig"] = json!({"thinkingConfig":{"thinkingBudget":0}});
            }
        }
    }
    CompletionRequest {
        model: None,
        preamble: None,
        chat_history: messages,
        documents: vec![],
        tools: tools.to_vec(),
        temperature: None,
        max_tokens,
        tool_choice: (!tools.is_empty()).then_some(rig_core::message::ToolChoice::Auto),
        additional_params: Some(options),
        output_schema: None,
        record_telemetry_content: false,
    }
}

// Each arm returns Rig's normalized response; no application message protocol or runner.
macro_rules! dispatch {
    ($config:expr, $transport:expr, $request:expr, $method:ident) => {{
        let config = $config;
        config.endpoint()?;
        let base = config.base_url.trim().trim_end_matches('/');
        let key = config.api_key.as_deref().unwrap_or("");
        match config.provider {
            Provider::OpenaiCompatible => {
                openai::Client::builder()
                    .api_key(key)
                    .base_url(base)
                    .http_client($transport)
                    .build()
                    .map_err(|_| ProbeError::Configuration)?
                    .completions_api()
                    .completion_model(&config.model)
                    .with_strict_tools()
                    .$method($request)
                    .await
            }
            Provider::OpenaiResponses => {
                openai::Client::builder()
                    .api_key(key)
                    .base_url(base)
                    .http_client($transport)
                    .build()
                    .map_err(|_| ProbeError::Configuration)?
                    .completion_model(&config.model)
                    .$method($request)
                    .await
            }
            Provider::Anthropic => {
                anthropic::Client::builder()
                    .api_key(key)
                    .base_url(base)
                    .http_client($transport)
                    .build()
                    .map_err(|_| ProbeError::Configuration)?
                    .completion_model(&config.model)
                    .$method($request)
                    .await
            }
            Provider::Gemini => {
                gemini::Client::builder()
                    .api_key(key)
                    .base_url(base)
                    .http_client($transport)
                    .build()
                    .map_err(|_| ProbeError::Configuration)?
                    .completion_model(&config.model)
                    .$method($request)
                    .await
            }
        }
        .map_err(error)
    }};
}

pub async fn stream_turn(
    config: &ModelConfig,
    messages: &[Message],
    tools: &[ToolDefinition],
    mut delta: impl FnMut(StreamDelta) -> Result<(), ProbeError>,
) -> Result<CompletionResponse, ProbeError> {
    tokio::time::timeout(OutputPolicy::FullText.timeout(), async {
        let mut messages = messages.to_vec();
        let transport = transport::Transport::new(OutputPolicy::FullText)?
            .with_history(config.provider, &mut messages)?;
        let request = request(config, messages, tools);
        let mut stream: StreamingCompletionResponse =
            dispatch!(config, transport.clone(), request, stream)
                .map_err(|e| transport.error(e))?;
        let mut fragments = std::collections::HashMap::<String, Option<String>>::new();
        let mut completed = std::collections::HashSet::new();
        let mut malformed_arguments = false;
        while let Some(part) = stream.next().await {
            let part = part.map_err(|e| transport.error(error(e)))?;
            match &part {
                StreamDelta::ToolCallDelta {
                    internal_call_id,
                    content,
                } => {
                    let text = fragments.entry(internal_call_id.clone()).or_default();
                    if let rig_core::streaming::ToolCallDeltaContent::Delta(fragment) = content {
                        text.get_or_insert_with(String::new).push_str(fragment);
                    }
                }
                StreamDelta::ToolCall {
                    internal_call_id,
                    tool_call,
                } => {
                    // Rig can drop partial calls or coerce evicted malformed input to {}.
                    // Validate the emitted arguments; execution still uses only Rig's assembled call.
                    if let Some(Some(raw)) = fragments.get(internal_call_id) {
                        let args = serde_json::from_str::<serde_json::Value>(raw).ok();
                        malformed_arguments |= args.as_ref() != Some(&tool_call.function.arguments);
                    }
                    completed.insert(internal_call_id.clone());
                }
                _ => {}
            }
            delta(part)?;
        }
        if stream.response.is_none() {
            return Err(ProbeError::InvalidResponse);
        }
        let mut response: CompletionResponse = stream.into();
        validate(&response, tools)?;
        if malformed_arguments || fragments.keys().any(|id| !completed.contains(id)) {
            return Err(ProbeError::InvalidResponse);
        }
        let reasoning = transport.validate(config.provider, true).await?;
        transport::retain_chat_reasoning(&mut response.choice, &reasoning)?;
        Ok(response)
    })
    .await
    .map_err(|_| ProbeError::Network)?
}

pub(super) async fn complete(
    config: &ModelConfig,
    messages: Vec<Message>,
    schema: serde_json::Value,
    policy: OutputPolicy,
) -> Result<CompletionResponse, ProbeError> {
    tokio::time::timeout(policy.timeout(), async {
        let mut messages = messages;
        let transport =
            transport::Transport::new(policy)?.with_history(config.provider, &mut messages)?;
        let mut request = request(config, messages, &[]);
        request.output_schema =
            Some(serde_json::from_value(schema).map_err(|_| ProbeError::InvalidResponse)?);
        let response = dispatch!(config, transport.clone(), request, completion)
            .map_err(|e| transport.error(e))?;
        transport.validate(config.provider, false).await?;
        validate(&response, &[])?;
        Ok(response)
    })
    .await
    .map_err(|_| ProbeError::Network)?
}

fn validate(response: &CompletionResponse, tools: &[ToolDefinition]) -> Result<(), ProbeError> {
    match response.finish_reason() {
        Some(FinishReason::Length) => return Err(ProbeError::Truncated),
        Some(FinishReason::Stop | FinishReason::ToolCalls) => {}
        _ => return Err(ProbeError::InvalidResponse),
    }
    let mut ids = std::collections::HashSet::new();
    for call in calls(response) {
        if call.id.as_str().is_empty()
            || call.id.as_str().len() > 200
            || !ids.insert(call.id.as_str())
            || !call.function.arguments.is_object()
            || !tools.iter().any(|t| t.name == call.function.name)
        {
            return Err(ProbeError::InvalidResponse);
        }
    }
    if calls(response).next().is_none() && text(response).trim().is_empty() {
        return Err(ProbeError::InvalidResponse);
    }
    Ok(())
}
fn error(error: CompletionError) -> ProbeError {
    use rig_core::http_client::Error;
    match error {
        CompletionError::HttpError(
            Error::InvalidStatusCode(status) | Error::InvalidStatusCodeWithMessage(status, _),
        ) => ProbeError::Status(status.as_u16()),
        CompletionError::HttpError(Error::InvalidStatusCodeWithDetails { status, .. }) => {
            ProbeError::Status(status.as_u16())
        }
        CompletionError::HttpError(Error::Instance(error)) => error
            .downcast::<ProbeError>()
            .map(|e| *e)
            .unwrap_or(ProbeError::Network),
        CompletionError::HttpError(_) => ProbeError::Network,
        _ => ProbeError::InvalidResponse,
    }
}
