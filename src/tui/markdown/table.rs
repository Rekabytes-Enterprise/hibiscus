//! Pipe tables: detection, column layout and cell wrapping.
use super::*;

pub(super) fn render_table(table: &[Vec<&str>], width: usize) -> Vec<Row> {
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

pub(super) fn wrap_table_cell(cells: &[Cell], width: usize) -> Vec<Vec<Cell>> {
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

pub(super) fn table_separator(line: &str) -> bool {
    let Some(columns) = table_row_columns(line) else {
        return false;
    };
    columns.iter().all(|column| {
        let column = column.trim();
        column.chars().filter(|ch| *ch == '-').count() >= 3
            && column.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
    })
}

pub(super) fn table_row_columns(line: &str) -> Option<Vec<&str>> {
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
