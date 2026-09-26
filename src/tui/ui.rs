use serde_json::Value;
use std::env;
use std::io::{self, Write};

fn safe_label(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(50).collect()
}

/// Only decorate the human-facing terminal. Piped output remains script-friendly.
pub(crate) struct Ui {
    pub(crate) interactive: bool,
    color: bool,
}

impl Ui {
    pub(crate) fn new(interactive: bool) -> Self {
        let color = interactive
            && env::var_os("NO_COLOR").is_none()
            && env::var("TERM").is_ok_and(|term| term != "dumb");
        Self { interactive, color }
    }

    fn style(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    pub(crate) fn header(&self, out: &mut impl Write) -> io::Result<()> {
        if !self.interactive {
            return Ok(());
        }
        let workspace = env::current_dir()
            .ok()
            .and_then(|cwd| cwd.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "workspace".into());
        let workspace = safe_label(&workspace);
        writeln!(
            out,
            "\n  {}  {}",
            self.style("1;38;2;236;74;125", "✿ hibiscus"),
            self.style("2", &workspace)
        )?;
        writeln!(
            out,
            "  {}\n",
            self.style("2", "Enter to send · Esc to stop · /help for commands")
        )
    }

    pub(crate) fn prompt(&self, out: &mut impl Write) -> io::Result<()> {
        if self.interactive {
            write!(out, "{} ", self.style("1;38;2;255;155;187", "❯"))?;
            out.flush()?;
        }
        Ok(())
    }

    pub(crate) fn user(&self, out: &mut impl Write) -> io::Result<()> {
        if self.interactive {
            write!(out, "{} ", self.style("1;38;2;255;155;187", "you ›"))?;
        }
        Ok(())
    }

    pub(crate) fn assistant(&self, out: &mut impl Write) -> io::Result<()> {
        if self.interactive {
            write!(out, "{} ", self.style("1;38;2;236;74;125", "hibi ›"))?;
            out.flush()?;
        }
        Ok(())
    }

    pub(crate) fn info(&self, out: &mut impl Write, message: &str) -> io::Result<()> {
        if self.interactive {
            writeln!(out, "  {} {message}", self.style("2", "·"))
        } else {
            writeln!(out, "{message}")
        }
    }

    pub(crate) fn session(&self, out: &mut impl Write, state: &Value) -> io::Result<()> {
        if !self.interactive {
            return Ok(());
        }
        let model = state["model"]["id"].as_str().unwrap_or("no model");
        let provider = state["model"]["provider"]
            .as_str()
            .unwrap_or("unconfigured");
        let name = state["sessionName"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(safe_label)
            .or_else(|| {
                state["sessionId"]
                    .as_str()
                    .map(|id| safe_label(&id.chars().take(8).collect::<String>()))
            })
            .unwrap_or_else(|| "new session".into());
        self.info(
            out,
            &format!(
                "{}/{}  ·  {}",
                safe_label(provider),
                safe_label(model),
                name
            ),
        )
    }

    pub(crate) fn help(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "\n  Chat        /new  /continue  /sessions  /quit")?;
        writeln!(
            out,
            "  Models      /models  /thinking [level]  (↑↓ / wheel, Enter, Esc)"
        )?;
        writeln!(
            out,
            "  Context     /compact [summary instructions]  (Pi model call)"
        )?;
        writeln!(out, "  Account     /login  (Codex here; others in Pi)")?;
        writeln!(
            out,
            "              /logout  (remove stored credentials here)"
        )?;
        writeln!(
            out,
            "  Recovery    /restore  (review failed prompt)  /reconnect"
        )?;
        writeln!(
            out,
            "  Keys        Enter send  ·  Esc stop  ·  Ctrl+D exit\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_ui_has_no_control_codes_or_banner() {
        let ui = Ui::new(false);
        let mut out = Vec::new();
        ui.header(&mut out).unwrap();
        ui.prompt(&mut out).unwrap();
        ui.session(&mut out, &json!({"model":{"provider":"test","id":"small"}}))
            .unwrap();
        assert!(out.is_empty());
        ui.info(&mut out, "Started new session.").unwrap();
        assert_eq!(out, b"Started new session.\n");
    }

    #[test]
    fn interactive_status_uses_active_session_only() {
        let ui = Ui {
            interactive: true,
            color: false,
        };
        let mut out = Vec::new();
        ui.session(
            &mut out,
            &json!({"model":{"provider":"demo","id":"one"},"sessionName":"Current"}),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "  · demo/one  ·  Current\n"
        );
    }

    #[test]
    fn session_label_cannot_inject_terminal_controls() {
        let ui = Ui {
            interactive: true,
            color: false,
        };
        let mut out = Vec::new();
        ui.session(&mut out, &json!({"sessionName":"bad\u{1b}[2J\nname"}))
            .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "  · unconfigured/no model  ·  bad[2Jname\n"
        );
    }
}
