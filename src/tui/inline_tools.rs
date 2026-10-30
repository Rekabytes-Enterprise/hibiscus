//! A single rewritable activity row in ordinary terminal scrollback.
use super::{inline, tool};
use std::fmt::Write;

const SUCCESS: &str = "\x1b[48;2;24;57;37m\x1b[38;2;164;232;181m";
const FAILURE: &str = "\x1b[48;2;72;28;34m\x1b[38;2;255;171;177m";
const MUTED: &str = "\x1b[38;2;155;155;155m";

#[derive(Default)]
pub(super) struct Tools {
    open: bool,
    reads: Vec<String>,
    read_count: usize,
    active: Option<String>,
}
impl Tools {
    pub(super) fn start(
        &mut self,
        id: &str,
        call: &tool::Call,
        color: bool,
        width: usize,
    ) -> String {
        let mut out = String::new();
        if call.name != "read" || self.active.is_some() {
            out.push_str(&self.flush(color, width));
        }
        self.active = Some(id.to_owned());
        if color {
            self.clear(&mut out);
            let label = if long_command(call, width) {
                "Run".into()
            } else {
                label(call)
            };
            let row = clip(&format!("  … {label}"), width.saturating_sub(1));
            out.push_str(&format!("\x1b[38;2;155;155;155m{row}{}", inline::BASE));
            self.open = true;
        }
        out
    }

    pub(super) fn end(
        &mut self,
        id: &str,
        call: &tool::Call,
        result: &tool::Outcome<'_>,
        color: bool,
        width: usize,
    ) -> String {
        let mut out = String::new();
        // Do not overwrite a different activity if the adapter ever interleaves IDs.
        if self.active.as_deref().is_some_and(|active| active != id) {
            out.push_str(&self.flush(color, width));
        }
        self.clear(&mut out);
        self.active = None;
        if call.name == "read" && !result.failed {
            self.read_count += 1;
            if self.reads.len() < 6 {
                self.reads.push(call.target.clone());
            }
            if color {
                out.push_str(&status(&self.read_label(), "", false, color, width));
                self.open = true;
            }
            return out;
        }
        out.push_str(&self.flush(color, width));
        let wrapped = long_command(call, width);
        out.push_str(&status(
            &if wrapped { "Run".into() } else { label(call) },
            &metadata(call, result),
            result.failed,
            color,
            width,
        ));
        out.push('\n');
        if wrapped {
            out.push_str(&command_block(&call.target, color, width, Some(8)));
        }
        if let Some(diff) = result.diff {
            out.push_str(&edit_preview(diff, color, width));
        }
        if call.name == "bash" || result.failed {
            out.push_str(&output(result.output, color, width));
        }
        out
    }

    pub(super) fn flush(&mut self, color: bool, width: usize) -> String {
        let mut out = String::new();
        if self.read_count > 0 {
            self.clear(&mut out);
            out.push_str(&status(&self.read_label(), "", false, color, width));
            out.push('\n');
            self.reads.clear();
            self.read_count = 0;
        } else if self.open {
            // A run stopped before a matching completion. Never invent success.
            out.push('\n');
            self.open = false;
        }
        self.active = None;
        out
    }

    fn clear(&mut self, out: &mut String) {
        if self.open {
            out.push_str("\r\x1b[2K");
            self.open = false;
        }
    }
    fn read_label(&self) -> String {
        let extra = self.read_count.saturating_sub(self.reads.len());
        format!(
            "Read  {}{}",
            self.reads.join(", "),
            if extra > 0 {
                format!(" (+{extra} files)")
            } else {
                String::new()
            }
        )
    }
}

fn label(call: &tool::Call) -> String {
    let name = match call.name.as_str() {
        "read" => "Read",
        "write" => "Write",
        "edit" => "Edit",
        "bash" => "Run",
        name => name,
    };
    format!("{name}  {}", call.target)
}
fn long_command(call: &tool::Call, width: usize) -> bool {
    call.name == "bash"
        && (call.target.contains('\n') || cells(&label(call)) > width.min(100).saturating_sub(24))
}
pub(super) fn waiting(width: usize) -> String {
    clip(
        "  … Waiting for response · Esc to stop",
        width.saturating_sub(1),
    )
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    let width = width.max(2);
    let clean = line.replace('\t', "    ");
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut used = 0;
    for ch in clean.chars().filter(|c| !c.is_control()) {
        let size = char_cells(ch);
        while used + size > width {
            let boundary = row
                .char_indices()
                .filter(|(_, c)| c.is_whitespace())
                .map(|(i, c)| i + c.len_utf8())
                .next_back();
            let tail = boundary.map(|at| row.split_off(at)).unwrap_or_default();
            rows.push(std::mem::take(&mut row));
            row = tail;
            used = cells(&row);
        }
        row.push(ch);
        used += size;
    }
    rows.push(row);
    rows
}
fn command_block(command: &str, color: bool, width: usize, limit: Option<usize>) -> String {
    let rows = command
        .split('\n')
        .flat_map(|line| wrap_line(line, width.saturating_sub(7)))
        .collect::<Vec<_>>();
    let count = limit.unwrap_or(rows.len()).min(rows.len());
    let mut out = String::new();
    for row in &rows[..count] {
        if color {
            out.push_str(MUTED);
        }
        let _ = writeln!(out, "      {row}");
        if color {
            out.push_str(inline::BASE);
        }
    }
    if count < rows.len() {
        let _ = writeln!(
            out,
            "      … {} command lines omitted\n      Saved output: /output list",
            rows.len() - count
        );
    }
    out
}

pub(super) fn saved_list(
    entries: &[crate::runtime::tool_output::Summary],
    color: bool,
    width: usize,
) -> String {
    let mut out = String::from("\nRecent tool results\n\n");
    if entries.is_empty() {
        out.push_str("  No saved tool output in the current session.\n");
    }
    for entry in entries {
        let call = tool::Call {
            name: entry.name.clone(),
            target: entry.target.clone(),
        };
        let meta = entry
            .exit_code
            .map(|code| format!("exit {code}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}",
            status(
                &format!("{}  {}", entry.index, label(&call)),
                &meta,
                entry.failed,
                color,
                width
            )
        );
    }
    if !entries.is_empty() {
        out.push_str("\nUse /output <number> to inspect a saved result.\n");
    }
    out.push('\n');
    out
}
pub(super) fn saved_output(
    entry: Option<&crate::runtime::tool_output::Output>,
    color: bool,
    width: usize,
) -> String {
    let Some(entry) = entry else {
        return "\nNo saved tool output matches this request in the current session.\n\n".into();
    };
    let mut out = format!("\nSaved output #{}\n\n", entry.summary.index);
    let call = tool::Call {
        name: entry.summary.name.clone(),
        target: entry.summary.target.clone(),
    };
    let meta = entry
        .summary
        .exit_code
        .map(|code| format!("exit {code}"))
        .unwrap_or_default();
    let long = long_command(&call, width);
    let _ = writeln!(
        out,
        "{}",
        status(
            &if long { "Run".into() } else { label(&call) },
            &meta,
            entry.summary.failed,
            color,
            width
        )
    );
    if long {
        out.push_str(&command_block(&call.target, color, width, None));
    }
    let mut diff = inline::Diff::default();
    for line in entry.text.split('\n') {
        for row in wrap_line(line, width.saturating_sub(7)) {
            if color {
                out.push_str(MUTED);
            }
            out.push_str("      ");
            let text = format!("{row}\n");
            out.push_str(&if color { diff.push(&text) } else { text });
            if color {
                out.push_str(inline::BASE);
            }
        }
    }
    out.push_str(&diff.finish());
    if let Some(patch) = &entry.diff {
        for line in patch.split('\n') {
            for row in wrap_line(line, width.saturating_sub(7)) {
                let _ = writeln!(
                    out,
                    "      {}",
                    if color { inline::patch_line(&row) } else { row }
                );
            }
        }
    }
    out.push('\n');
    out
}
fn metadata(call: &tool::Call, result: &tool::Outcome<'_>) -> String {
    if let Some((added, removed)) = result.changes {
        return format!("+{added} −{removed}");
    }
    if call.name != "bash" {
        return String::new();
    }
    let status = result
        .exit_code
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| {
            if result.failed {
                "failed".into()
            } else {
                String::new()
            }
        });
    match result.elapsed {
        Some(elapsed) => {
            let duration = if elapsed.as_millis() < 100 {
                "<0.1s".into()
            } else {
                format!("{:.1}s", elapsed.as_secs_f64())
            };
            if status.is_empty() {
                duration
            } else {
                format!("{status} · {duration}")
            }
        }
        None => status,
    }
}

fn status(label: &str, metadata: &str, failed: bool, color: bool, width: usize) -> String {
    let icon = if failed { "✗" } else { "✓" };
    let available = width.min(100).saturating_sub(3);
    let metadata = clip(metadata, available.saturating_sub(8));
    let reserve = if metadata.is_empty() {
        0
    } else {
        cells(&metadata) + 2
    };
    let text = clip(
        &format!(" {icon} {label} "),
        available.saturating_sub(reserve),
    );
    let mut row = if color {
        format!(
            "  {}{text}{}",
            if failed { FAILURE } else { SUCCESS },
            inline::BASE
        )
    } else {
        format!("  {text}")
    };
    if !metadata.is_empty() {
        let gap = available.saturating_sub(cells(&text) + cells(&metadata));
        row.push_str(&" ".repeat(gap));
        if color {
            row.push_str(MUTED);
        }
        row.push_str(&metadata);
        if color {
            row.push_str(inline::BASE);
        }
    }
    row
}

fn char_cells(ch: char) -> usize {
    if ch.is_ascii() || matches!(ch, '✓' | '✗' | '…' | '·' | '−') {
        1
    } else {
        2
    }
}
fn cells(text: &str) -> usize {
    text.chars().map(char_cells).sum()
}

/// A conservative cell bound avoids wrapping the only row we rewrite. It also
/// strips terminal controls from file names, commands and external output.
fn clip(text: &str, width: usize) -> String {
    let clean = text
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    if cells(&clean) <= width {
        return clean;
    }
    if width == 0 {
        return String::new();
    }
    let mut left = width - 1;
    let mut out = String::new();
    for ch in clean.chars() {
        let size = char_cells(ch);
        if size > left {
            break;
        }
        left -= size;
        out.push(ch);
    }
    out.push('…');
    out
}

fn output(text: &str, color: bool, width: usize) -> String {
    // Patch whitespace and lines consisting of dashes can be real changes.
    let compact = !text.lines().any(|line| {
        line.starts_with("diff --git ")
            || line.starts_with("@@ ")
            || matches!(line.trim(), "```diff" | "```patch")
    });
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        if !compact {
            lines.push(line);
            continue;
        }
        let trimmed = line.trim();
        // Decorative rules do not carry an error or command result. Keep all
        // actual text, including diff headers, and at most one internal blank.
        if trimmed.len() >= 12
            && (trimmed.chars().all(|ch| ch == '-') || trimmed.chars().all(|ch| ch == '='))
        {
            continue;
        }
        if trimmed.is_empty() && lines.last().is_none_or(|last| last.trim().is_empty()) {
            continue;
        }
        lines.push(line);
    }
    if compact && lines.last().is_some_and(|last| last.trim().is_empty()) {
        lines.pop();
    }
    let mut out = String::new();
    let mut diff = inline::Diff::default();
    for (i, line) in lines.iter().enumerate() {
        if lines.len() > 12 && i == 4 {
            out.push_str(&diff.finish());
            let _ = writeln!(out, "      … {} output lines omitted", lines.len() - 12);
        }
        if lines.len() > 12 && i >= 4 && i < lines.len() - 8 {
            continue;
        }
        let text = format!("{}\n", clip(line, width.saturating_sub(7)));
        if color {
            out.push_str(MUTED);
        }
        out.push_str("      ");
        out.push_str(&if color { diff.push(&text) } else { text });
        if color {
            out.push_str(inline::BASE);
        }
    }
    out.push_str(&diff.finish());
    if lines.len() > 12
        || lines
            .iter()
            .any(|line| cells(line) > width.saturating_sub(7))
    {
        out.push_str("      Saved output: /output list\n");
    }
    out
}

fn edit_preview(diff: &str, color: bool, width: usize) -> String {
    let mut out = String::new();
    let mut old_line = 1;
    let mut new_line = 1;
    for (i, line) in diff.lines().enumerate() {
        if i == 40 {
            out.push_str("      … remaining diff omitted\n");
            break;
        }
        if line.starts_with("@@ ") {
            let numbers = line.split_whitespace().skip(1).take(2).collect::<Vec<_>>();
            if let [old, new] = numbers.as_slice() {
                old_line = old
                    .trim_start_matches('-')
                    .split(',')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                new_line = new
                    .trim_start_matches('+')
                    .split(',')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
            }
            continue;
        }
        if line.starts_with(['\\', '[']) {
            let _ = writeln!(out, "      {}", clip(line, width.saturating_sub(7)));
            continue;
        }
        let (number, tone) = match line.as_bytes().first() {
            Some(b'-') => {
                let n = old_line;
                old_line += 1;
                (n, inline::REMOVED)
            }
            Some(b'+') => {
                let n = new_line;
                new_line += 1;
                (n, inline::ADDED)
            }
            _ => {
                let n = new_line;
                new_line += 1;
                old_line += 1;
                (n, inline::TEXT)
            }
        };
        let gutter = format!("{number:>4} ");
        let line = clip(line, width.saturating_sub(7 + cells(&gutter)));
        if color {
            let _ = writeln!(out, "      {MUTED}{gutter}{tone}{line}{}", inline::TEXT);
        } else {
            let _ = writeln!(out, "      {gutter}{line}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_commands_wrap_without_losing_text_and_wait_rows_never_wrap() {
        let command = "printf '%s\\n' 世界 ".repeat(40);
        let rows = wrap_line(&command, 40);
        assert_eq!(rows.join(""), command);
        assert_eq!(wrap_line("one two three", 10), ["one two ", "three"]);
        assert!(rows.iter().all(|row| cells(row) <= 40));
        for width in [2, 20, 40, 80] {
            assert!(cells(&waiting(width)) < width);
        }
        let call = call("bash", &command);
        assert!(long_command(&call, 80));
        let mut tools = Tools::default();
        tools.start("long", &call, false, 80);
        let completed = tools.end("long", &call, &outcome(false, "OK"), false, 80);
        assert!(completed.contains("command lines omitted") && completed.contains("/output list"));
    }
    fn call(name: &str, target: &str) -> tool::Call {
        tool::Call {
            name: name.into(),
            target: target.into(),
        }
    }
    fn outcome(failed: bool, output: &str) -> tool::Outcome<'_> {
        tool::Outcome {
            failed,
            output,
            diff: None,
            exit_code: None,
            changes: None,
            elapsed: None,
        }
    }
    #[test]
    fn consecutive_reads_are_one_completed_row_in_plain_output() {
        let mut tools = Tools::default();
        for (id, path) in [("1", "greeting.py"), ("2", "test_greeting.py")] {
            let call = call("read", path);
            assert_eq!(tools.start(id, &call, false, 100), "");
            assert_eq!(
                tools.end(id, &call, &outcome(false, "file contents"), false, 100),
                ""
            );
        }
        assert_eq!(
            tools.flush(false, 100),
            "   ✓ Read  greeting.py, test_greeting.py \n"
        );
        assert_eq!(tools.flush(false, 100), "");
    }
    #[test]
    fn results_restore_dark_background_and_expose_sanitized_failure_output() {
        let mut tools = Tools::default();
        let shell = call("bash", "printf test; exit 1");
        let start = tools.start("1", &shell, true, 80);
        assert!(start.contains("… Run") && !start.contains(SUCCESS));
        let failed = tools.end(
            "1",
            &shell,
            &outcome(true, "Shell exited with exit status: 1\n\x1b[2Jfailure"),
            true,
            80,
        );
        assert!(failed.starts_with("\r\x1b[2K"));
        assert!(failed.contains(FAILURE));
        assert!(failed.lines().next().unwrap().ends_with(inline::BASE));
        assert!(!failed.contains("\n\x1b[2J"));
        assert!(failed.contains("failure"));
        let success = tools.end("2", &shell, &outcome(false, "OK"), true, 80);
        assert!(success.contains(SUCCESS));
        assert!(!success.contains(FAILURE));
    }
    #[test]
    fn interrupted_activity_never_claims_a_success_and_plain_output_has_no_controls() {
        let mut tools = Tools::default();
        tools.start("1", &call("bash", "sleep 10"), true, 40);
        let stopped = tools.flush(true, 40);
        assert_eq!(stopped, "\n");
        let plain = tools.end(
            "2",
            &call("bash", "echo done"),
            &outcome(false, "done"),
            false,
            40,
        );
        assert!(!plain.contains('\x1b'));
        let clipped = clip("你好世界abcdefghijkl", 8);
        assert!(
            clipped
                .chars()
                .map(|c| if c.is_ascii() { 1 } else { 2 })
                .sum::<usize>()
                <= 8
        );
    }
    #[test]
    fn metadata_stays_on_one_row_at_narrow_widths_without_losing_exit_status() {
        let shell = call("bash", &format!("echo {}", "日本語".repeat(40)));
        let mut result = outcome(true, "");
        result.exit_code = Some(3);
        result.elapsed = Some(std::time::Duration::from_millis(237));
        assert_eq!(metadata(&shell, &result), "exit 3 · 0.2s");
        for width in [20, 40, 80, 120] {
            let row = status(
                &label(&shell),
                &metadata(&shell, &result),
                true,
                false,
                width,
            );
            assert!(cells(&row) < width);
            assert!(row.contains("exit 3"));
            assert!(!row.contains(['\n', '\x1b']));
        }
        result.exit_code = None;
        assert!(metadata(&shell, &result).starts_with("failed · "));
    }
    #[test]
    fn output_compaction_keeps_error_text_diff_headers_and_one_internal_blank() {
        let text = "\n============\nFAIL: example\n--------------\n\n\nAssertionError: expected friend\n\n\nFAILED\n\n";
        assert_eq!(output(text, false, 80), "      FAIL: example\n      \n      AssertionError: expected friend\n      \n      FAILED\n");
        let git = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-------------\n+new\n";
        let shown = output(git, true, 80);
        assert!(shown.contains("--- a/f") && shown.contains("+++ b/f"));
        assert!(shown.contains("-------------"));
        assert!(shown.contains(&format!("{}+new", inline::ADDED)));
        let lines = (0..30)
            .map(|i| format!("error context {i}\n"))
            .collect::<String>();
        let shown = output(&lines, false, 80);
        assert!(shown.contains("error context 0\n") && shown.contains("error context 29\n"));
        assert!(shown.contains("18 output lines omitted"));
    }
}
