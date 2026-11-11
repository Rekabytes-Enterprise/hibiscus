//! Dev servers the desktop app runs for a folder, so the browser pane and
//! Hibi's `dev_server` tool can preview the project (#176). One per folder,
//! owned by the app: they run in the background and stop with the folder.
use crate::Result;
use std::{
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Read},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Lines of output kept per server.
const KEEP: usize = 1000;

struct Server {
    command: String,
    child: Child,
    output: Arc<Mutex<Output>>,
}

#[derive(Default)]
struct Output {
    lines: VecDeque<String>,
    /// The first local URL the server printed.
    url: Option<String>,
}

static SERVERS: Mutex<Option<HashMap<PathBuf, Server>>> = Mutex::new(None);

fn servers<T>(f: impl FnOnce(&mut HashMap<PathBuf, Server>) -> T) -> T {
    f(SERVERS.lock().unwrap().get_or_insert_with(HashMap::new))
}

/// What a folder's server is doing.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub command: String,
    pub running: bool,
    /// Exit code once it has stopped by itself.
    pub exit_code: Option<i32>,
    pub url: Option<String>,
}

/// The usual start command for the project in `folder`, if it has one.
pub fn detect(folder: &Path) -> Option<String> {
    let has = |name: &str| folder.join(name).exists();
    if let Ok(text) = std::fs::read_to_string(folder.join("package.json")) {
        let package: serde_json::Value = serde_json::from_str(&text).ok()?;
        let script = ["dev", "start", "serve", "preview"]
            .into_iter()
            .find(|s| package["scripts"][s].is_string())?;
        let runner = if has("pnpm-lock.yaml") {
            "pnpm run"
        } else if has("yarn.lock") {
            "yarn run"
        } else if has("bun.lockb") || has("bun.lock") {
            "bun run"
        } else {
            "npm run"
        };
        return Some(format!("{runner} {script}"));
    }
    if has("manage.py") {
        return Some("python3 manage.py runserver".into());
    }
    if has("bin/rails") {
        return Some("bin/rails server".into());
    }
    if has("index.html") {
        return Some("python3 -m http.server 8000".into());
    }
    None
}

/// Starts `command`, or the detected one, in `folder`. A server already
/// running there is kept and returned instead.
pub fn start(folder: &Path, command: Option<&str>) -> Result<Status> {
    if let Some(status) = status(folder).filter(|s| s.running) {
        return Ok(status);
    }
    let command = match command {
        Some(c) if !c.trim().is_empty() => c.trim().to_string(),
        _ => detect(folder).ok_or("No start command found. Pass the command to run.")?,
    };
    let mut child = Command::new("/bin/sh")
        .args(["-c", &command])
        .current_dir(folder)
        // Keep tools from opening their own browser or buffering their output.
        .env("BROWSER", "none")
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own group, so stopping it also stops what the shell started.
        .process_group(0)
        .spawn()?;
    let output = Arc::new(Mutex::new(Output::default()));
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let output = output.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(|l| l.ok()) {
                let line = plain(&line);
                let mut o = output.lock().unwrap();
                if o.url.is_none() {
                    o.url = local_url(&line);
                }
                if o.lines.len() == KEEP {
                    o.lines.pop_front();
                }
                o.lines.push_back(line.chars().take(2000).collect());
            }
        });
    }
    servers(|s| {
        s.insert(
            folder.to_path_buf(),
            Server {
                command,
                child,
                output,
            },
        )
    });
    Ok(status(folder).expect("just started"))
}

/// Waits until the server prints its URL, stops, or `timeout` passes.
pub async fn ready(folder: &Path, timeout: Duration) -> Option<Status> {
    let end = Instant::now() + timeout;
    loop {
        let status = status(folder)?;
        if status.url.is_some() || !status.running || Instant::now() > end {
            return Some(status);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub fn status(folder: &Path) -> Option<Status> {
    servers(|s| {
        let server = s.get_mut(folder)?;
        let exit = server.child.try_wait().ok().flatten();
        Some(Status {
            command: server.command.clone(),
            running: exit.is_none(),
            exit_code: exit.and_then(|e| e.code()),
            url: server.output.lock().unwrap().url.clone(),
        })
    })
}

/// The last `count` lines the server printed.
pub fn output(folder: &Path, count: usize) -> Vec<String> {
    servers(|s| {
        s.get(folder).map_or_else(Vec::new, |server| {
            let o = server.output.lock().unwrap();
            o.lines
                .iter()
                .skip(o.lines.len().saturating_sub(count))
                .cloned()
                .collect()
        })
    })
}

pub fn stop(folder: &Path) {
    if let Some(server) = servers(|s| s.remove(folder)) {
        end(server);
    }
}

/// Stops every server, when the app quits.
pub fn stop_all() {
    for (_, server) in servers(std::mem::take) {
        end(server);
    }
}

fn end(mut server: Server) {
    if server.child.try_wait().ok().flatten().is_some() {
        return;
    }
    let group = -(server.child.id() as i32);
    // SAFETY: signals the server's own process group, created at spawn.
    unsafe { libc::kill(group, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(3);
    while server.child.try_wait().ok().flatten().is_none() {
        if Instant::now() > deadline {
            // SAFETY: as above.
            unsafe { libc::kill(group, libc::SIGKILL) };
            let _ = server.child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Removes terminal colour codes, which dev servers print around their URL.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip `ESC [ ... letter`.
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The first URL on this computer in a line of output.
fn local_url(line: &str) -> Option<String> {
    line.match_indices("http")
        .filter_map(|(i, _)| {
            let url = line[i..].split_whitespace().next()?;
            let url = url.trim_end_matches([')', ',', '.', ';', '"', '\'']);
            let rest = url
                .strip_prefix("http://")
                .or(url.strip_prefix("https://"))?;
            let host = rest.split([':', '/']).next()?;
            ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"]
                .contains(&host)
                .then(|| url.replacen("0.0.0.0", "localhost", 1))
        })
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_local_url_dev_servers_print() {
        let vite = plain("  \x1b[32m➜\x1b[39m  \x1b[1mLocal\x1b[22m:   \x1b[36mhttp://localhost:\x1b[1m5173\x1b[22m/\x1b[39m");
        assert_eq!(local_url(&vite).as_deref(), Some("http://localhost:5173/"));
        assert_eq!(
            local_url("Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...").as_deref(),
            Some("http://localhost:8000/")
        );
        assert_eq!(
            local_url("Starting development server at http://127.0.0.1:8000/").as_deref(),
            Some("http://127.0.0.1:8000/")
        );
        assert_eq!(local_url("See https://vitejs.dev/config for help"), None);
    }

    #[test]
    fn detects_the_start_command() {
        let dir = std::env::temp_dir().join(format!("hibiscus-dev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(detect(&dir), None);
        std::fs::write(dir.join("index.html"), "").unwrap();
        assert_eq!(detect(&dir).as_deref(), Some("python3 -m http.server 8000"));
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"start":"x","dev":"vite"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detect(&dir).as_deref(), Some("pnpm run dev"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn runs_one_server_per_folder_and_stops_its_whole_group() {
        let dir = std::env::temp_dir().join(format!("hibiscus-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = "echo 'ready at http://localhost:4321/'; sleep 30 & wait";
        let first = start(&dir, Some(script)).unwrap();
        let again = start(&dir, Some("echo other")).unwrap();
        assert_eq!(again.command, first.command);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let up = rt.block_on(ready(&dir, Duration::from_secs(5))).unwrap();
        assert_eq!(up.url.as_deref(), Some("http://localhost:4321/"));
        assert!(up.running);
        assert_eq!(output(&dir, 5), ["ready at http://localhost:4321/"]);
        stop(&dir);
        assert!(status(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
