//! Empty-chat presentation, never saved as conversation history.
use super::mode::Mode;
use std::fmt::Write;

pub(super) struct Context<'a> {
    pub workspace: &'a str,
    pub model: &'a str,
    pub mode: Mode,
    pub has_credentials: bool,
}
fn commands(context: &Context<'_>) -> String {
    let login = if context.has_credentials {
        ""
    } else {
        "/login  ·  "
    };
    format!(
        "{login}{}  ·  /models  ·  /help",
        context.mode.switch_command()
    )
}

pub(super) fn inline(context: Context<'_>, color: bool) -> String {
    let (brand, muted, reset) = if color {
        (
            "\x1b[1;38;2;225;80;130m",
            "\x1b[38;2;155;155;155m",
            "\x1b[0m",
        )
    } else {
        ("", "", "")
    };
    let clean = |text: &str| text.chars().filter(|c| !c.is_control()).collect::<String>();
    format!(
        "{brand}hibiscus{reset}{muted} {} · {} · CLI\n{}    {}{reset}\n",
        env!("CARGO_PKG_VERSION"),
        clean(context.workspace),
        clean(context.model),
        commands(&context),
    )
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
        Self(
            text.chars()
                .filter(|c| !c.is_control())
                .take(width)
                .map(|glyph| Cell { glyph, ink })
                .collect(),
        )
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
fn panel(width: usize, context: &Context<'_>) -> Vec<Row> {
    let inner = width - 4;
    let mut lines = vec![Row::text(
        &format!("╭{}╮", "─".repeat(width - 2)),
        Ink::Border,
        width,
    )];
    for (text, ink) in [
        (
            format!("› Hibiscus  v{}", env!("CARGO_PKG_VERSION")),
            Ink::Accent,
        ),
        (format!("Mode       {}", context.mode.label()), Ink::Muted),
        (format!("Model      {}", context.model), Ink::Text),
        (format!("Workspace  {}", context.workspace), Ink::Muted),
        (commands(context), Ink::Muted),
        (String::new(), Ink::Muted),
    ] {
        let mut row = Row::text("│ ", Ink::Border, width);
        let content = Row::text(&text, ink, inner);
        let length = content.0.len();
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

pub(super) fn rows(width: usize, height: usize, context: Context<'_>) -> Vec<Row> {
    let mut rows = Vec::new();
    if width >= LOGO_WIDTH + 3 + 32 && height >= 12 {
        let panel = panel(width - LOGO_WIDTH - 3, &context);
        for (mut logo, info) in logo().into_iter().zip(panel) {
            logo.0.extend([Cell::default(); 3]);
            logo.0.extend(info.0);
            rows.push(logo);
        }
    } else if width >= LOGO_WIDTH && height >= 13 {
        rows.extend(logo());
        rows.push(Row::text(
            &format!(
                "Hibiscus v{} · {} · {}",
                env!("CARGO_PKG_VERSION"),
                context.mode.label(),
                context.model
            ),
            Ink::Text,
            width,
        ));
        rows.push(Row::text(
            &format!("Workspace {}", context.workspace),
            Ink::Muted,
            width,
        ));
    } else {
        rows.push(Row::text(
            &format!("HIBI  ·  v{}", env!("CARGO_PKG_VERSION")),
            Ink::Accent,
            width,
        ));
        rows.push(Row::text(context.model, Ink::Text, width));
        rows.push(Row::text(context.workspace, Ink::Muted, width));
    }
    rows.push(Row(Vec::new()));
    if width >= 50 {
        rows.push(Row::text(
            &format!("Tip: {}", commands(&context)),
            Ink::Muted,
            width,
        ));
    } else {
        rows.push(Row::text(&commands(&context), Ink::Muted, width));
    }
    rows.push(Row::text("Enter to send · Esc to stop", Ink::Muted, width));
    rows.truncate(height);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tips_offer_other_view_and_hide_login_with_credentials() {
        for mode in [Mode::Tui, Mode::Cli] {
            for has_credentials in [false, true] {
                let context = Context {
                    workspace: "workspace",
                    model: "provider/model",
                    mode,
                    has_credentials,
                };
                let text = match mode {
                    Mode::Cli => inline(context, false),
                    Mode::Tui => rows(120, 20, context)
                        .into_iter()
                        .map(|r| r.render(false))
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                assert!(text.contains(mode.switch_command()));
                assert_eq!(text.contains("/login"), !has_credentials);
                assert!(text.contains(mode.label()));
            }
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
    fn welcome_fits_terminal_and_escapes_external_labels() {
        for width in 0..130 {
            for height in [0, 4, 12, 15, 24] {
                let rows = rows(
                    width,
                    height,
                    Context {
                        workspace: "bad\x1b[2Jworkspace",
                        model: "provider/model",
                        mode: Mode::Tui,
                        has_credentials: false,
                    },
                );
                assert!(rows.len() <= height);
                assert!(rows.iter().all(|r| r.0.len() <= width));
                assert!(rows.iter().all(|r| !r.render(false).contains('\x1b')));
            }
        }
    }
}
