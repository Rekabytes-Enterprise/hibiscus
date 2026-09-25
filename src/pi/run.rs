//! Session-level outcome, not a sticky record of whether any attempt failed.
use super::error::{ChatError, ErrorSource};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct RunState {
    assistant_error: Option<ChatError>,
    compaction_error: Option<ChatError>,
}

pub(crate) enum RunOutcome {
    Completed,
    Cancelled,
    Failed(ChatError),
}

impl RunState {
    pub(crate) fn observe(&mut self, event: &Value) {
        match event["type"].as_str() {
            Some("message_update") if event["assistantMessageEvent"]["type"] == "error" => {
                self.assistant_error = Some(ChatError::new(
                    ErrorSource::Assistant,
                    event["assistantMessageEvent"]["error"]["errorMessage"]
                        .as_str()
                        .unwrap_or("Provider stream failed"),
                ));
            }
            Some("message_end") if event["message"]["role"] == "assistant" => {
                let message = &event["message"];
                self.assistant_error = match message["stopReason"].as_str() {
                    Some("error" | "aborted") => Some(ChatError::new(
                        ErrorSource::Assistant,
                        message["errorMessage"]
                            .as_str()
                            .unwrap_or("Pi response failed or was aborted"),
                    )),
                    _ => {
                        self.compaction_error = None;
                        None
                    }
                };
            }
            Some("auto_retry_end") => {
                if event["success"] == true {
                    self.assistant_error = None;
                } else if event["success"] == false {
                    self.assistant_error = Some(ChatError::new(
                        ErrorSource::Assistant,
                        event["finalError"]
                            .as_str()
                            .unwrap_or("Pi retries ended without success"),
                    ));
                }
            }
            Some("compaction_end")
                if event["reason"] == "overflow" || event.get("reason").is_none() =>
            {
                if let Some(message) = event["errorMessage"].as_str() {
                    self.compaction_error = Some(ChatError::new(ErrorSource::Compaction, message));
                } else if event["aborted"] == true {
                    self.compaction_error = Some(ChatError::new(
                        ErrorSource::Compaction,
                        "Context compaction cancelled",
                    ));
                } else if event.get("result").is_some_and(|value| !value.is_null()) {
                    self.compaction_error = None;
                    // Keep an overflow error pending until Pi's next attempt succeeds.
                }
            }
            _ => {}
        }
    }
    pub(crate) fn finish(self, interrupted: bool) -> RunOutcome {
        if interrupted {
            RunOutcome::Cancelled
        } else if let Some(error) = self.compaction_error.or(self.assistant_error) {
            RunOutcome::Failed(error)
        } else {
            RunOutcome::Completed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn retry_success_replaces_failed_attempt_but_progress_does_not() {
        let failure = json!({"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"429 rate limit"}});
        let mut run = RunState::default();
        run.observe(&failure);
        run.observe(&json!({"type":"auto_retry_start"}));
        assert!(matches!(run.finish(false), RunOutcome::Failed(_)));
        let mut run = RunState::default();
        run.observe(&failure);
        run.observe(
            &json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop"}}),
        );
        run.observe(&json!({"type":"auto_retry_end","success":true}));
        assert!(matches!(run.finish(false), RunOutcome::Completed));
    }
    #[test]
    fn compaction_failures_need_authoritative_recovery_and_esc_cancels() {
        let failure = json!({"type":"compaction_end","errorMessage":"summary failed"});
        let mut run = RunState::default();
        run.observe(&failure);
        run.observe(&json!({"type":"summarization_retry_finished"}));
        assert!(matches!(run.finish(false), RunOutcome::Failed(_)));
        let mut run = RunState::default();
        run.observe(&failure);
        run.observe(&json!({"type":"compaction_end","result":{},"willRetry":true}));
        assert!(matches!(run.finish(false), RunOutcome::Completed));
        let mut run = RunState::default();
        run.observe(&failure);
        assert!(matches!(run.finish(true), RunOutcome::Cancelled));
    }
}
