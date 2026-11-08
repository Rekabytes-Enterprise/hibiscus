//! How tool calls, diffs and reasoning appear in the transcript.
use super::*;

pub(super) fn show_reasoning<W: ScrollDisplay>(
    ui: &Ui,
    display: &mut W,
    since: &mut Option<Instant>,
) -> Result<()> {
    if let Some(start) = since.take() {
        if ui.interactive {
            let seconds = start.elapsed().as_secs();
            let duration = if seconds == 0 {
                "<1s".into()
            } else {
                format!("{seconds}s")
            };
            if !display.thinking_end()? {
                ui.info(display, &format!("reasoning · {duration}"))?;
            }
            display.flush()?;
        }
    }
    Ok(())
}

/// Show native post-edit or checkpoint inspection data supplied by the runtime.
/// Never synthesize a preview from requested edits or display one on failure.
pub(super) fn show_tool_diff<W: ScrollDisplay>(
    ui: &Ui,
    display: &mut W,
    label: &str,
    result: &Value,
) -> Result<()> {
    let Some(diff) = result["details"]["diff"]
        .as_str()
        .filter(|diff| !diff.is_empty())
    else {
        return Ok(());
    };
    let path = label.strip_prefix("edit · ").unwrap_or(label);
    ui.info(display, &format!("diff · {path}"))?;
    const MAX_LINES: usize = 40;
    let mut remaining = 0;
    for (index, line) in diff.lines().enumerate() {
        if index < MAX_LINES {
            display.diff_line(&safe_diff_line(line))?;
        } else {
            remaining += 1;
        }
    }
    if remaining > 0 {
        ui.info(display, &format!("… {remaining} more diff lines"))?;
    }
    Ok(())
}

pub(super) fn safe_diff_line(line: &str) -> String {
    line.chars()
        .filter(|ch| !ch.is_control())
        .take(180)
        .collect()
}

pub(super) fn safe_detail(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(70)
        .collect()
}

pub(super) fn tool_label(name: &str, args: &Value) -> String {
    let name = safe_detail(name);
    if matches!(name.as_str(), "read" | "write" | "edit") {
        if let Some(path) = args["path"].as_str().or_else(|| args["filePath"].as_str()) {
            return format!("{name} · {}", safe_detail(path));
        }
    }
    name
}

pub(super) fn tool_call(name: &str, args: &Value) -> tool::Call {
    let target = match name {
        "bash" => args["command"].as_str(),
        "read" | "edit" | "write" => args["path"].as_str().or_else(|| args["filePath"].as_str()),
        _ => None,
    }
    .unwrap_or("");
    tool::Call {
        name: safe_detail(name),
        target: if name == "bash" {
            target.to_owned()
        } else {
            target
                .chars()
                .map(|c| {
                    if matches!(c, '\n' | '\r' | '\t') {
                        ' '
                    } else {
                        c
                    }
                })
                .filter(|c| !c.is_control())
                .take(400)
                .collect()
        },
    }
}
