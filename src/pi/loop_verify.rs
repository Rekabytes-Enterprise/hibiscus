//! Tool-free Pi SDK verdict on the immutable user request. No credentials or
//! private conversation text are persisted by Hibiscus.
use super::auth::sdk_entry;
use crate::{
    tui::screen::{escape_key, Navigation, Screen},
    Result,
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub(crate) enum Verdict {
    Met,
    NotMet(String),
    Missing(String),
    Blocked(String),
    Cancelled,
    Unavailable(&'static str),
}
struct Helper(Child);
impl Drop for Helper {
    fn drop(&mut self) {
        // SAFETY: the dedicated helper was started in its own process group.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate(
    goal: &str,
    steps: &[String],
    evidence: bool,
    evidence_summary: &Value,
    report: &str,
    model: &Value,
    events: &Receiver<u8>,
    output: &mut Screen,
) -> Result<Verdict> {
    let Some(sdk) = sdk_entry() else {
        return Ok(Verdict::Unavailable("Pi SDK could not be found"));
    };
    let (Some(provider), Some(model_id)) = (model["provider"].as_str(), model["id"].as_str())
    else {
        return Ok(Verdict::Unavailable("Pi has no selected model for review"));
    };
    let mut child = Helper(
        Command::new(std::env::var("HIBISCUS_NODE").unwrap_or_else(|_| "node".into()))
            .args([
                "--input-type=module",
                "--eval",
                include_str!("loop-verify.mjs"),
                "--",
            ])
            .arg(sdk)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?,
    );
    let mut writer = child
        .0
        .stdin
        .take()
        .ok_or("Loop reviewer stdin unavailable")?;
    writeln!(
        writer,
        "{}",
        json!({"goal":goal,"steps":steps,"evidence":evidence,"evidenceSummary":evidence_summary,
        "report":report.chars().take(4000).collect::<String>(),"provider":provider,"modelId":model_id})
    )?;
    writer.flush()?;
    drop(writer);
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or("Loop reviewer stdout unavailable")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = BufReader::new(stdout)
            .take(16 * 1024)
            .read_until(b'\n', &mut bytes)
            .ok()
            .filter(|n| *n > 0 && bytes.ends_with(b"\n"))
            .and_then(|_| serde_json::from_slice::<Value>(&bytes).ok());
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(65);
    let mut refreshed = Instant::now();
    loop {
        if Instant::now() >= deadline {
            return Ok(Verdict::Unavailable("Reviewer model call timed out"));
        }
        while let Ok(byte) = events.try_recv() {
            if byte == 27 {
                match escape_key(events) {
                    Navigation::Escape => return Ok(Verdict::Cancelled),
                    Navigation::EscapeWith(next) => {
                        output.defer_input(next);
                        return Ok(Verdict::Cancelled);
                    }
                    _ => {}
                }
            } else {
                output.defer_session_typing([byte]);
            }
        }
        match receiver.recv_timeout(Duration::from_millis(40)) {
            Ok(Some(value)) => {
                let mut reason: String = value["reason"]
                    .as_str()
                    .filter(|reason| !reason.is_empty() && reason.len() <= 500)
                    .unwrap_or("The reviewer did not provide a usable reason")
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(400)
                    .collect();
                let missing = value["missing"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str())
                            .take(50)
                            .map(|item| {
                                item.chars()
                                    .filter(|ch| !ch.is_control())
                                    .take(200)
                                    .collect::<String>()
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if !missing.is_empty() {
                    reason.push_str(" Missing: ");
                    reason.push_str(&missing.join("; "));
                }
                return Ok(match (value["scope"].as_str(), value["verdict"].as_str()) {
                    (Some("missing"), Some("not_met")) => Verdict::Missing(reason),
                    (Some("covered"), Some("met")) if evidence => Verdict::Met,
                    (Some("covered"), Some("met")) => Verdict::NotMet(
                        "Playwright JSON test and attached screenshot evidence were not confirmed"
                            .into(),
                    ),
                    (Some("covered"), Some("not_met")) => Verdict::NotMet(reason),
                    (_, Some("blocked")) => Verdict::Blocked(reason),
                    (_, Some("unavailable")) => {
                        Verdict::Unavailable(match value["code"].as_str() {
                            Some("invalid_input") => "Reviewer received invalid scope input",
                            Some("sdk_load_failed") => "Could not load the selected Pi SDK",
                            Some("sdk_api_unavailable") => {
                                "Installed Pi SDK lacks reviewer APIs; update Pi"
                            }
                            Some("runtime_init_failed") => {
                                "Could not initialize Pi's model runtime"
                            }
                            Some("model_unavailable") => {
                                "Selected Pi model unavailable to reviewer"
                            }
                            Some("model_call_failed") => "Reviewer provider call failed",
                            Some("model_call_aborted") => {
                                "Reviewer request was cancelled or timed out"
                            }
                            Some("model_output_truncated") => {
                                "Reviewer response hit its output limit"
                            }
                            Some("model_response_deferred") => {
                                "Reviewer returned a deferred response"
                            }
                            Some("invalid_response") => {
                                "Pi SDK returned an unsupported reviewer response"
                            }
                            Some("verdict_tool_missing") => {
                                "Reviewer did not call submit_verdict after correction"
                            }
                            Some("verdict_tool_unexpected") => {
                                "Reviewer returned unexpected or multiple verdict calls"
                            }
                            Some("invalid_verdict_arguments") => {
                                "Reviewer returned invalid verdict fields after correction"
                            }
                            _ => "Reviewer reported an unknown failure",
                        })
                    }
                    _ => Verdict::Unavailable("Reviewer returned an unexpected verdict"),
                });
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(Verdict::Unavailable(
                    "Reviewer stopped without a complete JSON reply",
                ))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if refreshed.elapsed() >= Duration::from_secs(1) {
                    output.render()?;
                    refreshed = Instant::now();
                }
            }
        }
    }
}
