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
    pub(crate) fn update(&mut self, text: &str, width: usize) {
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
                    format(source, width)
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
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut role = Role::Assistant;
    let mut fence = false;
    let mut in_diff = false;
    let mut sources = text.lines().peekable();
    while let Some(source) = sources.next() {
        let content = if let Some(content) = source.strip_prefix("you › ") {
            role = Role::User;
            fence = false;
            in_diff = false;
            content
        } else if let Some(content) = source
            .strip_prefix("hibi › ")
            .or_else(|| source.strip_prefix("hibiscus › "))
        {
            role = Role::Assistant;
            fence = false;
            in_diff = false;
            let mut label = Vec::new();
            push(&mut label, "hibi", Tone::Activity);
            rows.extend(wrap_cells(label, width, role));
            content
        } else if source.trim_start().starts_with('·') {
            role = Role::Note;
            in_diff = source.trim_start().starts_with("· diff · ");
            source
        } else if in_diff && source.starts_with("    ") {
            role = Role::Diff;
            &source[4..]
        } else {
            source
        };
        let content = content.trim_end_matches('\r');
        let trimmed = content.trim_start();
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
            if trimmed.starts_with('|') && trimmed.ends_with('|') && table_separator(trimmed) {
                continue;
            }
            if trimmed.starts_with('|') && trimmed.ends_with('|') {
                let tone = if sources
                    .peek()
                    .is_some_and(|next| table_separator(next.trim()))
                {
                    Tone::Strong
                } else {
                    Tone::Plain
                };
                for (index, column) in trimmed.trim_matches('|').split('|').enumerate() {
                    if index > 0 {
                        push(&mut cells, "  │  ", Tone::Muted);
                    }
                    cells.extend(inline(column.trim(), tone));
                }
            } else if let Some(heading) = trimmed
                .strip_prefix("# ")
                .or_else(|| trimmed.strip_prefix("## "))
                .or_else(|| trimmed.strip_prefix("### "))
            {
                cells = inline(heading, Tone::Heading);
            } else if let Some(quote) = trimmed.strip_prefix("> ") {
                push(&mut cells, "┃ ", Tone::Muted);
                cells.extend(inline(quote, Tone::Plain));
            } else if let Some(list) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                let indent = (content.len() - trimmed.len()).min(8);
                push(&mut cells, &" ".repeat(indent), Tone::Plain);
                push(&mut cells, "• ", Tone::Heading);
                cells.extend(inline(list, Tone::Plain));
            } else if let Some((number, rest)) = ordered_item(trimmed) {
                push(&mut cells, &format!("{number}. "), Tone::Heading);
                cells.extend(inline(rest, Tone::Plain));
            } else if matches!(trimmed, "---" | "***" | "___") {
                push(&mut cells, "────────", Tone::Muted);
            } else {
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

fn table_separator(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('|')
        && line.ends_with('|')
        && line.trim_matches('|').split('|').all(|column| {
            let column = column.trim();
            column.chars().filter(|ch| *ch == '-').count() >= 3
                && column.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        })
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
    fn formats_tables_and_numbered_lists() {
        let rows = format(
            "hibiscus › | Name | Value |\n| :--- | ---: |\n| pink | 2 |\n1. first\n---",
            80,
        );
        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            [
                "hibi",
                "Name  │  Value",
                "pink  │  2",
                "1. first",
                "────────"
            ]
        );
        assert_eq!(rows[1].cells[0].tone, Tone::Strong);
    }
}
