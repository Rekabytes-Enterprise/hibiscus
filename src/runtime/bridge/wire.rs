//! JSONL records to and from the in-process engine.
use super::*;

pub(super) fn receive_record(
    output: &impl Events,
    deadline: Instant,
    display: &mut impl ScrollDisplay,
) -> Result<Option<Value>> {
    loop {
        if Instant::now() >= deadline {
            return Err(error::transport(
                "Runtime command response timed out; connection state is uncertain",
            )
            .into());
        }
        // Startup/catalog requests can be quiet while stderr is active. Paint
        // queued diagnostics without resetting the command's original deadline.
        display.tick_work()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match output.recv_timeout(remaining.min(std::time::Duration::from_millis(40))) {
            Ok(event) => return event.map(Some),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
pub(super) fn read_record<R: BufRead>(output: &mut R) -> Result<Option<Value>> {
    crate::runtime::transport::read_frame(output, 64 * 1024 * 1024)
        .map_err(error::transport)?
        .map(|frame| {
            serde_json::from_slice(&frame)
                .map_err(|_| error::protocol("Invalid Hibiscus runtime JSON record").into())
        })
        .transpose()
}

pub(super) fn service_sending(
    display: &mut impl ScrollDisplay,
    events: Option<&Receiver<u8>>,
    prompt: bool,
) -> io::Result<()> {
    if !prompt {
        return Ok(());
    }
    display.tick_work()?;
    let Some(events) = events else {
        return Ok(());
    };
    let stopped = if display.live_input() {
        let mut stopped = false;
        for _ in 0..8 {
            let Some(byte) = display.pending_key().or_else(|| events.try_recv().ok()) else {
                break;
            };
            match display.active_key(byte, events).map_err(io::Error::other)? {
                ActiveAction::Stop => {
                    stopped = true;
                    break;
                }
                ActiveAction::Submit(submission) => display.hold_queue(submission)?,
                ActiveAction::None => {}
            }
        }
        stopped
    } else {
        active_keys(events, display).map_err(io::Error::other)?
    };
    if stopped {
        Err(io::Error::other("Stopped while sending RPC command; delivery is uncertain. Reconnect and review before resending"))
    } else {
        Ok(())
    }
}

/// Serialize borrowed attachments rather than cloning base64 into another
/// Value. Buffer small JSON fragments, then flush exactly one LF-framed command.
pub(super) fn write_command(
    input: &mut impl Write,
    command: &Value,
    images: &[SharedImage],
) -> Result<()> {
    struct Envelope<'a> {
        command: &'a Value,
        images: &'a [SharedImage],
    }
    impl serde::Serialize for Envelope<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            use serde::ser::{Error, SerializeMap};
            let fields = self
                .command
                .as_object()
                .ok_or_else(|| S::Error::custom("command must be an object"))?;
            let mut map = serializer.serialize_map(None)?;
            for (key, value) in fields {
                map.serialize_entry(key, value)?;
            }
            if !self.images.is_empty() {
                map.serialize_entry("images", self.images)?;
            }
            map.end()
        }
    }
    let mut writer = std::io::BufWriter::new(input);
    let result = (|| {
        serde_json::to_writer(&mut writer, &Envelope { command, images })
            .map_err(error::transport)?;
        writer.write_all(b"\n").map_err(error::transport)?;
        writer.flush().map_err(error::transport)?;
        Ok(())
    })();
    // BufWriter's Drop otherwise retries a failed flush. Once command delivery
    // is uncertain, only the caller's explicit recovery may send more bytes.
    let _ = writer.into_parts();
    result
}

pub(super) fn check_record(event: &Value) -> Result<()> {
    let kind = event["type"]
        .as_str()
        .ok_or_else(|| error::protocol("Runtime record has no event type"))?;
    if kind == "response" && !event["success"].is_boolean() {
        return Err(error::protocol("Runtime response has no success flag").into());
    }
    if kind == "response" && event["command"] == "parse" && event["success"] == false {
        return Err(error::protocol("Runtime rejected RPC framing").into());
    }
    if kind == "message_end"
        && event["message"]["role"] == "assistant"
        && !event["message"]["stopReason"].is_string()
    {
        return Err(error::protocol("Assistant message missing stopReason").into());
    }
    Ok(())
}
