//! Local memory manager shared by the CLI and TUI. Never submits model prompts.
use crate::{
    client::error::local,
    memory::manager::{Manager, Scope},
    ui::{display::ScrollDisplay, terminal::RawMode, Frontend},
    Result,
};
use serde_json::json;
use std::{io::Write, sync::mpsc::Receiver};

pub(super) fn open(
    message: &str,
    output: &mut Frontend,
    events: Option<&Receiver<u8>>,
    raw: &mut Option<RawMode>,
) -> Result<()> {
    let scope = match message
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => None,
        ["global"] => Some(Scope::Global),
        ["project"] => Some(Scope::Project),
        _ => return Err(local("Use /memory, /memory global, or /memory project.").into()),
    };
    let home = crate::paths::home()?;
    let cwd = std::env::current_dir()?.canonicalize()?;
    let manager = Manager::open(&home, &cwd)?;
    let (Some(events), Some(raw)) = (events, raw.as_mut()) else {
        // Piped input is read-only and never consumes later prompts as answers.
        for scope in scope.map_or(vec![Scope::Global, Scope::Project], |s| vec![s]) {
            let text = manager.load(scope)?;
            writeln!(
                output,
                "\nMemory · {}\n{}\n{}",
                scope.label(),
                size(text.as_deref()),
                clean(text.as_deref().unwrap_or("No saved memory."))
            )?;
        }
        return Ok(());
    };
    loop {
        let selected = if let Some(scope) = scope {
            scope
        } else {
            match select(
                output,
                events,
                raw,
                "Memory · local files",
                &["Global".into(), "Current Project".into(), "Close".into()],
            )? {
                Some(0) => Scope::Global,
                Some(1) => Scope::Project,
                _ => return Ok(()),
            }
        };
        manage(&manager, selected, output, events, raw)?;
        if scope.is_some() {
            return Ok(());
        }
    }
}

fn manage(
    manager: &Manager,
    scope: Scope,
    output: &mut Frontend,
    events: &Receiver<u8>,
    raw: &mut RawMode,
) -> Result<()> {
    loop {
        let current = manager.load(scope)?;
        let title = format!("Memory · {}\n{}", scope.label(), size(current.as_deref()));
        let options = ["View", "Edit", "Delete", "Revisions", "Back"].map(str::to_owned);
        match select(output, events, raw, &title, &options)? {
            Some(0) => {
                view(
                    output,
                    events,
                    raw,
                    &format!("Memory · {}", scope.label()),
                    current.as_deref().unwrap_or("No saved memory."),
                    "Back",
                )?;
            }
            Some(1) => {
                let text = current.as_deref().unwrap_or("");
                if text
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
                {
                    return Err(local("Memory contains control characters; use a text editor to clean it before editing here.").into());
                }
                if let Some(next) = output.memory_editor(
                    &format!("Edit memory · {}", scope.label()),
                    text,
                    events,
                    raw,
                )? {
                    if manager.save(scope, current.as_deref(), &next)? {
                        output.memory_notice(&format!("Memory saved · {}", scope.label()))?;
                    }
                }
            }
            Some(2) => {
                if current.is_none() {
                    continue;
                }
                if confirm(output, events, raw, &format!("Delete {} memory?\nThe active file will be removed. Revisions and legacy copies remain; this is not permanent erasure.", scope.label()))? && manager.delete(scope, current.as_deref())? {
                    output.memory_notice(&format!("Memory deleted · {}", scope.label()))?;
                }
            }
            Some(3) => {
                let revisions = manager.revisions(scope)?;
                if revisions.is_empty() {
                    view(
                        output,
                        events,
                        raw,
                        "Memory revisions",
                        "No saved revisions.",
                        "Back",
                    )?;
                    continue;
                }
                let mut labels: Vec<_> = revisions
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        format!(
                            "Revision {}{} · {} bytes",
                            i + 1,
                            if i == 0 { " (newest)" } else { "" },
                            r.bytes
                        )
                    })
                    .collect();
                labels.push("Back".into());
                let Some(index) = select(
                    output,
                    events,
                    raw,
                    "Memory revisions · newest first",
                    &labels,
                )?
                else {
                    continue;
                };
                let Some(revision) = revisions.get(index) else {
                    continue;
                };
                let text = manager.revision(scope, &revision.id)?;
                // Keep the current snapshot from before preview so a concurrent
                // change cannot be silently replaced after confirmation.
                if !view(
                    output,
                    events,
                    raw,
                    "Memory revision preview",
                    &text,
                    "Restore this revision",
                )? {
                    continue;
                }
                if confirm(output, events, raw, &format!("Restore {} memory?\nReplace active memory with the previewed revision. The current version will be backed up.", scope.label()))? && manager.save(scope, current.as_deref(), &text)? {
                    output.memory_notice(&format!("Memory restored · {}", scope.label()))?;
                }
            }
            _ => return Ok(()),
        }
    }
}

fn size(text: Option<&str>) -> String {
    let bytes = text.map_or(0, str::len);
    let status = if bytes > 65536 {
        "Not loaded by Hibi: exceeds 64 KiB. Shorten this file."
    } else if bytes > 8192 {
        "Above recommended 8 KiB; consider shortening."
    } else if text.is_none() {
        "No saved memory."
    } else {
        "Recommended maximum: 8 KiB."
    };
    format!("{bytes} bytes · {status}")
}
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '�'
            } else {
                c
            }
        })
        .collect()
}
fn select(
    output: &mut Frontend,
    events: &Receiver<u8>,
    raw: &mut RawMode,
    title: &str,
    options: &[String],
) -> Result<Option<usize>> {
    if output.is_full() {
        let response = output.extension_dialog(
            &json!({"method":"select","title":title,"options":options}),
            events,
        )?;
        Ok(response.and_then(|r| {
            options
                .iter()
                .position(|s| r["value"].as_str() == Some(s.as_str()))
        }))
    } else {
        writeln!(output, "\n{}", clean(title))?;
        for (index, option) in options.iter().enumerate() {
            writeln!(output, "  {}. {}", index + 1, clean(option))?;
        }
        output.flush()?;
        raw.write("Choose a number (Enter or Esc to cancel): ")?;
        Ok(read_choice(events, raw)?
            .and_then(|s| s.trim().parse::<usize>().ok())
            .and_then(|i| i.checked_sub(1))
            .filter(|i| *i < options.len()))
    }
}
fn confirm(
    output: &mut Frontend,
    events: &Receiver<u8>,
    raw: &mut RawMode,
    title: &str,
) -> Result<bool> {
    Ok(select(
        output,
        events,
        raw,
        title,
        &["Cancel".into(), "Confirm".into()],
    )? == Some(1))
}
fn view(
    output: &mut Frontend,
    events: &Receiver<u8>,
    raw: &mut RawMode,
    title: &str,
    text: &str,
    done: &str,
) -> Result<bool> {
    let text = clean(text);
    let mut pages = vec![];
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let mut end = rest.len().min(8000);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        pages.push(&rest[..end]);
        rest = &rest[end..];
    }
    if pages.is_empty() {
        pages.push("Empty memory.");
    }
    let mut page = 0;
    loop {
        let title = format!(
            "{title} · page {}/{}\n{}",
            page + 1,
            pages.len(),
            pages[page]
        );
        match select(
            output,
            events,
            raw,
            &title,
            &[done.into(), "Next page".into(), "Previous page".into()],
        )? {
            Some(1) => page = (page + 1).min(pages.len() - 1),
            Some(2) => page = page.saturating_sub(1),
            Some(0) => return Ok(true),
            _ => return Ok(false),
        }
    }
}

fn read_choice(events: &Receiver<u8>, raw: &mut RawMode) -> Result<Option<String>> {
    let mut value = String::new();
    while let Ok(byte) = events.recv() {
        match byte {
            b'\r' | b'\n' => {
                raw.write("\r\n")?;
                return Ok(Some(value));
            }
            3 | 4 => return Ok(None),
            27 => {
                if matches!(
                    crate::ui::input::escape_key(events),
                    crate::ui::input::Navigation::Escape
                ) {
                    raw.write("\r\n")?;
                    return Ok(None);
                }
            }
            8 | 127 => {
                if value.pop().is_some() {
                    raw.write("\x08 \x08")?;
                }
            }
            b'0'..=b'9' if value.len() < 4 => {
                value.push(byte as char);
                raw.write(&(byte as char).to_string())?;
            }
            _ => {}
        }
    }
    Ok(None)
}
