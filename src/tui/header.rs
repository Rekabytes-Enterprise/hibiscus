//! Persistent fullscreen header, never saved as conversation history.
#[cfg(test)]
use crate::ui::mode::Mode;
use crate::ui::welcome::{commands, Context};
use std::fmt::Write;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) struct Header<'a> {
    pub info: Context<'a>,
    pub thinking: Option<&'a str>,
    pub session: &'a str,
}

const LETTERS: [[&str; 7]; 4] = [
    [
        "██   ██",
        "██   ██",
        "██   ██",
        "███████",
        "██   ██",
        "██   ██",
        "██   ██",
    ],
    [
        "███████",
        "  ███  ",
        "  ███  ",
        "  ███  ",
        "  ███  ",
        "  ███  ",
        "███████",
    ],
    [
        "██████ ",
        "██   ██",
        "██   ██",
        "██████ ",
        "██   ██",
        "██   ██",
        "██████ ",
    ],
    [
        "███████",
        "  ███  ",
        "  ███  ",
        "  ███  ",
        "  ███  ",
        "  ███  ",
        "███████",
    ],
];
const LOGO_WIDTH: usize = 38;
const LOGO_HEIGHT: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ink {
    Text,
    Muted,
    Border,
    Accent,
    Logo(usize),
    Shadow(usize),
}
#[derive(Clone, Copy)]
struct Cell {
    glyph: char,
    ink: Ink,
}
impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            ink: Ink::Muted,
        }
    }
}
pub(super) struct Row(Vec<Cell>);
impl Row {
    fn text(text: &str, ink: Ink, width: usize) -> Self {
        let mut cells = Vec::new();
        let mut text_run = String::new();
        for glyph in text.chars().filter(|c| !c.is_control()) {
            let size = glyph.width().unwrap_or(0);
            // A leading combining mark must not attach to the border or a
            // preceding logo cell when separate text runs are joined.
            if size == 0 && cells.is_empty() {
                continue;
            }
            text_run.push(glyph);
            if text_run.width() > width {
                break;
            }
            cells.push(Cell { glyph, ink });
        }
        Self(cells)
    }
    fn columns(&self) -> usize {
        self.0
            .iter()
            .map(|cell| cell.glyph)
            .collect::<String>()
            .width()
    }
    pub(super) fn render(&self, color: bool) -> String {
        let mut out = String::new();
        let mut previous = None;
        for cell in &self.0 {
            if color && previous != Some(cell.ink) {
                let [r, g, b] = cell.ink.rgb();
                // Background-filled cells have no font/line-height seams.
                let plane = if matches!(cell.ink, Ink::Logo(_) | Ink::Shadow(_)) {
                    48
                } else {
                    38
                };
                let _ = write!(out, "\x1b[0m\x1b[{plane};2;{r};{g};{b}m");
                previous = Some(cell.ink);
            }
            out.push(
                if color && matches!(cell.ink, Ink::Logo(_) | Ink::Shadow(_)) {
                    ' '
                } else {
                    cell.glyph
                },
            );
        }
        if color {
            out.push_str("\x1b[0m");
        }
        out
    }
}
impl Ink {
    fn rgb(self) -> [u8; 3] {
        match self {
            Self::Text => [255, 241, 246],
            Self::Muted => [158, 133, 151],
            Self::Border => [113, 84, 107],
            Self::Accent => [255, 155, 187],
            Self::Logo(x) | Self::Shadow(x) => {
                let start = [184usize, 57, 101];
                let end = [236usize, 74, 125];
                let mut rgb = [0; 3];
                for i in 0..3 {
                    let value = (start[i] * (LOGO_WIDTH - 1 - x) + end[i] * x) / (LOGO_WIDTH - 1);
                    rgb[i] = if matches!(self, Self::Shadow(_)) {
                        (value / 2) as u8
                    } else {
                        value as u8
                    };
                }
                rgb
            }
        }
    }
}
fn logo() -> Vec<Row> {
    let mut grid = vec![vec![Cell::default(); LOGO_WIDTH]; LOGO_HEIGHT];
    for (letter, bitmap) in LETTERS.iter().enumerate() {
        for (y, line) in bitmap.iter().enumerate() {
            for (x, glyph) in line.chars().enumerate() {
                if glyph == '█' {
                    let x = letter * 9 + x + (6 - y) / 2;
                    grid[y + 1][x + 1] = Cell {
                        glyph: '░',
                        ink: Ink::Shadow(x + 1),
                    };
                }
            }
        }
    }
    for (letter, bitmap) in LETTERS.iter().enumerate() {
        for (y, line) in bitmap.iter().enumerate() {
            for (x, glyph) in line.chars().enumerate() {
                if glyph == '█' {
                    let x = letter * 9 + x + (6 - y) / 2;
                    grid[y][x] = Cell {
                        glyph,
                        ink: Ink::Logo(x),
                    };
                }
            }
        }
    }
    grid.into_iter().map(Row).collect()
}
fn panel(width: usize, header: &Header<'_>) -> Vec<Row> {
    let context = &header.info;
    let inner = width - 4;
    let mut lines = vec![Row::text(
        &format!("╭{}╮", "─".repeat(width - 2)),
        Ink::Border,
        width,
    )];
    for (text, ink) in [
        (
            format!(
                "› Hibiscus  v{}  ·  {}",
                env!("CARGO_PKG_VERSION"),
                context.mode.label()
            ),
            Ink::Accent,
        ),
        (format!("Model      {}", context.model), Ink::Text),
        (
            format!("Thinking   {}", header.thinking.unwrap_or("unavailable")),
            Ink::Text,
        ),
        (format!("Workspace  {}", context.workspace), Ink::Muted),
        (format!("Session    {}", header.session), Ink::Muted),
        (commands(context), Ink::Muted),
    ] {
        let mut row = Row::text("│ ", Ink::Border, width);
        let content = Row::text(&text, ink, inner);
        let length = content.columns();
        row.0.extend(content.0);
        row.0
            .extend(std::iter::repeat_n(Cell::default(), inner - length));
        row.0.extend(Row::text(" │", Ink::Border, width).0);
        lines.push(row);
    }
    lines.push(Row::text(
        &format!("╰{}╯", "─".repeat(width - 2)),
        Ink::Border,
        width,
    ));
    lines
}

pub(super) fn rows(width: usize, height: usize, header: Header<'_>) -> Vec<Row> {
    let context = &header.info;
    let mut rows = if width >= LOGO_WIDTH + 3 + 32 && height >= LOGO_HEIGHT {
        let panel = panel(width - LOGO_WIDTH - 3, &header);
        logo()
            .into_iter()
            .zip(panel)
            .map(|(mut logo, info)| {
                logo.0.extend([Cell::default(); 3]);
                logo.0.extend(info.0);
                logo
            })
            .collect::<Vec<_>>()
    } else if height < 3 {
        vec![
            Row::text(
                &format!(
                    "HIBI · Thinking {}",
                    header.thinking.unwrap_or("unavailable")
                ),
                Ink::Accent,
                width,
            ),
            Row::text(context.model, Ink::Text, width),
        ]
    } else {
        vec![
            Row::text(
                &format!(
                    "HIBI  ·  v{}  ·  {}",
                    env!("CARGO_PKG_VERSION"),
                    context.mode.label()
                ),
                Ink::Accent,
                width,
            ),
            Row::text(context.model, Ink::Text, width),
            Row::text(
                &format!(
                    "Thinking {} · {}",
                    header.thinking.unwrap_or("unavailable"),
                    header.session
                ),
                Ink::Text,
                width,
            ),
            Row::text(&commands(context), Ink::Muted, width),
            Row::text(
                &format!("Workspace {}", context.workspace),
                Ink::Muted,
                width,
            ),
        ]
    };
    rows.truncate(height);
    rows
}

pub(super) fn tips(width: usize, height: usize, context: &Context<'_>) -> Vec<Row> {
    let mut rows = vec![
        Row::text(&commands(context), Ink::Muted, width),
        Row::text("Enter to send · Esc to stop", Ink::Muted, width),
    ];
    rows.truncate(height);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tips_offer_other_view_and_hide_login_with_credentials() {
        for has_credentials in [false, true] {
            let context = Context {
                workspace: "workspace",
                model: "provider/model",
                mode: Mode::Tui,
                has_credentials,
            };
            let text = rows(
                120,
                20,
                Header {
                    info: context,
                    thinking: Some("high"),
                    session: "test-session",
                },
            )
            .into_iter()
            .map(|r| r.render(false))
            .collect::<Vec<_>>()
            .join("\n");
            assert!(text.contains("/cli"));
            assert_eq!(text.contains("/login"), !has_credentials);
            assert!(text.contains("TUI"));
        }
    }

    #[test]
    fn banner_is_slanted_and_uses_solid_cells_in_color_terminals() {
        let rows = logo();
        let top = rows[0]
            .0
            .iter()
            .position(|c| matches!(c.ink, Ink::Logo(_)))
            .unwrap();
        let bottom = rows[6]
            .0
            .iter()
            .position(|c| matches!(c.ink, Ink::Logo(_)))
            .unwrap();
        assert!(top > bottom, "top of the lettering should lean right");
        for row in rows {
            let colored = row.render(true);
            assert!(colored.contains("\x1b[48;2;"));
            assert!(!colored.contains(['█', '░']));
        }
    }
    #[test]
    fn header_fits_terminal_and_escapes_external_labels() {
        for width in 0..130 {
            for height in [0, 4, 12, 15, 24] {
                let rows = rows(
                    width,
                    height,
                    Header {
                        info: Context {
                            workspace: "bad\x1b[2Jworkspace",
                            model: "provider/model",
                            mode: Mode::Tui,
                            has_credentials: false,
                        },
                        thinking: Some("high\x1b[2J"),
                        session: "session\x1b[2J",
                    },
                );
                assert!(rows.len() <= height);
                assert!(rows.iter().all(|r| r.columns() <= width));
                assert!(rows.iter().all(|r| !r.render(false).contains('\x1b')));
            }
        }
    }
    #[test]
    fn every_layout_shows_the_actual_thinking_level_without_a_second_brand_header() {
        for (width, height) in [(120, 20), (76, 8), (42, 12), (42, 3), (42, 1)] {
            for thinking in [None, Some("off"), Some("high")] {
                let rows = rows(
                    width,
                    height,
                    Header {
                        info: Context {
                            workspace: "workspace",
                            model: "provider/model",
                            mode: Mode::Tui,
                            has_credentials: true,
                        },
                        thinking,
                        session: "session-id",
                    },
                );
                let text = rows
                    .iter()
                    .map(|row| row.render(false))
                    .collect::<Vec<_>>()
                    .join("\n");
                let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(
                    words.contains(&format!("Thinking {}", thinking.unwrap_or("unavailable"))),
                    "{width}x{height}: {words}"
                );
                assert!(!text.contains("✿ hibiscus"));
                assert!(rows.len() <= height);
            }
        }
    }
    #[test]
    fn wide_and_combining_labels_keep_panel_borders_within_the_terminal() {
        for width in [1, 2, 20, 42, 73, 80, 120] {
            for height in [1, 4, 8] {
                let workspace = "工作區👩‍💻🦀e\u{301}".repeat(20);
                let rows = rows(
                    width,
                    height,
                    Header {
                        info: Context {
                            workspace: &workspace,
                            model: "供應商/模型🦀",
                            mode: Mode::Tui,
                            has_credentials: false,
                        },
                        thinking: Some("high"),
                        session: "session-工作區",
                    },
                );
                for row in &rows {
                    let plain = row.render(false);
                    assert!(
                        UnicodeWidthStr::width(plain.as_str()) <= width,
                        "{width}: {plain}"
                    );
                    if width >= LOGO_WIDTH + 3 + 32 && height >= LOGO_HEIGHT {
                        assert_eq!(UnicodeWidthStr::width(plain.as_str()), width);
                    }
                }
            }
        }
        let text = Row::text("\u{301}e\u{301}界", Ink::Text, 1).render(false);
        assert_eq!(text, "e\u{301}");
    }
}
