//! Display-only activity for one portion of a Pi run. Routine reads and bash
//! calls contribute to one compact summary; edits and failures stay reviewable.
use std::collections::HashMap;

const EXPLORE_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

#[derive(Default)]
pub(crate) struct Timeline {
    entries: Vec<Entry>,
    calls: HashMap<String, Call>,
    reads: usize,
    commands: usize,
    latest_read: Option<String>,
    active_reads: usize,
    interrupted_reads: bool,
    exploring: bool,
    exploration_ended: bool,
    explore_frame: usize,
}

enum Call {
    Read(String),
    Bash,
    Entry(usize),
}

enum Entry {
    Tool {
        name: String,
        path: String,
        running: bool,
        failed: bool,
        diff: Vec<String>,
        omitted: usize,
    },
    Failure {
        name: String,
        path: String,
    },
}

fn safe(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(90)
        .collect()
}

impl Timeline {
    pub(crate) fn tool_start(&mut self, id: &str, name: &str, path: &str) {
        if self.calls.contains_key(id) {
            return;
        }
        let call = match name {
            "read" => {
                let path = safe(path);
                self.exploring = true;
                self.exploration_ended = false;
                self.latest_read = Some(path.clone());
                self.active_reads += 1;
                Call::Read(path)
            }
            "bash" => Call::Bash,
            _ => {
                if matches!(name, "edit" | "write") {
                    self.exploration_ended = true;
                }
                let index = self.entries.len();
                self.entries.push(Entry::Tool {
                    name: safe(name),
                    path: safe(path),
                    running: true,
                    failed: false,
                    diff: Vec::new(),
                    omitted: 0,
                });
                Call::Entry(index)
            }
        };
        self.calls.insert(id.to_owned(), call);
    }

    pub(crate) fn tool_end(
        &mut self,
        id: &str,
        name: &str,
        path: &str,
        failed: bool,
        diff: Option<&str>,
    ) {
        let matched_start = self.calls.contains_key(id);
        let call = self.calls.remove(id).unwrap_or_else(|| match name {
            "read" => {
                let path = safe(path);
                self.exploring = true;
                self.exploration_ended = false;
                self.latest_read = Some(path.clone());
                Call::Read(path)
            }
            "bash" => Call::Bash,
            _ => {
                if matches!(name, "edit" | "write") {
                    self.exploration_ended = true;
                }
                let index = self.entries.len();
                self.entries.push(Entry::Tool {
                    name: safe(name),
                    path: safe(path),
                    running: true,
                    failed: false,
                    diff: Vec::new(),
                    omitted: 0,
                });
                Call::Entry(index)
            }
        });
        match call {
            Call::Read(path) => {
                if matched_start {
                    self.active_reads = self.active_reads.saturating_sub(1);
                }
                if failed {
                    self.entries.push(Entry::Failure {
                        name: "read".into(),
                        path,
                    });
                } else {
                    self.reads += 1;
                }
            }
            Call::Bash => {
                if failed {
                    self.entries.push(Entry::Failure {
                        name: "bash".into(),
                        path: "command hidden".into(),
                    });
                } else {
                    self.commands += 1;
                }
            }
            Call::Entry(index) => {
                if let Entry::Tool {
                    running,
                    failed: error,
                    diff: lines,
                    omitted,
                    ..
                } = &mut self.entries[index]
                {
                    *running = false;
                    *error = failed;
                    if name == "edit" && !failed {
                        if let Some(diff) = diff {
                            for (index, line) in diff.lines().enumerate() {
                                if index < 40 {
                                    lines.push(
                                        line.chars()
                                            .filter(|ch| !ch.is_control())
                                            .take(180)
                                            .collect(),
                                    );
                                } else {
                                    *omitted += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn is_exploring(&self) -> bool {
        self.latest_read.is_some()
            && self.exploring
            && !self.interrupted_reads
            && (!self.exploration_ended || self.active_reads > 0)
    }

    /// Rotate a single fixed-width glyph without changing any tool state.
    pub(crate) fn advance_explore(&mut self) -> bool {
        if !self.is_exploring() {
            return false;
        }
        self.explore_frame = (self.explore_frame + 1) % EXPLORE_FRAMES.len();
        true
    }

    /// An unfinished read is never reported as successfully completed.
    pub(crate) fn finish(&mut self) {
        self.exploration_ended = true;
        if self.active_reads > 0 {
            self.interrupted_reads = true;
            self.active_reads = 0;
        }
    }

    pub(crate) fn lines(&self) -> String {
        let mut lines = Vec::new();
        if let Some(path) = &self.latest_read {
            let status = if self.interrupted_reads {
                "Explore stopped".to_owned()
            } else if self.is_exploring() {
                format!("{} Exploring", EXPLORE_FRAMES[self.explore_frame])
            } else {
                "✓ Explored".to_owned()
            };
            lines.push(format!("  · {status} · latest: {path}"));
        }
        if self.reads > 0 {
            lines.push(format!(
                "  · ✓ Read · {} {}",
                self.reads,
                if self.reads == 1 { "file" } else { "files" }
            ));
        }
        if self.commands > 0 {
            lines.push(format!(
                "  · ✓ Bash · {} {}",
                self.commands,
                if self.commands == 1 {
                    "command"
                } else {
                    "commands"
                }
            ));
        }
        for entry in &self.entries {
            match entry {
                Entry::Failure { name, path } => lines.push(format!("  · ✗ {name} · {path}")),
                Entry::Tool {
                    name,
                    path,
                    running,
                    failed,
                    diff,
                    omitted,
                } => {
                    let symbol = if *running {
                        "◌"
                    } else if *failed {
                        "✗"
                    } else {
                        "✓"
                    };
                    lines.push(format!(
                        "  · {symbol} {name} · {path}{}",
                        if *running { " …" } else { "" }
                    ));
                    if !diff.is_empty() {
                        lines.push(format!("  · diff · {path}"));
                        lines.extend(diff.iter().map(|line| format!("    {line}")));
                        if *omitted > 0 {
                            lines.push(format!("  · … {omitted} more diff lines"));
                        }
                    }
                }
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explore_spinner_only_moves_during_an_open_exploration_phase() {
        let mut timeline = Timeline::default();
        assert!(!timeline.advance_explore());
        timeline.tool_start("r", "read", "src/first.rs");
        timeline.tool_end("r", "read", "src/first.rs", false, None);
        assert!(timeline
            .lines()
            .contains("◐ Exploring · latest: src/first.rs"));
        assert!(timeline.advance_explore());
        assert!(timeline
            .lines()
            .contains("◓ Exploring · latest: src/first.rs"));
        assert!(timeline.lines().contains("✓ Read · 1 file"));
        timeline.tool_start("e", "edit", "src/first.rs");
        assert!(!timeline.advance_explore());
        assert!(timeline
            .lines()
            .contains("✓ Explored · latest: src/first.rs"));
        timeline.tool_start("r2", "read", "src/second.rs");
        assert!(timeline.advance_explore());
        timeline.finish();
        assert!(!timeline.advance_explore());
        assert!(timeline
            .lines()
            .contains("Explore stopped · latest: src/second.rs"));
        assert!(!timeline.lines().contains("✓ Read · 2 files"));
    }

    #[test]
    fn latest_read_updates_in_place_above_completed_counts() {
        let mut timeline = Timeline::default();
        timeline.tool_start("r1", "read", "src/first.rs");
        assert!(timeline
            .lines()
            .contains("◐ Exploring · latest: src/first.rs"));
        assert!(!timeline.lines().contains("✓ Read"));
        timeline.tool_end("r1", "read", "src/first.rs", false, None);
        assert!(timeline
            .lines()
            .contains("◐ Exploring · latest: src/first.rs\n  · ✓ Read · 1 file"));
        timeline.tool_start("r2", "read", "src/second.rs");
        assert!(timeline
            .lines()
            .contains("◐ Exploring · latest: src/second.rs\n  · ✓ Read · 1 file"));
        timeline.tool_end("r2", "read", "src/second.rs", false, None);
        assert!(timeline
            .lines()
            .contains("◐ Exploring · latest: src/second.rs\n  · ✓ Read · 2 files"));
        timeline.tool_start("r3", "read", "src/unfinished.rs");
        timeline.tool_end("unmatched", "read", "src/extra.rs", false, None);
        assert!(timeline
            .lines()
            .contains("◐ Exploring · latest: src/extra.rs"));
        timeline.finish();
        assert!(timeline
            .lines()
            .contains("Explore stopped · latest: src/extra.rs\n  · ✓ Read · 3 files"));
        assert!(!timeline.lines().contains("✓ Read · 4 files"));
        let mut moved_on = Timeline::default();
        moved_on.tool_start("r", "read", "src/main.rs");
        moved_on.tool_end("r", "read", "src/main.rs", false, None);
        moved_on.tool_start("e", "edit", "src/main.rs");
        assert!(moved_on
            .lines()
            .contains("✓ Explored · latest: src/main.rs\n  · ✓ Read · 1 file"));
    }

    #[test]
    fn routine_work_collapses_but_edits_and_failures_remain() {
        let mut timeline = Timeline::default();
        assert!(timeline.lines().is_empty());
        for id in 1..=27 {
            let path = format!("src/file_{id}.rs");
            timeline.tool_start(&format!("r{id}"), "read", &path);
            timeline.tool_end(&format!("r{id}"), "read", &path, false, None);
        }
        for id in 1..=4 {
            timeline.tool_start(&format!("b{id}"), "bash", "private command");
            timeline.tool_end(&format!("b{id}"), "bash", "private command", false, None);
        }
        for id in 1..=3 {
            let path = format!("change_{id}.rs");
            timeline.tool_start(&format!("e{id}"), "edit", &path);
            timeline.tool_end(
                &format!("e{id}"),
                "edit",
                &path,
                false,
                Some(&format!("-old_{id}\n+new_{id}")),
            );
        }
        timeline.tool_end("missing", "read", "missing.txt", true, None);
        timeline.tool_end("failed", "bash", "private command", true, None);
        timeline.tool_end(
            "bad-edit",
            "edit",
            "failed.rs",
            true,
            Some("+must-not-show"),
        );
        let text = timeline.lines();
        assert!(text.contains(
            "✓ Explored · latest: missing.txt\n  · ✓ Read · 27 files\n  · ✓ Bash · 4 commands"
        ));
        assert_eq!(text.matches("diff ·").count(), 3);
        assert!(text.contains("✗ read · missing.txt"));
        assert!(text.contains("✗ bash · command hidden"));
        assert!(text.contains("✗ edit · failed.rs"));
        assert!(!text.contains("private command"));
        assert!(!text.contains("thinking") && !text.contains("must-not-show"));
    }
}
