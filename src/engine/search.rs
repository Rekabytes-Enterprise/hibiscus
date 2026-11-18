//! The grep and glob tools: search file contents and names without reading
//! whole files, skipping what .gitignore ignores, hidden folders and binaries,
//! with capped results. Built on ripgrep's own walker, so nothing needs installing.
use crate::Result;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use regex::RegexBuilder;
use std::path::{Path, PathBuf};
use tokio::sync::watch;

/// Matching lines (or files) returned at most.
const MAX_RESULTS: usize = 200;
/// A matching line is cut to this many characters.
const LINE_CHARS: usize = 300;
/// Larger files aren't searched; they are rarely source.
const MAX_FILE: u64 = 4 * 1024 * 1024;

pub(super) struct Grep {
    pub pattern: String,
    pub root: PathBuf,
    pub glob: Option<String>,
    pub ignore_case: bool,
    pub files_only: bool,
}

/// Checks a pattern before the search runs.
pub(super) fn check_pattern(pattern: &str) -> Result<()> {
    RegexBuilder::new(pattern)
        .size_limit(1 << 22)
        .build()
        .map(drop)
        .map_err(|e| format!("Invalid grep pattern: {e}").into())
}

fn walker(root: &Path, glob: Option<&str>) -> Result<ignore::Walk> {
    let mut builder = WalkBuilder::new(root);
    builder.sort_by_file_name(|a, b| a.cmp(b));
    if let Some(glob) = glob {
        let mut overrides = OverrideBuilder::new(root);
        overrides.add(glob)?;
        builder.overrides(overrides.build()?);
    }
    Ok(builder.build())
}

fn shown(root: &Path, cwd: &Path, path: &Path) -> String {
    let rel = path
        .strip_prefix(cwd)
        .or_else(|_| path.strip_prefix(root))
        .unwrap_or(path);
    let text = rel.display().to_string();
    // One path style for the model; Windows accepts forward slashes too.
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

fn more(what: &str) -> String {
    format!("\n[Showing the first {MAX_RESULTS} {what}; narrow the pattern, path or glob to see the rest.]")
}

pub(super) async fn grep(a: Grep, cwd: PathBuf, cancel: watch::Receiver<bool>) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let regex = RegexBuilder::new(&a.pattern)
            .case_insensitive(a.ignore_case)
            .size_limit(1 << 22)
            .build()?;
        let mut out = String::new();
        let mut found = 0;
        for entry in walker(&a.root, a.glob.as_deref())? {
            if *cancel.borrow() {
                return Err("Search cancelled".into());
            }
            let Ok(entry) = entry else { continue };
            let small = entry
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE);
            if !small {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes[..bytes.len().min(8192)].contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let name = shown(&a.root, &cwd, entry.path());
            for (n, line) in text.lines().enumerate() {
                if !regex.is_match(line) {
                    continue;
                }
                if found == MAX_RESULTS {
                    out.push_str(&more(if a.files_only { "files" } else { "matches" }));
                    return Ok(out);
                }
                found += 1;
                if a.files_only {
                    out.push_str(&format!("{name}\n"));
                    break;
                }
                let cut = line
                    .char_indices()
                    .nth(LINE_CHARS)
                    .map_or(line.len(), |(i, _)| i);
                let tail = if cut < line.len() { "…" } else { "" };
                out.push_str(&format!(
                    "{name}:{}: {}{tail}\n",
                    n + 1,
                    line[..cut].trim_end()
                ));
            }
        }
        Ok(if out.is_empty() {
            "No matches.".into()
        } else {
            out
        })
    })
    .await?
}

pub(super) async fn glob(
    pattern: String,
    root: PathBuf,
    cwd: PathBuf,
    cancel: watch::Receiver<bool>,
) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let mut out = String::new();
        let mut found = 0;
        for entry in walker(&root, Some(&pattern))? {
            if *cancel.borrow() {
                return Err("Search cancelled".into());
            }
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if found == MAX_RESULTS {
                out.push_str(&more("files"));
                return Ok(out);
            }
            found += 1;
            out.push_str(&format!("{}\n", shown(&root, &cwd, entry.path())));
        }
        Ok(if out.is_empty() {
            "No files match.".into()
        } else {
            out
        })
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hibiscus-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn main() {\n    run();\n}\nfn run() {}\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/app.ts"), "export function run() {}\n").unwrap();
        std::fs::write(dir.join("build/out.rs"), "fn run() {}\n").unwrap();
        std::fs::write(dir.join("src/blob.bin"), b"fn run\0").unwrap();
        // .gitignore applies inside a git repository.
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    #[tokio::test]
    async fn grep_finds_lines_and_respects_ignores_and_globs() {
        let dir = project();
        let (_tx, rx) = watch::channel(false);
        let run = |glob: Option<&str>, files_only| Grep {
            pattern: r"fn run".into(),
            root: dir.clone(),
            glob: glob.map(Into::into),
            ignore_case: false,
            files_only,
        };
        let out = grep(run(None, false), dir.clone(), rx.clone())
            .await
            .unwrap();
        assert_eq!(out, "src/main.rs:4: fn run() {}\n");
        let out = grep(run(Some("*.ts"), true), dir.clone(), rx.clone())
            .await
            .unwrap();
        assert_eq!(out, "No matches.");
        let mut a = run(None, true);
        a.pattern = "RUN".into();
        a.ignore_case = true;
        let out = grep(a, dir.clone(), rx.clone()).await.unwrap();
        assert_eq!(out, "src/app.ts\nsrc/main.rs\n");
        assert!(check_pattern("fn (").is_err());
        let out = glob("src/*.rs".into(), dir.clone(), dir.clone(), rx)
            .await
            .unwrap();
        assert_eq!(out, "src/main.rs\n");
        let _ = std::fs::remove_dir_all(dir);
    }
}
