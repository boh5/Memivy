//! Execution checkpoints contain Rig messages plus request-local evidence proofs.
//! Legacy OpenAI checkpoints are converted only when reading stored execution state.
use super::{DataError, Result};
use crate::model::{self, AssistantContent, Message, ToolCall, ToolResultContent, UserContent};
use rig_core::message::{Reasoning, ToolFunction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    #[serde(flatten)]
    pub message: Message,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "_memivy_request_reads"
    )]
    pub reads: Option<Value>,
}
impl From<Message> for Checkpoint {
    fn from(message: Message) -> Self {
        Self {
            message,
            reads: None,
        }
    }
}
pub(super) fn encode(messages: &[Checkpoint]) -> Result<Vec<Value>> {
    messages
        .iter()
        .map(|m| serde_json::to_value(m).map_err(|_| DataError::Invalid))
        .collect()
}
pub(super) fn decode(values: &[Value]) -> Result<Vec<Checkpoint>> {
    let mut messages: Vec<Checkpoint> = Vec::with_capacity(values.len());
    for value in values {
        let message = match value["role"].as_str() {
            Some("system") => Message::system(value["content"].as_str().ok_or(DataError::Invalid)?),
            Some("user") if value["content"].is_string() => {
                Message::user(value["content"].as_str().ok_or(DataError::Invalid)?)
            }
            Some("assistant") if !value["content"].is_array() => {
                let mut content = vec![];
                if let Some(text) = value["reasoning_content"]
                    .as_str()
                    .or_else(|| value["reasoning"].as_str())
                    .filter(|s| !s.is_empty())
                {
                    content.push(AssistantContent::Reasoning(Reasoning::new(text)));
                }
                if let Some(text) = value["content"].as_str().filter(|s| !s.is_empty()) {
                    content.push(AssistantContent::text(text));
                }
                if let Some(calls) = value["tool_calls"].as_array() {
                    for call in calls {
                        let id = call["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .ok_or(DataError::Invalid)?;
                        let function = ToolFunction::new(
                            call["function"]["name"]
                                .as_str()
                                .ok_or(DataError::Invalid)?
                                .into(),
                            serde_json::from_str(
                                call["function"]["arguments"]
                                    .as_str()
                                    .ok_or(DataError::Invalid)?,
                            )
                            .map_err(|_| DataError::Invalid)?,
                        );
                        content.push(AssistantContent::ToolCall(ToolCall::from_wire(
                            id, function,
                        )));
                    }
                }
                model::transport::retain_chat_reasoning(&mut content, value)
                    .map_err(|_| DataError::Invalid)?;
                Message::Assistant { id: None, content }
            }
            Some("tool") => {
                let id = value["tool_call_id"].as_str().ok_or(DataError::Invalid)?;
                let call = messages
                    .iter()
                    .rev()
                    .flat_map(|m| calls(&m.message))
                    .find(|c| c.id.as_str() == id)
                    .ok_or(DataError::Invalid)?;
                Message::User {
                    content: vec![UserContent::tool_result_for(
                        call.id.clone(),
                        call.provider.clone(),
                        call.function.name.clone(),
                        vec![ToolResultContent::text(
                            value["content"].as_str().ok_or(DataError::Invalid)?,
                        )],
                    )],
                }
            }
            _ => serde_json::from_value(value.clone()).map_err(|_| DataError::Invalid)?,
        };
        messages.push(Checkpoint {
            message,
            reads: value.get("_memivy_request_reads").cloned(),
        });
    }
    Ok(messages)
}
pub(super) fn calls(message: &Message) -> impl Iterator<Item = &ToolCall> {
    let content = match message {
        Message::Assistant { content, .. } => content.as_slice(),
        _ => &[],
    };
    content.iter().filter_map(|c| match c {
        AssistantContent::ToolCall(call) => Some(call),
        _ => None,
    })
}
pub(super) fn answered(message: &Message, call: &ToolCall) -> bool {
    matches!(message, Message::User { content } if content.iter().any(|c| matches!(c, UserContent::ToolResult(r) if r.call == call.id)))
}
pub(super) fn is_result(message: &Message) -> bool {
    matches!(message, Message::User { content } if content.iter().any(|c| matches!(c, UserContent::ToolResult(_))))
}
/// Only application-authored context and completed tool results supply evidence.
pub(super) fn context_texts(message: &Message) -> Vec<&str> {
    let Message::User { content } = message else {
        return vec![];
    };
    content
        .iter()
        .flat_map(|c| match c {
            UserContent::Text(t) => vec![t.text.as_str()],
            UserContent::ToolResult(result) => result
                .content
                .iter()
                .filter_map(|part| match part {
                    ToolResultContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        })
        .collect()
}
pub(super) fn edit_context(
    message: &mut Message,
    mut edit: impl FnMut(&mut String) -> Result<()>,
) -> Result<()> {
    if let Message::User { content } = message {
        for part in content {
            match part {
                UserContent::Text(t) => edit(&mut t.text)?,
                UserContent::ToolResult(result) => {
                    for part in &mut result.content {
                        if let ToolResultContent::Text(t) = part {
                            edit(&mut t.text)?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
pub(super) fn result(call: &ToolCall, value: &Value) -> Checkpoint {
    model::tool_result(call, value).into()
}
