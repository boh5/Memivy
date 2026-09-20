//! Execution checkpoints contain Rig messages plus request-local evidence proofs.
use super::{DataError, Result};
use crate::model::{self, AssistantContent, Message, ToolCall, ToolResultContent, UserContent};
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
    values
        .iter()
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| DataError::Invalid))
        .collect()
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
