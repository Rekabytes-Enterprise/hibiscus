use std::collections::HashMap;

/// Lightweight terminal Markdown for chat transcripts. This is a display
/// transform only: Pi and saved messages keep the original Markdown text.
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

/// Cache at explicit role-reset boundaries, not arbitrary lines: fences and
/// table lookahead can change the interpretation of previously received text.
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
            if offset > 0
                && (line.starts_with("you › ")
                    || line.starts_with("hibi › ")
                    || line.starts_with("hibiscus › "))
            {
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

fn push(cells: &mut Vec<Cell>, text: &str, tone: Tone) {
    cells.extend(text.chars().map(|ch| Cell { ch, tone }));
}

fn inline(text: &str, default: Tone) -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut rest = text;
    let mut strong = false;
    let mut emphasis = false;
    let mut code = false;
    // A failed link search cannot succeed in a shorter suffix of this line.
    let mut links_exhausted = false;
    while !rest.is_empty() {
        let tone = if code {
            Tone::Code
        } else if strong {
            Tone::Strong
        } else if emphasis {
            Tone::Emphasis
        } else {
            default
        };
        if rest.starts_with('\\') {
            let escaped = &rest[1..];
            if let Some(ch) = escaped.chars().next() {
                if "\\*`_[]".contains(ch) {
                    cells.push(Cell { ch, tone });
                    rest = &escaped[ch.len_utf8()..];
                    continue;
                }
            }
        }
        if rest.starts_with('`') && (code || rest[1..].contains('`')) {
            code = !code;
            rest = &rest[1..];
            continue;
        }
        if !code {
            if rest.starts_with("**") || rest.starts_with("__") {
                if strong || rest[2..].contains(&rest[..2]) {
                    strong = !strong;
                } else {
                    push(&mut cells, &rest[..2], tone);
                }
                rest = &rest[2..];
                continue;
            }
            if rest.starts_with('_')
                && cells
                    .last()
                    .is_some_and(|cell: &Cell| cell.ch.is_alphanumeric())
                && rest[1..]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphanumeric())
            {
                push(&mut cells, "_", tone);
                rest = &rest[1..];
                continue;
            }
            if (rest.starts_with('*') || rest.starts_with('_'))
                && (emphasis || rest[1..].contains(&rest[..1]))
            {
                emphasis = !emphasis;
                rest = &rest[1..];
                continue;
            }
            if !links_exhausted && rest.starts_with('[') {
                if let Some(end) = rest.find("](") {
                    if let Some(close) = rest[end + 2..].find(')') {
                        push(&mut cells, &rest[1..end], Tone::Link);
                        rest = &rest[end + close + 3..];
                        continue;
                    }
                }
                links_exhausted = true;
            }
        }
        let ch = rest.chars().next().expect("nonempty text");
        cells.push(Cell { ch, tone });
        rest = &rest[ch.len_utf8()..];
    }
    cells
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
    let mut sources = text.lines().peekable();
    while let Some(source) = sources.next() {
        let content = if let Some(content) = source.strip_prefix("you › ") {
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
            let mut label = Vec::new();
            push(&mut label, "hibi", Tone::Activity);
            rows.extend(wrap_cells(label, width, role));
            content
        } else if source.trim_start().starts_with('·') {
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
fn complete_mermaid(
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

// Pi's terminal table renderer measures the entire block, then wraps each
// cell independently. Keep that property here so long values never shift the
// following column or spill into the next row.
fn render_table(table: &[Vec<&str>], width: usize) -> Vec<Row> {
    let count = table[0].len();
    let overhead = 3 * count + 1; // │ cell │ cell │
    if width < overhead + count {
        return table
            .iter()
            .flat_map(|columns| {
                wrap_cells(
                    inline(&columns.join(" | "), Tone::Plain),
                    width,
                    Role::Assistant,
                )
            })
            .collect();
    }
    let content_width = width - overhead;
    let styled: Vec<Vec<Vec<Cell>>> = table
        .iter()
        .enumerate()
        .map(|(index, columns)| {
            columns
                .iter()
                .map(|column| {
                    inline(
                        column.trim(),
                        if index == 0 {
                            Tone::Strong
                        } else {
                            Tone::Plain
                        },
                    )
                })
                .collect()
        })
        .collect();
    let natural: Vec<usize> = (0..count)
        .map(|col| {
            styled
                .iter()
                .map(|row| row[col].len())
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    let mut widths: Vec<usize> = natural.iter().map(|size| (*size).min(8)).collect();
    // If even the minimums do not fit, divide the available cells fairly.
    while widths.iter().sum::<usize>() > content_width {
        let widest = (0..count)
            .filter(|&i| widths[i] > 1)
            .max_by_key(|&i| widths[i]);
        if let Some(i) = widest {
            widths[i] -= 1;
        } else {
            break;
        }
    }
    while widths.iter().sum::<usize>() < content_width {
        let neediest = (0..count)
            .filter(|&i| widths[i] < natural[i])
            .max_by_key(|&i| natural[i] - widths[i]);
        if let Some(i) = neediest {
            widths[i] += 1;
        } else {
            break;
        }
    }
    let border = |left: &str, joint: &str, right: &str| {
        let mut cells = Vec::new();
        push(
            &mut cells,
            &format!(
                "{left}─{}─{right}",
                widths
                    .iter()
                    .map(|w| "─".repeat(*w))
                    .collect::<Vec<_>>()
                    .join(&format!("─{joint}─"))
            ),
            Tone::TableBorder,
        );
        Row {
            role: Role::Assistant,
            cells,
        }
    };
    let mut result = vec![border("╭", "┬", "╮")];
    for (row_index, row) in styled.iter().enumerate() {
        if row_index > 0 {
            result.push(border("├", "┼", "┤"));
        }
        let wrapped: Vec<Vec<Vec<Cell>>> = row
            .iter()
            .enumerate()
            .map(|(col, cells)| wrap_table_cell(cells, widths[col]))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            let mut cells = Vec::new();
            push(&mut cells, "│ ", Tone::TableBorder);
            for col in 0..count {
                let part = wrapped[col].get(line).map(Vec::as_slice).unwrap_or(&[]);
                cells.extend_from_slice(part);
                push(
                    &mut cells,
                    &" ".repeat(widths[col] - part.len()),
                    Tone::Plain,
                );
                push(
                    &mut cells,
                    if col + 1 == count { " │" } else { " │ " },
                    Tone::TableBorder,
                );
            }
            result.push(Row {
                role: Role::Assistant,
                cells,
            });
        }
    }
    result.push(border("╰", "┴", "╯"));
    result
}

fn wrap_table_cell(cells: &[Cell], width: usize) -> Vec<Vec<Cell>> {
    if cells.is_empty() {
        return vec![Vec::new()];
    }
    let mut lines = Vec::new();
    let mut rest = cells;
    while !rest.is_empty() {
        let take = if rest.len() <= width {
            rest.len()
        } else {
            rest[..width]
                .iter()
                .rposition(|cell| cell.ch.is_whitespace())
                .filter(|&index| index > 0)
                .unwrap_or(width)
        };
        let mut line = rest[..take].to_vec();
        while line.last().is_some_and(|cell| cell.ch.is_whitespace()) {
            line.pop();
        }
        lines.push(line);
        rest = &rest[take..];
        while rest.first().is_some_and(|cell| cell.ch.is_whitespace()) {
            rest = &rest[1..];
        }
    }
    lines
}

fn table_separator(line: &str) -> bool {
    let Some(columns) = table_row_columns(line) else {
        return false;
    };
    columns.iter().all(|column| {
        let column = column.trim();
        column.chars().filter(|ch| *ch == '-').count() >= 3
            && column.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
    })
}

fn table_row_columns(line: &str) -> Option<Vec<&str>> {
    let line = line.trim();
    if !line.contains('|') {
        return None;
    }
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    if !line.contains('|') {
        return None;
    }
    Some(line.split('|').collect())
}

fn ordered_item(text: &str) -> Option<(&str, &str)> {
    let split = text.find('.')?;
    let (number, rest) = text.split_at(split);
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some((number, rest.strip_prefix(". ")?))
}

fn wrap_cells(cells: Vec<Cell>, width: usize, role: Role) -> Vec<Row> {
    if cells.is_empty() {
        return vec![Row { role, cells }];
    }
    let mut result = Vec::new();
    let mut cells = cells.as_slice();
    while !cells.is_empty() {
        let continuation = !result.is_empty() && width > 4;
        let capacity = width - usize::from(continuation) * 2;
        let take = if cells.len() <= capacity {
            cells.len()
        } else {
            cells[..capacity]
                .iter()
                .rposition(|cell| cell.ch.is_whitespace())
                .filter(|pos| *pos >= capacity / 3)
                .unwrap_or(capacity)
        };
        let mut row = Vec::new();
        if continuation {
            push(&mut row, "  ", Tone::Plain);
        }
        row.extend_from_slice(&cells[..take]);
        cells = &cells[take..];
        if !cells.is_empty() {
            while row.last().is_some_and(|cell| cell.ch.is_whitespace()) {
                row.pop();
            }
            while cells.first().is_some_and(|cell| cell.ch.is_whitespace()) {
                cells = &cells[1..];
            }
        }
        result.push(Row { role, cells: row });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(row: &Row) -> String {
        row.cells.iter().map(|cell| cell.ch).collect()
    }

    #[test]
    fn layout_matches_full_format_across_streaming_resize_and_replacement() {
        let mut layout = Layout::default();
        let text = "you › literal **text**\n\nhibi › ## Title\n| A | B |\n| --- | --- |\n```rust\nlet x = 1;\n```\n\nyou › next\nhibi › [link](url)";
        for width in [73, 12, 1, 73] {
            for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
                layout.update(&text[..end], width);
                assert_eq!(
                    layout
                        .rows()
                        .iter()
                        .map(|r| (**r).clone())
                        .collect::<Vec<_>>(),
                    format(&text[..end], width)
                );
            }
        }
        let first = layout.rows()[0].clone();
        layout.update(&format!("{text} more"), 73);
        assert!(std::rc::Rc::ptr_eq(&first, &layout.rows()[0]));
        layout.update("hibi › replacement", 73);
        assert_eq!(
            layout
                .rows()
                .iter()
                .map(|r| (**r).clone())
                .collect::<Vec<_>>(),
            format("hibi › replacement", 73)
        );
    }

    #[test]
    fn long_lines_and_unmatched_links_remain_literal() {
        for source in ["[".repeat(65536), "x".repeat(65536), " ".repeat(65536)] {
            let rows = format(&source, 73);
            assert!(rows.iter().all(|r| r.cells.len() <= 73));
            if !source.starts_with(' ') {
                assert_eq!(
                    rows.iter()
                        .map(|r| text(r).trim_start().to_owned())
                        .collect::<String>(),
                    source
                );
            }
        }
        assert_eq!(text(&format("[no closing](", 73)[0]), "[no closing](");
    }

    #[test]
    fn renders_lists_bold_code_links_and_fences_without_markers() {
        let rows = format("hibiscus › ## Overview\n- **Bold** and `code` [link](https://example.com)\n```rust\nlet x = 2;\n```", 100);
        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            [
                "hibi",
                "Overview",
                "• Bold and code link",
                "  rust",
                "  let x = 2;",
                ""
            ]
        );
        assert!(rows[1].cells.iter().all(|cell| cell.tone == Tone::Heading));
        assert_eq!(rows[2].cells[2].tone, Tone::Strong);
        assert!(rows[4]
            .cells
            .iter()
            .skip(2)
            .all(|cell| cell.tone == Tone::Code));
    }

    #[test]
    fn wraps_without_losing_roles_and_keeps_user_text_literal() {
        let rows = format("you › **literal**\nhibiscus › **hello** world", 7);
        assert_eq!(text(&rows[0]), "**liter");
        assert_eq!(rows[0].role, Role::User);
        assert_eq!(rows.last().unwrap().role, Role::Assistant);
        assert_eq!(
            rows.iter()
                .filter(|row| row.role == Role::User)
                .map(|row| text(row).trim_start().to_owned())
                .collect::<Vec<_>>()
                .join(""),
            "**literal**"
        );
    }

    #[test]
    fn wraps_at_words_and_indents_continuations() {
        let rows = format("hibiscus › - testing a fairly long line", 15);
        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            ["hibi", "• testing a", "  fairly long", "  line"]
        );
        assert!(rows.iter().all(|r| r.cells.len() <= 15));
    }

    #[test]
    fn unmatched_markers_remain_visible() {
        let rows = format("hibiscus › src/my_file_name.rs and **unfinished", 80);
        assert_eq!(text(&rows[1]), "src/my_file_name.rs and **unfinished");
    }

    #[test]
    fn unclosed_fence_does_not_leak_into_next_reply() {
        let rows = format(
            "hibiscus › ```rust\nlet x = 1\nyou › next\nhibiscus › **answer**",
            80,
        );
        assert_eq!(text(rows.last().unwrap()), "answer");
        assert_eq!(rows.last().unwrap().cells[0].tone, Tone::Strong);
    }

    #[test]
    fn colors_activity_and_keeps_edit_diff_literal() {
        let rows = format("  · reasoning · 3s\n  · diff · updated.md\n     11 context\n    -12 **old**\n    +12 `new`\n  · ✓ done", 80);
        assert_eq!(rows[0].cells[0].tone, Tone::Activity);
        assert_eq!(rows[1].cells[0].tone, Tone::Activity);
        assert_eq!(rows[2].role, Role::Diff);
        assert_eq!(rows[2].cells[0].tone, Tone::DiffContext);
        assert_eq!(text(&rows[3]), "-12 **old**");
        assert_eq!(rows[3].cells[0].tone, Tone::DiffRemoved);
        assert_eq!(text(&rows[4]), "+12 `new`");
        assert_eq!(rows[4].cells[0].tone, Tone::DiffAdded);
        assert_eq!(rows[5].cells[0].tone, Tone::Success);
    }

    #[test]
    fn mermaid_stays_source_until_a_completed_run_supplies_art() {
        let message = "you › ```mermaid\nflowchart LR\n  X --> Y\n```\n\nhibi › Before\n```mermaid\nflowchart LR\n  A[Start] --> B[Done]\n```\nAfter";
        let body = "flowchart LR\n  A[Start] --> B[Done]\n";
        assert_eq!(mermaid_sources(message), vec![body]);
        let mut layout = Layout::default();
        layout.update(message, 60);
        let raw: Vec<String> = layout.rows().iter().map(|row| text(row)).collect();
        assert!(raw.iter().any(|line| line.contains("A[Start] --> B[Done]")));
        let mut diagrams = HashMap::new();
        diagrams.insert(
            body.into(),
            vec!["╭─────╮".into(), "│ Done│".into(), "╰─────╯".into()],
        );
        layout.invalidate();
        layout.update_with_diagrams(message, 60, &diagrams);
        let final_rows: Vec<String> = layout.rows().iter().map(|row| text(row)).collect();
        assert!(final_rows.iter().any(|line| line == "Before"));
        assert!(final_rows.iter().any(|line| line == "After"));
        assert!(final_rows.iter().any(|line| line == "╭─────╮"));
        assert!(!final_rows
            .iter()
            .any(|line| line.contains("A[Start] --> B[Done]")));
        layout.update_with_diagrams(message, 5, &diagrams);
        assert!(layout.rows().iter().any(|row| text(row).contains("flow")));
        // A partial fence never consumes the chat that follows it.
        assert!(mermaid_sources("hibi › ```mermaid\nA --> B").is_empty());
        assert!(mermaid_sources("hibi › ```mermaid\nA --> B\nyou › ```\nhibi › Later").is_empty());
        assert!(mermaid_sources(
            "hibi › prose\n  · diff · file\n```mermaid\nflowchart LR\n  A --> B\n```"
        )
        .is_empty());
    }

    #[test]
    fn formats_tables_and_numbered_lists() {
        let rows = format(
            "hibiscus › | Name | Value |\n| :--- | ---: |\n| pink | 2 |\n1. first\n---",
            80,
        );
        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            [
                "hibi",
                "╭──────┬───────╮",
                "│ Name │ Value │",
                "├──────┼───────┤",
                "│ pink │ 2     │",
                "╰──────┴───────╯",
                "1. first",
                "────────"
            ]
        );
        assert_eq!(rows[2].cells[2].tone, Tone::Strong);
    }

    #[test]
    fn formats_tables_without_outer_pipes() {
        let rows = format(
            "hibiscus › Name | Value\n:--- | ---:\npink | 2\nplain | text\n\nnot | a table",
            80,
        );
        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            [
                "hibi",
                "╭───────┬───────╮",
                "│ Name  │ Value │",
                "├───────┼───────┤",
                "│ pink  │ 2     │",
                "├───────┼───────┤",
                "│ plain │ text  │",
                "╰───────┴───────╯",
                "",
                "not | a table"
            ]
        );
        assert_eq!(rows[2].cells[2].tone, Tone::Strong);
        assert_eq!(rows[4].cells[2].tone, Tone::Plain);
    }

    #[test]
    fn wraps_long_table_cells_without_shifting_columns() {
        let rows = format(
            "hibi › Aspect | JavaScript | TypeScript\nTypes | Dynamic; errors appear at runtime | Static types catch many errors before runtime\nRuntime | Runs directly in browsers and Node.js | Usually compiled to JavaScript first",
            55,
        );
        let lines: Vec<String> = rows.iter().map(text).collect();
        let table: Vec<&String> = lines.iter().filter(|line| line.starts_with('│')).collect();
        assert!(table.len() > 3, "{lines:?}");
        assert!(rows.iter().all(|row| row.cells.len() <= 55), "{lines:?}");
        let borders: Vec<usize> = table[0].match_indices('│').map(|(i, _)| i).collect();
        for line in table {
            assert_eq!(
                line.match_indices('│').map(|(i, _)| i).collect::<Vec<_>>(),
                borders,
                "{line}"
            );
        }
        assert!(lines.iter().any(|line| line.contains("errors appear")));
        assert!(lines.iter().any(|line| line.contains("JavaScript first")));
    }

    #[test]
    fn formats_consecutive_pipe_rows_without_a_separator() {
        let rows = format(
            "hibiscus › Aspect | JavaScript | TypeScript\nTypes | Dynamic | Static\nRuntime | Direct | Compiled\n\nA | single row stays literal",
            100,
        );
        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            [
                "hibi",
                "╭─────────┬────────────┬────────────╮",
                "│ Aspect  │ JavaScript │ TypeScript │",
                "├─────────┼────────────┼────────────┤",
                "│ Types   │ Dynamic    │ Static     │",
                "├─────────┼────────────┼────────────┤",
                "│ Runtime │ Direct     │ Compiled   │",
                "╰─────────┴────────────┴────────────╯",
                "",
                "A | single row stays literal"
            ]
        );
        assert_eq!(rows[2].cells[2].tone, Tone::Strong);
        assert_eq!(rows[4].cells[2].tone, Tone::Plain);
    }
}
