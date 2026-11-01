//! Inline renderer. Owns no fullscreen composer, transcript layout or painter.
use super::{style, tools, welcome};
use crate::ui::{display::ScrollDisplay, mode::Mode, terminal, tool, welcome::Context};
use serde_json::Value;
use std::io::{self, Write};

pub(crate) struct Screen {
    out: io::Stdout,
    terminal: bool,
    capable: bool,
    color: bool,
    active: bool,
    suspended: bool,
    diff: style::Diff,
    tools: tools::Tools,
    speaker: bool,
    waiting: Option<bool>,
    workspace: String,
    model: String,
    has_credentials: bool,
}
impl Screen {
    pub(crate) fn new(terminal: bool, capable: bool, color: bool) -> io::Result<Self> {
        let mut screen = Self {
            out: io::stdout(),
            terminal,
            capable,
            color,
            active: false,
            suspended: true,
            diff: style::Diff::default(),
            tools: tools::Tools::default(),
            speaker: false,
            waiting: None,
            workspace: std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|s| clean(&s.to_string_lossy())))
                .unwrap_or_else(|| "workspace".into()),
            model: "no active model".into(),
            has_credentials: false,
        };
        screen.resume()?;
        Ok(screen)
    }
    pub(crate) fn status(&mut self, state: &Value) {
        self.has_credentials = state["hasCredentials"]
            .as_bool()
            .unwrap_or_else(|| state["model"].is_object());
        let provider = state["model"]["provider"].as_str().unwrap_or("");
        let model = state["model"]["id"].as_str().unwrap_or("no active model");
        self.model = if provider.is_empty() {
            clean(model)
        } else {
            format!("{}/{}", clean(provider), clean(model))
        };
    }
    pub(crate) fn welcome(&mut self) -> io::Result<()> {
        if !self.terminal {
            return Ok(());
        }
        let text = welcome::render(
            Context {
                workspace: &self.workspace,
                model: &self.model,
                mode: Mode::Cli,
                has_credentials: self.has_credentials,
            },
            self.active,
        );
        write!(self, "\n{text}\n")?;
        self.flush()
    }
    pub(crate) fn input_style(&self) -> Option<bool> {
        (self.terminal && self.capable).then_some(self.active)
    }
    pub(crate) fn styled(&self) -> bool {
        self.active
    }
    pub(crate) fn suspend(&mut self) -> io::Result<()> {
        if self.suspended {
            return Ok(());
        }
        self.end_wait()?;
        if self.terminal && self.capable {
            self.out.write_all(b"\x1b[?2004l")?;
        }
        if self.active {
            self.end_assistant_text()?;
            self.out.write_all(b"\x1b[0m")?;
            self.active = false;
            self.out.flush()?;
        }
        self.suspended = true;
        Ok(())
    }
    pub(crate) fn resume(&mut self) -> io::Result<()> {
        self.suspended = false;
        if self.terminal && self.color && !self.active {
            write!(self.out, "{}\x1b[J", style::BASE)?;
            self.active = true;
            self.out.flush()?;
        }
        Ok(())
    }
    fn end_wait(&mut self) -> io::Result<bool> {
        match self.waiting.take() {
            Some(rewrite) => {
                if rewrite {
                    self.write_all(b"\r\x1b[2K")?;
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }
}
impl ScrollDisplay for Screen {
    fn assistant_start(&mut self) -> io::Result<bool> {
        if !self.terminal {
            return Ok(false);
        }
        let was_waiting = self.end_wait()?;
        let pending = self.tools.flush(self.active, terminal::size().0);
        self.write_all(pending.as_bytes())?;
        if !self.speaker {
            self.speaker = true;
            if self.active {
                write!(self, "\n\x1b[1;38;2;225;80;130mhibi\x1b[0m\n")?;
            } else {
                write!(self, "\nhibi\n")?;
            }
        } else if !was_waiting {
            writeln!(self)?;
        }
        Ok(true)
    }
    fn assistant_text(&mut self, text: &str) -> io::Result<()> {
        if self.active {
            let text = self.diff.push(text);
            self.write_all(text.as_bytes())
        } else {
            self.write_all(text.as_bytes())
        }
    }
    fn end_assistant_text(&mut self) -> io::Result<()> {
        let tail = self.diff.finish();
        self.write_all(tail.as_bytes())
    }
    fn end_turn(&mut self) -> io::Result<()> {
        self.end_wait()?;
        self.end_assistant_text()?;
        let pending = self.tools.flush(self.active, terminal::size().0);
        self.write_all(pending.as_bytes())
    }
    fn diff_line(&mut self, line: &str) -> io::Result<()> {
        let text = if self.active {
            style::patch_line(line)
        } else {
            line.to_owned()
        };
        writeln!(self, "    {text}")
    }
    fn start_command(&mut self, label: &str) -> io::Result<()> {
        if !self.terminal {
            return Ok(());
        }
        self.waiting = Some(self.active);
        if self.active {
            write!(
                self,
                "\x1b[38;2;155;155;155m{label} · Esc to stop{}",
                style::BASE
            )?;
        } else {
            writeln!(self, "{label} · Esc to stop")?;
        }
        self.flush()
    }
    fn start_work(&mut self) -> io::Result<()> {
        self.speaker = false;
        if self.terminal {
            self.assistant_start()?;
            self.waiting = Some(self.active);
            let status = tools::waiting(terminal::size().0);
            if self.active {
                write!(self, "\x1b[38;2;155;155;155m{status}{}", style::BASE)?;
            } else {
                writeln!(self, "{status}")?;
            }
            self.flush()?;
        }
        Ok(())
    }
    fn stop_work(&mut self) -> io::Result<()> {
        self.end_turn()
    }
    fn thinking_end(&mut self) -> io::Result<bool> {
        Ok(self.terminal)
    }
    fn tool_start(&mut self, id: &str, call: &tool::Call) -> io::Result<bool> {
        if !self.terminal {
            return Ok(false);
        }
        self.end_wait()?;
        if !self.speaker {
            self.assistant_start()?;
        }
        let row = self.tools.start(id, call, self.active, terminal::size().0);
        self.write_all(row.as_bytes())?;
        Ok(true)
    }
    fn tool_end(
        &mut self,
        id: &str,
        call: &tool::Call,
        result: &tool::Outcome<'_>,
    ) -> io::Result<bool> {
        if !self.terminal {
            return Ok(false);
        }
        let row = self
            .tools
            .end(id, call, result, self.active, terminal::size().0);
        self.write_all(row.as_bytes())?;
        Ok(true)
    }
}
impl Write for Screen {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.terminal {
            return self.out.write(buf);
        }
        let styled = if self.active {
            style::background(buf)
        } else {
            std::borrow::Cow::Borrowed(buf)
        };
        let bytes = styled.as_ref();
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' {
                self.out.write_all(&bytes[start..index])?;
                if index == 0 || bytes[index - 1] != b'\r' {
                    self.out.write_all(b"\r")?;
                }
                self.out.write_all(b"\n")?;
                start = index + 1;
            }
        }
        self.out.write_all(&bytes[start..])?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
