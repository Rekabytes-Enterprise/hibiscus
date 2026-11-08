//! Inline Markdown (bold, code, links), list items and word wrapping.
use super::*;

pub(super) fn push(cells: &mut Vec<Cell>, text: &str, tone: Tone) {
    cells.extend(text.chars().map(|ch| Cell { ch, tone }));
}

pub(super) fn inline(text: &str, default: Tone) -> Vec<Cell> {
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

pub(super) fn ordered_item(text: &str) -> Option<(&str, &str)> {
    let split = text.find('.')?;
    let (number, rest) = text.split_at(split);
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some((number, rest.strip_prefix(". ")?))
}

pub(super) fn wrap_cells(cells: Vec<Cell>, width: usize, role: Role) -> Vec<Row> {
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
