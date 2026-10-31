//! Terminal key decoding shared by both views.
use std::sync::mpsc::Receiver;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Navigation {
    Escape,
    EscapeWith(u8),
    ScrollPage(i32),
    ScrollLines(i32),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    Newline,
    FollowUp,
    PasteImage,
    PasteStart,
    MouseScroll(i32, usize),
    Mouse(MouseEvent),
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MouseEvent {
    pub(crate) button: usize,
    pub(crate) column: usize,
    pub(crate) row: usize,
    pub(crate) release: bool,
}

pub(crate) fn sgr_mouse(sequence: &str) -> Option<MouseEvent> {
    let release = sequence.ends_with('m');
    let fields = sequence.strip_prefix('<')?.trim_end_matches(['M', 'm']);
    let mut fields = fields.split(';');
    let button = fields.next()?.parse().ok()?;
    let column = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
    let row = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
    if fields.next().is_some() {
        return None;
    }
    Some(MouseEvent {
        button,
        column,
        row,
        release,
    })
}

pub(crate) fn escape_key(events: &Receiver<u8>) -> Navigation {
    match events.recv_timeout(std::time::Duration::from_millis(25)) {
        Ok(b'[') => {}
        Ok(b'v') => return Navigation::PasteImage, // legacy Alt+V (Hibiscus's WSL default)
        Ok(b'\r' | b'\n') => return Navigation::FollowUp, // legacy Alt+Enter
        Ok(next) => return Navigation::EscapeWith(next),
        Err(_) => return Navigation::Escape,
    }
    let mut sequence = String::new();
    while let Ok(next) = events.recv_timeout(std::time::Duration::from_millis(25)) {
        sequence.push(next as char);
        if next.is_ascii_alphabetic() || next == b'~' || sequence.len() >= 32 {
            break;
        }
    }
    match sequence.as_str() {
        "118;3u" | "118;5u" | "27;3;118~" | "27;5;118~" | "118;3:1u" | "118;5:1u" | "118;3:2u"
        | "118;5:2u" => Navigation::PasteImage,
        "13;2u" | "27;2;13~" | "13;2~" | "13;5u" | "27;5;13~" | "13;5~" => Navigation::Newline,
        "13;3u" | "27;3;13~" | "13;3~" => Navigation::FollowUp,
        "3~" => Navigation::Delete,
        "200~" => Navigation::PasteStart,
        "C" => Navigation::Right,
        "D" => Navigation::Left,
        "5~" => Navigation::ScrollPage(1),
        "6~" => Navigation::ScrollPage(-1),
        "A" => Navigation::Up,
        "B" => Navigation::Down,
        "H" | "1~" => Navigation::Home,
        "F" | "4~" => Navigation::End,
        seq if (seq.starts_with("<64;") || seq.starts_with("<65;")) && seq.ends_with('M') => {
            let row = seq
                .trim_end_matches('M')
                .rsplit(';')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            Navigation::MouseScroll(if seq.starts_with("<64;") { 3 } else { -3 }, row)
        }
        seq if seq.starts_with('<') && (seq.ends_with('M') || seq.ends_with('m')) => {
            sgr_mouse(seq).map_or(Navigation::Other, Navigation::Mouse)
        }
        _ => Navigation::Other,
    }
}
