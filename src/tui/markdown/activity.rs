//! Tool activity lines and Mermaid blocks inside transcripts.
use super::*;

pub(super) fn is_tool_activity(content: &str) -> bool {
    content
        .trim_start()
        .strip_prefix("· ")
        .and_then(|status| status.split_once(" · "))
        .is_some_and(|(badge, _)| {
            badge.starts_with(['✓', '✗', '◌'])
                || matches!(
                    badge,
                    "◐ Exploring"
                        | "◓ Exploring"
                        | "◑ Exploring"
                        | "◒ Exploring"
                        | "Explore stopped"
                )
        })
}

pub(super) fn tool_status(content: &str) -> Option<Vec<Cell>> {
    let status = content.trim_start().strip_prefix("· ")?;
    let (badge, detail) = status.split_once(" · ")?;
    let tone = if badge.starts_with("✓ ") {
        Tone::ToolSuccess
    } else if badge.starts_with("✗ ") {
        Tone::ToolError
    } else {
        return None;
    };
    let mut cells = Vec::new();
    push(&mut cells, "  ", Tone::Muted);
    let badge = badge
        .strip_suffix(" edit")
        .map(|prefix| format!("{prefix} Edit"))
        .or_else(|| {
            badge
                .strip_suffix(" write")
                .map(|prefix| format!("{prefix} Write"))
        })
        .unwrap_or_else(|| badge.to_owned());
    push(&mut cells, &format!(" {badge} "), tone);
    push(&mut cells, &format!("  {detail}"), Tone::Muted);
    Some(cells)
}

pub(super) fn complete_mermaid(
    mut lines: std::iter::Peekable<std::str::Lines<'_>>,
) -> Option<(String, usize)> {
    let mut body = String::new();
    let mut consumed = 0;
    for line in lines.by_ref() {
        consumed += 1;
        if line.trim() == "```" {
            return (!body.trim().is_empty()).then_some((body, consumed));
        }
        if consumed > 100
            || body.len() + line.len() > 8192
            || line.starts_with("hibi › ")
            || line.starts_with("hibiscus › ")
            || line.starts_with("you › ")
        {
            return None;
        }
        body.push_str(line.trim_end_matches('\r'));
        body.push('\n');
    }
    None
}
