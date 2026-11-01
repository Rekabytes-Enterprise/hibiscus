//! Native lifecycle events. Only this adapter encodes the presentation records
//! consumed by the terminal; provider wire records never reach the UI directly.
use super::session::Message;
use serde_json::{json, Value};
pub enum Event<'a> {
    Started,
    Text(&'a str),
    MessageStarted(&'a Message),
    MessageFinished(&'a Message),
    ToolStarted {
        id: &'a str,
        name: &'a str,
        args: &'a Value,
    },
    ToolFinished {
        id: &'a str,
        name: &'a str,
        failed: bool,
        result: &'a Value,
        elapsed: std::time::Duration,
    },
    Failed(&'a str),
    Settled,
}
impl Event<'_> {
    pub fn wire(self) -> Value {
        match self {
            Self::Started => json!({"type":"agent_start"}),
            Self::Text(delta) => {
                json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":delta}})
            }
            Self::MessageStarted(message) => json!({"type":"message_start","message":message}),
            Self::MessageFinished(message) => json!({"type":"message_end","message":message}),
            Self::ToolStarted { id, name, args } => {
                json!({"type":"tool_execution_start","toolCallId":id,"toolName":name,"args":args})
            }
            Self::ToolFinished {
                id,
                name,
                failed,
                result,
                elapsed,
            } => {
                let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
                json!({"type":"tool_execution_end","toolCallId":id,"toolName":name,"isError":failed,"result":result,"durationMs":duration_ms})
            }
            Self::Failed(message) => {
                json!({"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":super::error::safe_message(message)}})
            }
            Self::Settled => json!({"type":"agent_settled"}),
        }
    }
}
