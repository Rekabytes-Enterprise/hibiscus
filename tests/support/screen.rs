//! Tiny test-only terminal model for row-diff output. Integration assertions
//! must inspect the reconstructed view, not assume every redraw is a full frame.
use std::collections::BTreeMap;

pub fn snapshots(output: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut rows = BTreeMap::<usize, Vec<char>>::new();
    let (mut row, mut col) = (1, 0);
    let mut chars = output.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.next() {
                Some('[') => {
                    let mut args = String::new();
                    let end = loop {
                        let Some(ch) = chars.next() else {
                            break '\0';
                        };
                        if ('@'..='~').contains(&ch) {
                            break ch;
                        }
                        args.push(ch);
                    };
                    match end {
                        'H' | 'f' => {
                            let mut parts = args.split(';');
                            row = parts.next().and_then(|x| x.parse().ok()).unwrap_or(1);
                            col = parts
                                .next()
                                .and_then(|x| x.parse::<usize>().ok())
                                .unwrap_or(1)
                                .saturating_sub(1);
                        }
                        'K' => {
                            rows.entry(row).or_default().truncate(col);
                        }
                        'J' if args == "2" => {
                            rows.clear();
                        }
                        'J' => {
                            rows.retain(|r, _| *r <= row);
                            rows.entry(row).or_default().truncate(col);
                        }
                        'l' if args == "?2026" => result.push(view(&rows)),
                        _ => {}
                    }
                }
                Some(']') => {
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' {
                            break;
                        }
                        if ch == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => col = 0,
            '\n' => row += 1,
            ch if !ch.is_control() && row <= 200 && col < 1000 => {
                let cells = rows.entry(row).or_default();
                if col >= cells.len() {
                    cells.resize(col + 1, ' ');
                }
                cells[col] = ch;
                col += 1;
            }
            _ => {}
        }
    }
    result
}

fn view(rows: &BTreeMap<usize, Vec<char>>) -> String {
    let height = rows.keys().next_back().copied().unwrap_or(0).min(200);
    (1..=height)
        .map(|row| {
            rows.get(&row)
                .map(|cells| cells.iter().collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
