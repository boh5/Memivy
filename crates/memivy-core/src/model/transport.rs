//! Rig transport limits and preservation of replay-required Chat reasoning.
//! Rig owns ordinary provider encoding, SSE parsing and tool-call assembly.
use super::{AssistantContent, Message, OutputPolicy, ProbeError, Provider};
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use rig_core::http_client::{
    self as http, HttpClientExt, LazyBody, MultipartForm, Request, Response,
};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};

const CHAT_REASONING_KEY: &str = "_memivy_openai_reasoning";
const REASONING_FIELDS: [&str; 3] = ["reasoning", "reasoning_content", "reasoning_details"];

/// Keep replay-required Chat fields opaque; Rig's generic Chat adapter drops
/// structured reasoning. A real call carries this message's private metadata.
pub(crate) fn retain_chat_reasoning(
    content: &mut [AssistantContent],
    value: &Value,
) -> Result<(), ProbeError> {
    let fields = reasoning_fields(value)?;
    // Rig already round-trips the canonical plaintext field. Avoid duplicating
    // ordinary reasoning unless an alias or structured block needs preservation.
    if !fields.contains_key("reasoning") && !fields.contains_key("reasoning_details") {
        return Ok(());
    }
    if let Some(call) = content.iter_mut().find_map(|part| match part {
        AssistantContent::ToolCall(call) => Some(call),
        _ => None,
    }) {
        call.additional_params
            .get_or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or(ProbeError::InvalidResponse)?
            .insert(CHAT_REASONING_KEY.into(), Value::Object(fields));
    }
    Ok(())
}

fn reasoning_fields(value: &Value) -> Result<Map<String, Value>, ProbeError> {
    let mut fields = Map::new();
    for key in REASONING_FIELDS {
        if let Some(value) = value.get(key) {
            if !value.is_null()
                && if key == "reasoning_details" {
                    !value
                        .as_array()
                        .is_some_and(|parts| parts.iter().all(Value::is_object))
                } else {
                    !value.is_string()
                }
            {
                return Err(ProbeError::InvalidResponse);
            }
            fields.insert(key.into(), value.clone());
        }
    }
    Ok(fields)
}

fn merge_reasoning(fields: &mut Map<String, Value>, delta: &Value) -> Result<(), ProbeError> {
    for (key, value) in reasoning_fields(delta)? {
        let target = fields.entry(key.clone()).or_insert(Value::Null);
        if value.is_null() {
            continue;
        }
        if key != "reasoning_details" {
            if target.is_null() {
                *target = Value::String(String::new());
            }
            let Value::String(target) = target else {
                return Err(ProbeError::InvalidResponse);
            };
            target.push_str(value.as_str().ok_or(ProbeError::InvalidResponse)?);
            continue;
        }
        if target.is_null() {
            *target = Value::Array(vec![]);
        }
        let parts = target.as_array_mut().ok_or(ProbeError::InvalidResponse)?;
        for part in value.as_array().ok_or(ProbeError::InvalidResponse)? {
            // OpenRouter may reuse index 0 across logical blocks. Only
            // consecutive text/summary deltas merge; encrypted blobs remain
            // discrete, and a type transition starts a new ordered block.
            // https://github.com/OpenRouterTeam/ai-sdk-provider/pull/520
            let target = match parts.last_mut().filter(|old| {
                matches!(
                    part["type"].as_str(),
                    Some("reasoning.text" | "reasoning.summary")
                ) && old["type"] == part["type"]
                    && !old["id"]
                        .as_str()
                        .zip(part["id"].as_str())
                        .is_some_and(|(a, b)| a != b)
                    && !old["index"]
                        .as_u64()
                        .zip(part["index"].as_u64())
                        .is_some_and(|(a, b)| a != b)
            }) {
                Some(target) => target,
                None => {
                    parts.push(part.clone());
                    continue;
                }
            };
            let target = target.as_object_mut().ok_or(ProbeError::InvalidResponse)?;
            for (key, value) in part.as_object().ok_or(ProbeError::InvalidResponse)? {
                if value.is_null() {
                    continue;
                }
                let old = target.entry(key.clone()).or_insert(Value::Null);
                if old.is_null() {
                    *old = value.clone();
                } else if matches!(key.as_str(), "text" | "summary" | "signature") {
                    let Value::String(old) = old else {
                        return Err(ProbeError::InvalidResponse);
                    };
                    old.push_str(value.as_str().ok_or(ProbeError::InvalidResponse)?);
                } else if old != value {
                    return Err(ProbeError::InvalidResponse);
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct Transport {
    client: reqwest::Client,
    limit: usize,
    raw: Arc<Mutex<Vec<u8>>>,
    fault: Arc<Mutex<Option<ProbeError>>>,
    chat_history: Vec<(usize, String, Value)>,
}
impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}
impl Default for Transport {
    fn default() -> Self {
        Self::new(OutputPolicy::FullText).expect("HTTP transport initialization failed")
    }
}
impl Transport {
    pub fn new(policy: OutputPolicy) -> Result<Self, ProbeError> {
        static CLIENT: std::sync::OnceLock<Result<reqwest::Client, reqwest::Error>> =
            std::sync::OnceLock::new();
        let client = CLIENT
            .get_or_init(|| {
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(OutputPolicy::FullText.timeout())
                    .build()
            })
            .as_ref()
            .map_err(|_| ProbeError::Network)?
            .clone();
        Ok(Self {
            client,
            limit: policy.bytes(),
            raw: Arc::default(),
            fault: Arc::default(),
            chat_history: vec![],
        })
    }
    pub fn with_history(
        mut self,
        provider: Provider,
        messages: &mut [Message],
    ) -> Result<Self, ProbeError> {
        let mut call_message_index = 0;
        for message in messages {
            if let Message::Assistant { content, .. } = message {
                let has_calls = content
                    .iter()
                    .any(|part| matches!(part, AssistantContent::ToolCall(_)));
                let mut retained = false;
                for part in content {
                    if let AssistantContent::ToolCall(call) = part
                        && let Some(params) = call
                            .additional_params
                            .as_mut()
                            .and_then(Value::as_object_mut)
                    {
                        let metadata = params.remove(CHAT_REASONING_KEY);
                        if params.is_empty() {
                            call.additional_params = None;
                        }
                        // Scrub on every provider, including retries after a protocol change.
                        if provider == Provider::OpenaiCompatible
                            && let Some(metadata) = metadata
                        {
                            if retained {
                                return Err(ProbeError::InvalidResponse);
                            }
                            retained = true;
                            self.chat_history.push((
                                call_message_index,
                                call.wire_call_id().to_owned(),
                                Value::Object(reasoning_fields(&metadata)?),
                            ));
                        }
                    }
                }
                call_message_index += usize::from(has_calls);
            }
        }
        Ok(self)
    }
    pub fn error(&self, fallback: ProbeError) -> ProbeError {
        self.fault
            .lock()
            .ok()
            .and_then(|error| error.clone())
            .unwrap_or(fallback)
    }
    fn failure(&self, error: ProbeError) -> http::Error {
        if let Ok(mut fault) = self.fault.lock() {
            *fault = Some(error.clone());
        }
        http::Error::Instance(Box::new(error))
    }
    // Check terminal/refusal markers and collect reasoning Rig would discard,
    // using the existing SSE framing library without interpreting tool fragments.
    pub async fn validate(&self, provider: Provider, streaming: bool) -> Result<Value, ProbeError> {
        if !matches!(
            provider,
            Provider::OpenaiCompatible | Provider::OpenaiResponses
        ) {
            return Ok(Value::Null);
        }
        let mut reasoning = Map::new();
        let bytes = std::mem::take(&mut *self.raw.lock().map_err(|_| ProbeError::InvalidResponse)?);
        if streaming {
            let mut events =
                futures_util::stream::once(async { Ok::<_, std::io::Error>(Bytes::from(bytes)) })
                    .boxed()
                    .eventsource();
            let mut done = false;
            while let Some(event) = events.next().await {
                let event = event.map_err(|_| ProbeError::InvalidResponse)?;
                if event.data == "[DONE]" {
                    done = true;
                    continue;
                }
                if done {
                    return Err(ProbeError::InvalidResponse);
                }
                let value: serde_json::Value =
                    serde_json::from_str(&event.data).map_err(|_| ProbeError::InvalidResponse)?;
                check_refusal(&value, provider)?;
                if provider == Provider::OpenaiCompatible {
                    merge_reasoning(&mut reasoning, &value["choices"][0]["delta"])?;
                }
            }
            if provider == Provider::OpenaiCompatible && !done {
                return Err(ProbeError::InvalidResponse);
            }
        } else {
            let value = serde_json::from_slice(&bytes).map_err(|_| ProbeError::InvalidResponse)?;
            check_refusal(&value, provider)?;
            if provider == Provider::OpenaiCompatible {
                reasoning = reasoning_fields(&value["choices"][0]["message"])?;
            }
        }
        Ok(Value::Object(reasoning))
    }
    async fn open(&self, req: Request<Bytes>) -> http::Result<reqwest::Response> {
        let (mut parts, mut body) = req.into_parts();
        if !self.chat_history.is_empty() {
            let mut value: Value = serde_json::from_slice(&body)
                .map_err(|_| self.failure(ProbeError::InvalidResponse))?;
            let messages = value["messages"]
                .as_array_mut()
                .ok_or_else(|| self.failure(ProbeError::InvalidResponse))?;
            let mut pending = self.chat_history.clone();
            for (message_index, message) in messages
                .iter_mut()
                .filter(|message| {
                    message["role"] == "assistant"
                        && message["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| !calls.is_empty())
                })
                .enumerate()
            {
                let Some(index) = pending
                    .iter()
                    .position(|(ordinal, _, _)| *ordinal == message_index)
                else {
                    continue;
                };
                let (_, id, metadata) = pending.remove(index);
                if !message["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| calls.iter().any(|call| call["id"] == id))
                {
                    return Err(self.failure(ProbeError::InvalidResponse));
                }
                let object = message
                    .as_object_mut()
                    .ok_or_else(|| self.failure(ProbeError::InvalidResponse))?;
                for field in REASONING_FIELDS {
                    object.remove(field);
                }
                object.extend(
                    metadata
                        .as_object()
                        .ok_or_else(|| self.failure(ProbeError::InvalidResponse))?
                        .clone(),
                );
            }
            if !pending.is_empty() {
                return Err(self.failure(ProbeError::InvalidResponse));
            }
            body = Bytes::from(
                serde_json::to_vec(&value)
                    .map_err(|_| self.failure(ProbeError::InvalidResponse))?,
            );
            parts.headers.remove(reqwest::header::CONTENT_LENGTH);
        }
        if body.len() > 1_048_576 {
            return Err(self.failure(ProbeError::TooLarge));
        }
        let response = self
            .client
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| self.failure(ProbeError::Network))?;
        if !response.status().is_success() {
            // Status is sufficient for the application; do not read or expose vendor error bodies.
            return Err(self.failure(ProbeError::Status(response.status().as_u16())));
        }
        if response
            .content_length()
            .is_some_and(|n| n > self.limit as u64)
        {
            return Err(self.failure(ProbeError::TooLarge));
        }
        Ok(response)
    }
}
impl HttpClientExt for Transport {
    fn send<T, U>(
        &self,
        req: Request<T>,
    ) -> impl Future<Output = http::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        let this = self.clone();
        let req = req.map(Into::into);
        async move {
            let mut response = this.open(req).await?;
            let mut result = Response::builder().status(response.status());
            *result.headers_mut().ok_or(http::Error::NoHeaders)? = response.headers().clone();
            let body: LazyBody<U> = Box::pin(async move {
                let mut bytes = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| this.failure(ProbeError::Network))?
                {
                    if bytes.len() + chunk.len() > this.limit {
                        return Err(this.failure(ProbeError::TooLarge));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                *this
                    .raw
                    .lock()
                    .map_err(|_| this.failure(ProbeError::InvalidResponse))? = bytes.clone();
                Ok(U::from(Bytes::from(bytes)))
            });
            Ok(result.body(body)?)
        }
    }
    fn send_multipart<U>(
        &self,
        _: Request<MultipartForm>,
    ) -> impl Future<Output = http::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        let this = self.clone();
        async move { Err(this.failure(ProbeError::InvalidResponse)) }
    }
    async fn send_streaming<T>(&self, req: Request<T>) -> http::Result<http::StreamingResponse>
    where
        T: Into<Bytes> + Send,
    {
        let response = self.open(req.map(Into::into)).await?;
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
            })
        {
            return Err(self.failure(ProbeError::ToolsUnsupported));
        }
        let mut result = Response::builder().status(response.status());
        *result.headers_mut().ok_or(http::Error::NoHeaders)? = response.headers().clone();
        let limit = self.limit;
        let raw = self.raw.clone();
        let mut size = 0;
        let this = self.clone();
        let stream = response.bytes_stream().map(move |chunk| {
            let chunk = chunk.map_err(|_| this.failure(ProbeError::Network))?;
            size += chunk.len();
            if size > limit {
                return Err(this.failure(ProbeError::TooLarge));
            }
            raw.lock()
                .map_err(|_| this.failure(ProbeError::InvalidResponse))?
                .extend_from_slice(&chunk);
            Ok(chunk)
        });
        Ok(result.body(Box::pin(stream) as http::sse::BoxedStream)?)
    }
}

fn check_refusal(value: &serde_json::Value, provider: Provider) -> Result<(), ProbeError> {
    let nonempty = |v: &serde_json::Value| v.as_str().is_some_and(|s| !s.is_empty());
    let refused = match provider {
        Provider::OpenaiCompatible => {
            let choices = value["choices"]
                .as_array()
                .ok_or(ProbeError::InvalidResponse)?;
            if choices.len() > 1 {
                return Err(ProbeError::InvalidResponse);
            }
            choices
                .iter()
                .any(|c| nonempty(&c["delta"]["refusal"]) || nonempty(&c["message"]["refusal"]))
        }
        Provider::OpenaiResponses => {
            let content_refused = |item: &serde_json::Value| {
                item["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|p| p["type"] == "refusal"))
            };
            let output_refused = |v: &serde_json::Value| {
                v["output"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(content_refused))
            };
            value["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("response.refusal."))
                || value["part"]["type"] == "refusal"
                || content_refused(&value["item"])
                || output_refused(value)
                || output_refused(&value["response"])
        }
        _ => false,
    };
    if refused {
        Err(ProbeError::InvalidResponse)
    } else {
        Ok(())
    }
}
