//! Keys read while a run is active: interrupt, steer and queue.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn maybe_interrupt(
    input: &mut impl WireWrite,
    display: &mut impl ScrollDisplay,
    events: Option<&Receiver<u8>>,
    kind: &str,
    id: &str,
    accepted: bool,
    settled: bool,
    interrupted: &mut bool,
    clear_id: &mut Option<String>,
    abort_id: &mut Option<String>,
    pending_queued: &mut HashMap<String, DraftSubmission>,
    next_queue_id: &mut u64,
    queued_count: usize,
) -> Result<()> {
    let stop = if kind == "extension_command" && !*interrupted {
        match events {
            Some(rx) if display.live_input() => {
                let mut stop = false;
                for _ in 0..8 {
                    let Some(byte) = display.pending_key().or_else(|| rx.try_recv().ok()) else {
                        break;
                    };
                    match display.active_key(byte, rx)? {
                        ActiveAction::Stop => {
                            stop = true;
                            break;
                        }
                        ActiveAction::Submit(draft) => display.hold_queue(draft)?,
                        ActiveAction::None => {}
                    }
                }
                stop
            }
            Some(rx) => active_keys(rx, display)?,
            None => false,
        }
    } else if kind == "prompt" && !settled && !*interrupted {
        match events {
            Some(rx) if display.live_input() => poll_live_input(
                input,
                display,
                rx,
                id,
                accepted,
                queued_count,
                pending_queued,
                next_queue_id,
            )?,
            Some(rx) => active_keys(rx, display)?,
            None => false,
        }
    } else {
        false
    };
    if stop {
        *interrupted = true;
        *clear_id = Some(format!("{id}-clear"));
        *abort_id = Some(format!("{id}-abort"));
        writeln!(input, "{}", json!({"id":clear_id,"type":"clear_queue"}))
            .map_err(error::transport)?;
        writeln!(input, "{}", json!({"id":abort_id,"type":"abort"})).map_err(error::transport)?;
        input.flush().map_err(error::transport)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Runtime stdin, terminal events and correlated queue state are independent.
pub(super) fn poll_live_input(
    input: &mut impl WireWrite,
    display: &mut impl ScrollDisplay,
    events: &Receiver<u8>,
    id: &str,
    accepted: bool,
    queued_count: usize,
    pending: &mut HashMap<String, DraftSubmission>,
    sequence: &mut u64,
) -> Result<bool> {
    // Bound input work so a fast paste cannot starve Runtime stdout or the spinner.
    // Each printable action may consume up to 128 already queued bytes.
    for _ in 0..8 {
        let byte = match display.pending_key().or_else(|| events.try_recv().ok()) {
            Some(byte) => byte,
            None => break,
        };
        match display.active_key(byte, events)? {
            ActiveAction::Stop => return Ok(true),
            ActiveAction::None => {}
            ActiveAction::Submit(submission) => {
                if !accepted {
                    display.hold_queue(submission)?;
                    continue;
                }
                if pending.len() + queued_count >= 16 {
                    display.rejected_queue(submission, "Too many pending queue requests")?;
                    continue;
                }
                *sequence += 1;
                let queue_id = format!("{id}-queued-{sequence}");
                let behavior = match submission.mode {
                    QueueMode::Steer => "steer",
                    QueueMode::FollowUp => "followUp",
                };
                let command = json!({"id":queue_id,"type":"prompt","message":&submission.text,"streamingBehavior":behavior});
                if let Err(error) = write_command(
                    &mut input.checked(|| service_sending(display, Some(events), true)),
                    &command,
                    &submission.images,
                ) {
                    display.rejected_queue(
                        submission,
                        "Delivery uncertain: sending failed; check history before resending",
                    )?;
                    return Err(error);
                }
                pending.insert(queue_id, submission);
            }
        }
    }
    Ok(false)
}

pub(super) fn active_keys(events: &Receiver<u8>, display: &mut impl ScrollDisplay) -> Result<bool> {
    while let Ok(key) = events.try_recv() {
        if display.paste_byte(key)? {
            continue;
        }
        if key != 27 {
            continue;
        }
        match escape_key(events) {
            Navigation::Escape | Navigation::EscapeWith(_) => return Ok(true),
            Navigation::PasteStart => display.paste_start(),
            Navigation::Mouse(event) => display.mouse(event)?,
            nav => display.scroll(nav)?,
        }
    }
    Ok(false)
}
