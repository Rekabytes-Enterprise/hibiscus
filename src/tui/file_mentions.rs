//! Display-only @path completion. Pi receives the selected path as ordinary
//! prompt text; unlike Pi's CLI @file arguments, RPC does not expand it.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Prefix {
    pub start: usize,
    pub query: String,
    pub quoted: bool,
}

/// Only a token beginning with @ at a word boundary activates completion.
/// The cursor may be in the middle of the draft; never consume following text.
pub(super) fn prefix(text: &str, cursor: usize) -> Option<Prefix> {
    let before = text.get(..cursor)?;
    let line = before.rsplit('\n').next().unwrap_or(before);
    let base = before.len() - line.len();
    let mut start = 0;
    let mut quoted = false;
    for (index, ch) in line.char_indices() {
        if ch == '@'
            && (index == 0
                || line[..index]
                    .chars()
                    .last()
                    .is_some_and(|prev| prev.is_whitespace() || matches!(prev, '=' | '(')))
        {
            start = index;
            quoted = line[index..].starts_with("@\"");
        } else if ch.is_whitespace() && !quoted {
            start = line.len();
        } else if ch == '"' && quoted && index > start + 1 {
            start = line.len();
            quoted = false;
        }
    }
    let token = line.get(start..)?;
    let query = token.strip_prefix(if quoted { "@\"" } else { "@" })?;
    if query.chars().any(char::is_control) || (!quoted && query.contains([' ', '\t', '"'])) {
        return None;
    }
    Some(Prefix {
        start: base + start,
        query: query.to_owned(),
        quoted,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub path: String,
    pub directory: bool,
}
impl Entry {
    pub fn display(&self) -> String {
        if self.directory {
            format!("{}/", self.path.trim_end_matches('/'))
        } else {
            self.path.clone()
        }
    }
    pub fn replacement(&self, quoted: bool) -> (String, usize) {
        let path = self.display();
        let quoted = quoted || path.chars().any(char::is_whitespace);
        if quoted {
            let value = format!("@\"{path}\"{}", if self.directory { "" } else { " " });
            let cursor = if self.directory {
                value.len() - 1
            } else {
                value.len()
            };
            (value, cursor)
        } else {
            let value = format!("@{path}{}", if self.directory { "" } else { " " });
            let cursor = value.len();
            (value, cursor)
        }
    }
}

pub(super) struct Search {
    receiver: Receiver<Vec<Entry>>,
    cancelled: Arc<AtomicBool>,
}
impl Search {
    pub fn start(cwd: PathBuf, query: String) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let entries = search(&cwd, &query, &stop);
            if !stop.load(Ordering::Relaxed) {
                let _ = sender.send(entries);
            }
        });
        Self {
            receiver,
            cancelled,
        }
    }
    pub fn poll(&self) -> Option<Vec<Entry>> {
        self.receiver.try_recv().ok()
    }
}
impl Drop for Search {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn scope<'a>(cwd: &Path, query: &'a str) -> (PathBuf, &'a str, String) {
    if let Some((folder, term)) = query.rsplit_once('/') {
        let root = if folder.is_empty() && query.starts_with('/') {
            PathBuf::from("/")
        } else if folder == "~" || folder.starts_with("~/") {
            let Some(home) = std::env::var_os("HOME") else {
                return (cwd.to_owned(), query, String::new());
            };
            PathBuf::from(home).join(folder.trim_start_matches('~').trim_start_matches('/'))
        } else {
            cwd.join(folder)
        };
        if root.is_dir() {
            return (root, term, format!("{folder}/"));
        }
    }
    (cwd.to_owned(), query, String::new())
}

fn search(cwd: &Path, query: &str, stop: &AtomicBool) -> Vec<Entry> {
    if query.len() > 512 || query.contains(['\0', '\n']) {
        return Vec::new();
    }
    let (root, term, display_base) = scope(cwd, query);
    let mut command = Command::new("fd");
    command.current_dir(&root).args([
        "--max-results",
        "100",
        "--type",
        "f",
        "--type",
        "d",
        "--hidden",
        "--exclude",
        ".git",
        "--ignore-case",
        "--fixed-strings",
        "--color",
        "never",
        "--print0",
    ]);
    if term.contains('/') {
        command.arg("--full-path");
    }
    if !term.is_empty() {
        command.arg("--").arg(term);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return local_directory(cwd, query);
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Vec::new();
    };
    let (sender, reader) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(64 * 1024).read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    let deadline = Instant::now() + Duration::from_millis(700);
    let status = loop {
        if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => break None,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    };
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    if !status.is_some_and(|status| status.success()) || stop.load(Ordering::Relaxed) {
        return Vec::new();
    }
    let Ok(bytes) = reader.recv_timeout(Duration::from_millis(100)) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = bytes
        .split(|byte| *byte == 0)
        .filter_map(|item| {
            let path = std::str::from_utf8(item).ok()?.trim_start_matches("./");
            if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
                return None;
            }
            let directory = path.ends_with('/');
            let path = format!("{display_base}{}", path.trim_end_matches('/'));
            Some(Entry { path, directory })
        })
        .take(100)
        .collect();
    entries.sort_by(|a, b| {
        let score = |entry: &Entry| {
            let name = entry
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&entry.path)
                .to_lowercase();
            let query = term.to_lowercase();
            if name == query {
                0
            } else if name.starts_with(&query) {
                1
            } else if name.contains(&query) {
                2
            } else {
                3
            }
        };
        score(a)
            .cmp(&score(b))
            .then_with(|| b.directory.cmp(&a.directory))
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    entries.truncate(20);
    entries
}

// A small direct-path fallback if the optional fd executable is unavailable.
// Keep it off the UI thread, just like recursive search.
fn local_directory(cwd: &Path, query: &str) -> Vec<Entry> {
    let (folder, fragment, display_base) = scope(cwd, query);
    let Ok(read) = fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for item in read.take(256).flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" || !name.to_lowercase().starts_with(&fragment.to_lowercase()) {
            continue;
        }
        let path = format!("{display_base}{name}");
        let directory = item.file_type().is_ok_and(|kind| kind.is_dir());
        entries.push(Entry { path, directory });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.path.cmp(&b.path))
    });
    entries.truncate(20);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_directory_and_fallback_keep_display_paths() {
        let root = std::env::temp_dir().join(format!(
            "hibiscus-mentions-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "test").unwrap();
        let (base, term, display) = scope(&root, "src/ma");
        assert_eq!(base, root.join("src"));
        assert_eq!((term, display.as_str()), ("ma", "src/"));
        assert_eq!(
            local_directory(&root, "src/ma"),
            vec![Entry {
                path: "src/main.rs".into(),
                directory: false
            }]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mention_starts_at_cursor_boundary_and_quotes_space_names() {
        assert_eq!(
            prefix("explain @rea later", 12),
            Some(Prefix {
                start: 8,
                query: "rea".into(),
                quoted: false
            })
        );
        assert_eq!(prefix("a@b", 3), None);
        assert_eq!(
            prefix("use @\"my file", 13),
            Some(Prefix {
                start: 4,
                query: "my file".into(),
                quoted: true
            })
        );
        assert_eq!(
            prefix("x @one\ny @src/", 14),
            Some(Prefix {
                start: 9,
                query: "src/".into(),
                quoted: false
            })
        );
        assert_eq!(prefix("use @one and more", 17), None);
        assert_eq!(
            Entry {
                path: "a file.txt".into(),
                directory: false
            }
            .replacement(false),
            ("@\"a file.txt\" ".into(), 14)
        );
        assert_eq!(
            Entry {
                path: "my dir".into(),
                directory: true
            }
            .replacement(true),
            ("@\"my dir/\"".into(), 9)
        );
    }
}
