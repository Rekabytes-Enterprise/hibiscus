mod activity;
mod inline;
mod table;
use activity::*;
use inline::*;
use table::*;

use std::collections::HashMap;

/// Lightweight terminal Markdown for chat transcripts. This is a display
/// transform only: Runtime and saved messages keep the original Markdown text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Strong,
    Emphasis,
    Code,
    Heading,
    Link,
    Muted,
    TableBorder,
    Activity,
    Success,
    Error,
    ToolSuccess,
    ToolError,
    DiffAdded,
    DiffRemoved,
    DiffContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    Assistant,
    Note,
    Diff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cell {
    pub(crate) ch: char,
    pub(crate) tone: Tone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) role: Role,
    pub(crate) cells: Vec<Cell>,
}

/// Cache whole user turns: assistant headings are shared by tools and prose,
/// and fences/table lookahead can change previously received text.
#[derive(Default)]
pub(crate) struct Layout {
    width: usize,
    blocks: Vec<(String, Vec<std::rc::Rc<Row>>)>,
    rows: std::rc::Rc<Vec<std::rc::Rc<Row>>>,
}

impl Layout {
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
    pub(crate) fn rows(&self) -> std::rc::Rc<Vec<std::rc::Rc<Row>>> {
        self.rows.clone()
    }
    pub(crate) fn width(&self) -> usize {
        self.width
    }
    pub(crate) fn invalidate(&mut self) {
        self.blocks.clear();
    }
    // Retained for formatter benchmarks and isolated Markdown tests.
    #[allow(dead_code)]
    pub(crate) fn update(&mut self, text: &str, width: usize) {
        self.update_with_diagrams(text, width, &HashMap::new());
    }
    pub(crate) fn update_with_diagrams(
        &mut self,
        text: &str,
        width: usize,
        diagrams: &HashMap<String, Vec<String>>,
    ) {
        if self.width != width {
            self.blocks.clear();
        }
        self.width = width;
        let mut boundaries = vec![0];
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if offset > 0 && line.starts_with("you › ") {
                boundaries.push(offset);
            }
            offset += line.len();
        }
        boundaries.push(text.len());
        let mut rows = Vec::new();
        for (index, range) in boundaries.windows(2).enumerate() {
            let source = &text[range[0]..range[1]];
            if self.blocks.get(index).is_none_or(|(old, _)| old != source) {
                let block = (
                    source.to_owned(),
                    if diagrams.is_empty() {
                        format(source, width)
                    } else {
                        format_with_diagrams(source, width, diagrams)
                    }
                    .into_iter()
                    .map(std::rc::Rc::new)
                    .collect(),
                );
                if index < self.blocks.len() {
                    self.blocks[index] = block;
                } else {
                    self.blocks.push(block);
                }
            }
            rows.extend(self.blocks[index].1.iter().cloned());
        }
        self.blocks.truncate(boundaries.len() - 1);
        self.rows = std::rc::Rc::new(rows);
    }
}

/// Return screen rows no wider than `width` columns (for ordinary terminal
/// text). Fenced code is deliberately literal; prose receives inline styles.
pub(crate) fn format(text: &str, width: usize) -> Vec<Row> {
    format_with_diagrams(text, width, &HashMap::new())
}

fn format_with_diagrams(
    text: &str,
    width: usize,
    diagrams: &HashMap<String, Vec<String>>,
) -> Vec<Row> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut role = Role::Assistant;
    let mut fence = false;
    let mut in_diff = false;
    let mut in_table = false;
    let mut assistant_labeled = false;
    let mut sources = text.lines().peekable();
    while let Some(source) = sources.next() {
        let content = if let Some(content) = source.strip_prefix("you › ") {
            assistant_labeled = false;
            role = Role::User;
            fence = false;
            in_diff = false;
            in_table = false;
            content
        } else if let Some(content) = source
            .strip_prefix("hibi › ")
            .or_else(|| source.strip_prefix("hibiscus › "))
        {
            role = Role::Assistant;
            fence = false;
            in_diff = false;
            in_table = false;
            // Old saved tool-only messages can contain an empty text block.
            // Do not turn that into an orphan speaker label on replay.
            if content.trim().is_empty()
                && !sources
                    .clone()
                    .take_while(|line| {
                        !line.starts_with("you › ")
                            && !line.starts_with("hibi › ")
                            && !line.starts_with("hibiscus › ")
                            && !line.trim_start().starts_with('·')
                    })
                    .any(|line| !line.trim().is_empty())
            {
                while sources.peek().is_some_and(|line| line.trim().is_empty()) {
                    sources.next();
                }
                continue;
            }
            if !assistant_labeled {
                let mut label = Vec::new();
                push(&mut label, "hibi", Tone::Activity);
                rows.extend(wrap_cells(label, width, role));
                assistant_labeled = true;
            }
            content
        } else if source.trim_start().starts_with('·') {
            // Tool-first turns still need a speaker. Share one heading with
            // subsequent tools and prose, including replayed empty replies.
            if !assistant_labeled && is_tool_activity(source) {
                let mut label = Vec::new();
                push(&mut label, "hibi", Tone::Activity);
                rows.extend(wrap_cells(label, width, Role::Assistant));
                assistant_labeled = true;
            }
            role = Role::Note;
            in_diff = source.trim_start().starts_with("· diff · ");
            in_table = false;
            source
        } else if in_diff && source.starts_with("    ") {
            role = Role::Diff;
            &source[4..]
        } else {
            source
        };
        let content = content.trim_end_matches('\r');
        let trimmed = content.trim_start();
        if role == Role::Assistant && !fence && trimmed.eq_ignore_ascii_case("```mermaid") {
            if let Some((body, consumed)) = complete_mermaid(sources.clone()) {
                if let Some(lines) = diagrams.get(&body).filter(|lines| {
                    !lines.is_empty() && lines.iter().all(|line| line.chars().count() <= width)
                }) {
                    for line in lines {
                        let mut cells = Vec::new();
                        push(&mut cells, line, Tone::Code);
                        rows.push(Row { role, cells });
                    }
                    for _ in 0..consumed {
                        sources.next();
                    }
                    continue;
                }
            }
        }
        // Collect a table as a block so every row uses the same column widths.
        // Also handle consecutive pipe rows when a model omits the separator.
        if role == Role::Assistant && !fence {
            if let Some(header) = table_row_columns(trimmed) {
                let next = sources.peek().map(|line| line.trim()).unwrap_or("");
                if !table_separator(trimmed)
                    && table_row_columns(next).is_some_and(|columns| columns.len() == header.len())
                {
                    let count = header.len();
                    let mut table = vec![header];
                    if table_separator(next) {
                        sources.next();
                    }
                    while sources.peek().is_some_and(|line| {
                        !table_separator(line.trim())
                            && table_row_columns(line.trim())
                                .is_some_and(|columns| columns.len() == count)
                    }) {
                        table.push(table_row_columns(sources.next().unwrap().trim()).unwrap());
                    }
                    rows.extend(render_table(&table, width));
                    in_table = false;
                    continue;
                }
            }
        }
        let mut cells = Vec::new();
        if role == Role::Diff {
            let tone = match content.chars().next() {
                Some('+') => Tone::DiffAdded,
                Some('-') => Tone::DiffRemoved,
                _ => Tone::DiffContext,
            };
            push(&mut cells, content, tone);
        } else if let Some(status) = tool_status(content).filter(|_| role == Role::Note) {
            cells = status;
        } else if role == Role::Assistant && trimmed.starts_with("```") {
            if !fence && trimmed.len() > 3 {
                push(
                    &mut cells,
                    &format!("  {}", trimmed[3..].trim()),
                    Tone::Muted,
                );
            }
            fence = !fence;
        } else if fence && role == Role::Assistant {
            push(&mut cells, "  ", Tone::Muted);
            push(&mut cells, content, Tone::Code);
        } else if role == Role::Assistant {
            if table_separator(trimmed) {
                in_table = true;
                continue;
            }
            if let Some(columns) = table_row_columns(trimmed) {
                let next_columns = sources
                    .peek()
                    .and_then(|next| table_row_columns(next.trim()));
                let next_is_separator = sources
                    .peek()
                    .is_some_and(|next| table_separator(next.trim()));
                let next_is_same_width_row = next_columns
                    .as_ref()
                    .is_some_and(|next| next.len() == columns.len() && !next_is_separator);
                let is_header = next_is_separator || (!in_table && next_is_same_width_row);
                let is_bordered = trimmed.starts_with('|') && trimmed.ends_with('|');
                if is_header || in_table || is_bordered {
                    in_table = true;
                    let tone = if is_header { Tone::Strong } else { Tone::Plain };
                    for (index, column) in columns.iter().enumerate() {
                        if index > 0 {
                            push(&mut cells, "  │  ", Tone::Muted);
                        }
                        cells.extend(inline(column.trim(), tone));
                    }
                } else {
                    in_table = false;
                    cells = inline(content, Tone::Plain);
                }
            } else if let Some(heading) = trimmed
                .strip_prefix("# ")
                .or_else(|| trimmed.strip_prefix("## "))
                .or_else(|| trimmed.strip_prefix("### "))
            {
                in_table = false;
                cells = inline(heading, Tone::Heading);
            } else if let Some(quote) = trimmed.strip_prefix("> ") {
                in_table = false;
                push(&mut cells, "┃ ", Tone::Muted);
                cells.extend(inline(quote, Tone::Plain));
            } else if let Some(list) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                in_table = false;
                let indent = (content.len() - trimmed.len()).min(8);
                push(&mut cells, &" ".repeat(indent), Tone::Plain);
                push(&mut cells, "• ", Tone::Heading);
                cells.extend(inline(list, Tone::Plain));
            } else if let Some((number, rest)) = ordered_item(trimmed) {
                in_table = false;
                push(&mut cells, &format!("{number}. "), Tone::Heading);
                cells.extend(inline(rest, Tone::Plain));
            } else if matches!(trimmed, "---" | "***" | "___") {
                in_table = false;
                push(&mut cells, "────────", Tone::Muted);
            } else {
                in_table = false;
                cells = inline(content, Tone::Plain);
            }
        } else {
            let tone = if role == Role::Note {
                if trimmed.starts_with("· ✓") {
                    Tone::Success
                } else if trimmed.starts_with("· ✗") {
                    Tone::Error
                } else if trimmed.starts_with("· reasoning")
                    || trimmed.starts_with("· ◌")
                    || trimmed.starts_with("· ↳")
                    || trimmed.starts_with("· diff · ")
                {
                    Tone::Activity
                } else {
                    Tone::Muted
                }
            } else {
                Tone::Plain
            };
            push(&mut cells, content, tone);
        }
        if role == Role::Diff {
            if cells.is_empty() {
                rows.push(Row { role, cells });
            } else {
                rows.extend(cells.chunks(width).map(|chunk| Row {
                    role,
                    cells: chunk.to_vec(),
                }));
            }
        } else {
            rows.extend(wrap_cells(cells, width, role));
        }
    }
    rows
}

// A completed fence is required: while streaming, keep the original source.

pub(crate) fn mermaid_sources(text: &str) -> Vec<String> {
    let mut lines = text.lines().peekable();
    let mut assistant = false;
    let mut other_fence = false;
    let mut sources = Vec::new();
    while let Some(line) = lines.next() {
        let content = if let Some(rest) = line.strip_prefix("you › ") {
            assistant = false;
            other_fence = false;
            rest
        } else if let Some(rest) = line
            .strip_prefix("hibi › ")
            .or_else(|| line.strip_prefix("hibiscus › "))
        {
            assistant = true;
            other_fence = false;
            rest
        } else if line.trim_start().starts_with('·') {
            assistant = false;
            other_fence = false;
            line
        } else {
            line
        };
        if !assistant {
            continue;
        }
        if other_fence {
            if content.trim().starts_with("```") {
                other_fence = false;
            }
            continue;
        }
        if content.trim().eq_ignore_ascii_case("```mermaid") {
            if let Some((body, consumed)) = complete_mermaid(lines.clone()) {
                if !sources.contains(&body) {
                    if sources.len() == 8 {
                        sources.remove(0);
                    }
                    sources.push(body);
                }
                for _ in 0..consumed {
                    lines.next();
                }
            }
        } else if content.trim().starts_with("```") {
            other_fence = true;
        }
    }
    sources
}

// Hibiscus's terminal table renderer measures the entire block, then wraps each
// cell independently. Keep that property here so long values never shift the
// following column or spill into the next row.

#[cfg(test)]
mod tests;
