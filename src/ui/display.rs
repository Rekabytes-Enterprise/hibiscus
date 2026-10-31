//! Presentation contracts consumed by the runtime adapter.
use super::input::{MouseEvent, Navigation};
use crate::Result;
use serde_json::Value;
use std::{
    io::{self, Write},
    sync::{mpsc::Receiver, Arc},
};
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PromptAction {
    Text(Option<String>),
    Update(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueueMode {
    Steer,
    FollowUp,
}

/// A staged attachment can be owned by both the composer and the explicit
/// recovery slot without copying its base64 payload. Runtime wire shape is unchanged.
pub(crate) type SharedImage = Arc<Value>;

pub(crate) struct DraftSubmission {
    pub(crate) text: String,
    pub(crate) images: Vec<SharedImage>,
    pub(crate) mode: QueueMode,
}

pub(crate) enum ActiveAction {
    Stop,
    Submit(DraftSubmission),
    None,
}

pub(crate) trait ScrollDisplay: Write {
    fn assistant_start(&mut self) -> io::Result<bool> {
        Ok(false)
    }
    fn assistant_text(&mut self, text: &str) -> io::Result<()> {
        self.write_all(text.as_bytes())
    }
    fn end_assistant_text(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn end_turn(&mut self) -> io::Result<()> {
        self.end_assistant_text()
    }
    fn diff_line(&mut self, line: &str) -> io::Result<()> {
        writeln!(self, "    {line}")
    }

    fn scroll(&mut self, _navigation: Navigation) -> io::Result<()> {
        Ok(())
    }
    fn mouse(&mut self, _event: MouseEvent) -> io::Result<()> {
        Ok(())
    }
    fn paste_start(&mut self) {}
    fn paste_byte(&mut self, _byte: u8) -> io::Result<bool> {
        Ok(false)
    }
    fn start_command(&mut self, label: &str) -> io::Result<()> {
        self.start_work()?;
        self.set_work(label)
    }
    fn start_work(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn set_work(&mut self, _phase: &str) -> io::Result<()> {
        Ok(())
    }
    fn tick_work(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn stop_work(&mut self) -> io::Result<()> {
        self.end_assistant_text()?;
        Ok(())
    }
    fn complete_diagrams(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn compaction_mode(&mut self, _active: bool) -> io::Result<()> {
        Ok(())
    }
    fn thinking_start(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn thinking_end(&mut self) -> io::Result<bool> {
        Ok(false)
    }
    fn tool_start(&mut self, _id: &str, _call: &super::tool::Call) -> io::Result<bool> {
        Ok(false)
    }
    fn tool_end(
        &mut self,
        _id: &str,
        _call: &super::tool::Call,
        _result: &super::tool::Outcome<'_>,
    ) -> io::Result<bool> {
        Ok(false)
    }
    fn goal(&mut self, _details: &Value) -> io::Result<()> {
        Ok(())
    }
    fn split_timeline(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn live_input(&self) -> bool {
        false
    }
    fn pending_key(&mut self) -> Option<u8> {
        None
    }
    fn active_key(&mut self, _byte: u8, _events: &Receiver<u8>) -> Result<ActiveAction> {
        Ok(ActiveAction::None)
    }
    fn queued(&mut self, _submission: &DraftSubmission) -> io::Result<()> {
        Ok(())
    }
    fn expect_initial_user(&mut self, _message: &str) {}
    fn delivered_user(&mut self, _message: &Value) -> io::Result<bool> {
        Ok(false)
    }
    fn hold_queue(&mut self, _submission: DraftSubmission) -> io::Result<()> {
        Ok(())
    }
    fn rejected_queue(&mut self, _submission: DraftSubmission, _error: &str) -> io::Result<()> {
        Ok(())
    }
    fn queue_update(&mut self, _update: &Value) -> io::Result<()> {
        Ok(())
    }
    fn clear_queue_display(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn extension_dialog(
        &mut self,
        _event: &Value,
        _events: &Receiver<u8>,
    ) -> Result<Option<Value>> {
        Ok(None)
    }
}
impl ScrollDisplay for io::Stdout {}
impl ScrollDisplay for Vec<u8> {}
