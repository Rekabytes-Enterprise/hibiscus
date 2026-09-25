//! Shared docked dialog chrome and state. Models/sessions and Pi extension
//! dialogs use the same borders, margins, selection rows and keyboard hints.
use super::{picker::Picker, screen::Navigation};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub(crate) const BORDER: &str = "38;2;184;57;101";
const TITLE: &str = "1;38;2;236;74;125";
const SELECTED: &str = "1;38;2;255;155;187;48;2;77;27;51";
const TEXT: &str = "38;2;255;220;230";

pub(crate) struct Row {
    pub(crate) text: String,
    pub(crate) style: &'static str,
}

fn clip(text: &str, width: usize) -> String {
    let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
    if text.chars().count() <= width {
        text
    } else if width == 0 {
        String::new()
    } else {
        text.chars().take(width - 1).collect::<String>() + "…"
    }
}

fn inner(text: &str, width: usize, style: &'static str, edge: char) -> Row {
    let text = clip(text, width.saturating_sub(4));
    Row {
        text: format!(
            "│ {text}{} {edge}",
            " ".repeat(width.saturating_sub(4 + text.chars().count()))
        ),
        style,
    }
}

fn panel(title: &str, body: Vec<Row>, hint: &str, width: usize) -> Vec<Row> {
    let title = clip(&format!("✿ {title}"), width.saturating_sub(5));
    let mut rows = vec![Row {
        text: format!(
            "╭─ {title} {}╮",
            "─".repeat(width.saturating_sub(title.chars().count() + 5))
        ),
        style: TITLE,
    }];
    rows.extend(body);
    rows.push(inner(hint, width, BORDER, '│'));
    rows.push(Row {
        text: format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
        style: BORDER,
    });
    rows
}

fn choices(picker: &Picker, width: usize, height: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let visible = picker.visible(height);
    for line in 0..visible {
        let index = picker.first + line;
        let mark = if index == picker.selected { "❯" } else { " " };
        let suffix = if picker.current == Some(index) {
            "  ● current"
        } else {
            ""
        };
        let label = clip(
            &picker.items[index],
            width.saturating_sub(6 + suffix.chars().count()),
        );
        let edge = if picker.items.len() > visible && line == picker.thumb(height) {
            '█'
        } else {
            '│'
        };
        rows.push(inner(
            &format!("{mark} {label}{suffix}"),
            width,
            if index == picker.selected {
                SELECTED
            } else {
                TEXT
            },
            edge,
        ));
    }
    rows
}

pub(crate) fn picker_panel(
    picker: &Picker,
    width: usize,
    height: usize,
    suggestions: bool,
) -> Vec<Row> {
    let hint = format!(
        "{}   {}/{}",
        if suggestions {
            "↑↓ choose · Tab complete · Enter run · Esc dismiss"
        } else {
            "↑↓ move · Enter select · Esc cancel"
        },
        picker.selected + 1,
        picker.items.len()
    );
    panel(
        &picker.title,
        choices(picker, width, height.saturating_sub(3)),
        &hint,
        width,
    )
}

pub(crate) struct Modal {
    title: String,
    details: String,
    picker: Option<Picker>,
    text: Option<String>,
    placeholder: String,
    secret: bool,
    confirm: bool,
    approval: bool,
    deadline: Option<Instant>,
    details_scroll: usize,
    detail_rows: usize,
    detail_total: usize,
    choice_rows: usize,
    reviewed: bool,
    review_notice: bool,
    utf8: Vec<u8>,
    pub(crate) cursor: Option<(usize, usize)>,
}

impl Modal {
    pub(crate) fn input(title: &str, text: String, placeholder: String, secret: bool) -> Self {
        Self {
            title: title.into(),
            details: String::new(),
            picker: None,
            text: Some(text),
            placeholder,
            secret,
            confirm: false,
            approval: false,
            deadline: None,
            details_scroll: 0,
            detail_rows: 0,
            detail_total: 0,
            choice_rows: 1,
            reviewed: false,
            review_notice: false,
            utf8: Vec::new(),
            cursor: None,
        }
    }
    pub(crate) fn from_rpc(event: &Value) -> Option<Self> {
        let raw_title = event["title"].as_str().unwrap_or("Pi request");
        if raw_title.len() > 16_384 {
            return None;
        }
        let (title, details) = raw_title.split_once('\n').unwrap_or((raw_title, ""));
        let mut modal = Self::input(title, String::new(), String::new(), false);
        // A long one-line title must not silently hide approval details in
        // the clipped border. Keep its full text in the scrollable body too.
        modal.details = if details.is_empty() && title.chars().count() > 60 {
            title.to_owned()
        } else {
            details.to_owned()
        };
        modal.approval = raw_title.starts_with("Approval needed:");
        modal.deadline = event["timeout"]
            .as_u64()
            .map(|ms| Instant::now() + Duration::from_millis(ms.min(86_400_000)));
        if modal.approval && modal.deadline.is_none() {
            modal.deadline = Some(Instant::now() + Duration::from_secs(60));
        }
        match event["method"].as_str()? {
            "select" => {
                let options = event["options"].as_array()?;
                if options.is_empty() || options.len() > 1000 {
                    return None;
                }
                let items = options
                    .iter()
                    .map(|option| option.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()?;
                if items.iter().any(|item| item.len() > 8192) {
                    return None;
                }
                let selected = items
                    .iter()
                    .position(|item| matches!(item.as_str(), "Deny" | "Block" | "Cancel" | "No"))
                    .unwrap_or(0);
                let mut picker = Picker::new(title.into(), items, Some(selected), 5);
                picker.current = None;
                modal.picker = Some(picker);
                modal.text = None;
            }
            "confirm" => {
                modal.confirm = true;
                if !modal.details.is_empty() {
                    modal.details.push('\n');
                }
                modal
                    .details
                    .push_str(event["message"].as_str().unwrap_or(""));
                modal.picker = Some(Picker::new(
                    title.into(),
                    vec!["Cancel".into(), "Confirm".into()],
                    None,
                    2,
                ));
                modal.text = None;
            }
            "input" | "editor" => {
                let text = event["prefill"].as_str().unwrap_or("");
                if text.len() > 8192 {
                    return None;
                }
                modal.text = Some(text.into());
                modal.placeholder = event["placeholder"].as_str().unwrap_or("").into();
            }
            _ => return None,
        }
        if modal.details.len() > 16_384 {
            return None;
        }
        Some(modal)
    }
    pub(crate) fn expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
    pub(crate) fn navigate(&mut self, key: Navigation) {
        if let Navigation::ScrollPage(direction) = key {
            if self.detail_total > self.detail_rows {
                self.details_scroll = self
                    .details_scroll
                    .saturating_add_signed(-(direction as isize) * self.detail_rows.max(1) as isize)
                    .min(self.detail_total.saturating_sub(self.detail_rows));
            } else if let Some(picker) = &mut self.picker {
                picker.page(-(direction as isize), self.choice_rows);
            }
            return;
        }
        if let Some(picker) = self.picker.as_mut() {
            match key {
                Navigation::Up | Navigation::ScrollLines(3) | Navigation::MouseScroll(3, _) => {
                    picker.move_by(-1, self.choice_rows)
                }
                Navigation::Down | Navigation::ScrollLines(-3) | Navigation::MouseScroll(-3, _) => {
                    picker.move_by(1, self.choice_rows)
                }
                Navigation::Home => {
                    picker.move_by(-(picker.items.len() as isize), self.choice_rows)
                }
                Navigation::End => picker.move_by(picker.items.len() as isize, self.choice_rows),
                _ => {}
            }
        }
    }
    pub(crate) fn key(&mut self, byte: u8) -> Option<Value> {
        if self.expired() {
            return Some(json!({"cancelled":true}));
        }
        match byte {
            3 | 4 => return Some(json!({"cancelled":true})),
            b'\r' | b'\n' => {
                if let Some(picker) = &self.picker {
                    let item = &picker.items[picker.selected];
                    if self.approval
                        && matches!(item.as_str(), "Allow" | "Always Allow")
                        && !self.reviewed
                    {
                        self.review_notice = true;
                        return None;
                    }
                    return Some(if self.confirm {
                        json!({"confirmed":picker.selected == 1})
                    } else {
                        json!({"value":item})
                    });
                }
                return Some(json!({"value":self.text.as_deref().unwrap_or("")}));
            }
            8 | 127 => {
                self.utf8.clear();
                if let Some(text) = &mut self.text {
                    text.pop();
                }
            }
            32..=126 | 128..=255 => {
                if let Some(text) = &mut self.text {
                    if text.len() < 8192 {
                        self.utf8.push(byte);
                        match std::str::from_utf8(&self.utf8) {
                            Ok(value) => {
                                text.push_str(value);
                                self.utf8.clear();
                            }
                            Err(error) if error.error_len().is_some() => self.utf8.clear(),
                            Err(_) => {}
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }
    pub(crate) fn rows(&mut self, width: usize, height: usize) -> Vec<Row> {
        let body_height = height.saturating_sub(3).max(1);
        let details = wrap_details(&self.details, width.saturating_sub(4).max(1));
        self.detail_total = details.len();
        let reserve = usize::from(!details.is_empty() && body_height > 1);
        self.choice_rows = self.picker.as_ref().map_or(1, |picker| {
            picker.visible(body_height.saturating_sub(reserve).min(5))
        });
        self.detail_rows = details
            .len()
            .min(body_height.saturating_sub(self.choice_rows));
        self.details_scroll = self
            .details_scroll
            .min(details.len().saturating_sub(self.detail_rows));
        self.reviewed |= self.details_scroll + self.detail_rows >= details.len();
        let mut body = details
            .iter()
            .skip(self.details_scroll)
            .take(self.detail_rows)
            .map(|text| inner(text, width, TEXT, '│'))
            .collect::<Vec<_>>();
        self.cursor = None;
        if let Some(picker) = &mut self.picker {
            picker.move_by(0, self.choice_rows);
            body.extend(choices(picker, width, self.choice_rows));
        } else if let Some(text) = &self.text {
            let visible = if self.secret {
                "•".repeat(text.chars().count())
            } else {
                text.replace(['\n', '\r'], " ")
            };
            let visible: String = visible
                .chars()
                .rev()
                .take(width.saturating_sub(7))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let (shown, style) = if text.is_empty() {
                (clip(&self.placeholder, width.saturating_sub(7)), BORDER)
            } else {
                (visible.clone(), TEXT)
            };
            self.cursor = Some((1 + body.len(), 4 + visible.chars().count()));
            body.push(inner(&format!("❯ {shown}"), width, style, '│'));
        }
        let hint = if self.review_notice && !self.reviewed {
            "PgDn: review remaining details before Allow".into()
        } else if self.detail_total > self.detail_rows {
            format!(
                "PgUp/PgDn details {}/{} · ↑↓ · Enter · Esc",
                self.details_scroll + self.detail_rows,
                self.detail_total
            )
        } else if self.picker.is_some() {
            "↑↓ move · Enter select · Esc cancel".into()
        } else {
            "Enter submit · Esc cancel".into()
        };
        panel(&self.title, body, &hint, width)
    }
}

fn wrap_details(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for line in text.lines() {
        let clean: Vec<char> = line
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect();
        if clean.is_empty() {
            rows.push(String::new());
        } else {
            rows.extend(clean.chunks(width).map(|chunk| chunk.iter().collect()));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_defaults_to_deny_and_long_commands_require_review() {
        let event = json!({"method":"select","title":format!("Approval needed: command\n{}", "rm -rf example ".repeat(30)),"options":["Deny","Allow","Always Allow"]});
        let mut modal = Modal::from_rpc(&event).unwrap();
        let rows = modal.rows(40, 10);
        assert!(rows.len() <= 10);
        assert!(rows.iter().all(|row| row.text.chars().count() <= 40));
        assert_eq!(modal.key(b'\r').unwrap()["value"], "Deny");
        modal.navigate(Navigation::Down);
        assert!(modal.key(b'\r').is_none());
        for _ in 0..50 {
            modal.navigate(Navigation::ScrollPage(-1));
            modal.rows(40, 10);
        }
        assert_eq!(modal.key(b'\r').unwrap()["value"], "Allow");
    }
    #[test]
    fn confirm_timeout_and_masked_input_use_the_same_chrome() {
        let mut modal =
            Modal::from_rpc(&json!({"method":"confirm","title":"Confirm?","message":"Details"}))
                .unwrap();
        assert_eq!(modal.key(b'\r').unwrap()["confirmed"], false);
        let mut modal =
            Modal::from_rpc(&json!({"method":"select","options":["Deny","Allow"],"timeout":0}))
                .unwrap();
        assert_eq!(modal.key(b'\r').unwrap()["cancelled"], true);
        let mut input = Modal::input("Secret", "private".into(), String::new(), true);
        let rows = input.rows(40, 10);
        assert!(rows[0].text.contains("╭─ ✿ Secret"));
        assert!(!rows.iter().any(|row| row.text.contains("private")));
        assert_eq!(input.key(b'\r').unwrap()["value"], "private");
    }
}
