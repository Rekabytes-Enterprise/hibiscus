//! Thin glue between the window and `hibiscus::desktop`. The engine runs in
//! this process on its own thread; the frontend only sees JSON records.

use hibiscus::desktop::{self, CodexSignIn, Engine, Start};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

type Reply = Sender<Result<String, String>>;

/// The running engine. Its thread owns the `Engine`, so sends never wait
/// behind a blocking event read for longer than one short poll.
struct Running {
    commands: Sender<(Value, Reply)>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
struct App(Mutex<Option<Running>>);

impl App {
    /// Stops the engine (if any) and waits for its tools to stop.
    fn stop(&self) {
        if let Some(running) = self.0.lock().unwrap().take() {
            drop(running.commands);
            let _ = running.thread.join();
        }
    }
}

fn pump(mut engine: Engine, commands: Receiver<(Value, Reply)>, app: AppHandle) {
    loop {
        loop {
            match commands.try_recv() {
                Ok((command, reply)) => {
                    let _ = reply.send(engine.send(command).map_err(|e| e.to_string()));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return engine.shutdown(),
            }
        }
        match engine.next_event(Duration::from_millis(25)) {
            Ok(Some(event)) => {
                let _ = app.emit("engine", event);
            }
            Ok(None) => {}
            Err(error) => {
                let _ = app.emit("engine-stopped", error.to_string());
                return;
            }
        }
    }
}

/// Starts an engine for `folder`: a new session, the latest one, or `session`.
#[tauri::command]
async fn open_project(
    app: AppHandle,
    state: State<'_, App>,
    folder: PathBuf,
    latest: bool,
    session: Option<PathBuf>,
) -> Result<(), String> {
    state.stop();
    let start = match session {
        Some(path) => Start::Selected(path),
        None if latest => Start::Latest,
        None => Start::New,
    };
    let engine = Engine::start(start, &folder).map_err(|e| e.to_string())?;
    let (commands, receiver) = mpsc::channel();
    let thread = std::thread::spawn(move || pump(engine, receiver, app));
    *state.0.lock().unwrap() = Some(Running { commands, thread });
    Ok(())
}

/// Sends a command; returns its id. The answer arrives as a `response` event.
#[tauri::command]
async fn send(state: State<'_, App>, command: Value) -> Result<String, String> {
    let (reply, answer) = mpsc::channel();
    state
        .0
        .lock()
        .unwrap()
        .as_ref()
        .ok_or("No project is open.")?
        .commands
        .send((command, reply))
        .map_err(|_| "The engine has stopped.")?;
    answer.recv().map_err(|_| "The engine has stopped.")?
}

#[tauri::command]
async fn close_project(state: State<'_, App>) -> Result<(), String> {
    state.stop();
    Ok(())
}

#[derive(Serialize)]
struct Saved {
    path: PathBuf,
    title: String,
    /// Milliseconds since the Unix epoch.
    modified: u64,
}

#[tauri::command]
async fn rename_session(path: PathBuf, name: String) -> Result<(), String> {
    desktop::rename_session(&path, &name).map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_session(path: PathBuf) -> Result<(), String> {
    desktop::delete_session(&path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sessions(folder: PathBuf) -> Result<Vec<Saved>, String> {
    Ok(desktop::sessions(&folder)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| Saved {
            modified: s
                .modified
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            path: s.path,
            title: s.title,
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Provider {
    id: String,
    api_key: bool,
    browser: bool,
    signed_in: bool,
    key_page: Option<&'static str>,
}

#[tauri::command]
async fn providers() -> Result<Vec<Provider>, String> {
    Ok(desktop::providers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| Provider {
            id: p.id,
            api_key: p.api_key,
            browser: p.browser,
            signed_in: p.signed_in,
            key_page: p.key_page,
        })
        .collect())
}

#[tauri::command]
async fn save_api_key(provider: String, key: String) -> Result<(), String> {
    desktop::save_api_key(&provider, &key).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sign_out(provider: String) -> Result<(), String> {
    desktop::sign_out(&provider).map_err(|e| e.to_string())
}

#[tauri::command]
async fn training_denied() -> bool {
    desktop::training_denied()
}

#[tauri::command]
async fn set_training_denied(deny: bool) -> Result<(), String> {
    desktop::set_training_denied(deny).map_err(|e| e.to_string())
}

/// The pending Codex sign-in: where pasted redirects go, and the task waiting for it.
#[derive(Default)]
struct Codex(
    Mutex<
        Option<(
            tauri::async_runtime::Sender<String>,
            tauri::async_runtime::JoinHandle<()>,
        )>,
    >,
);

impl Codex {
    /// Stops the pending sign-in; its port closes and the stored login is untouched.
    fn cancel(&self) {
        if let Some((_, task)) = self.0.lock().unwrap().take() {
            task.abort();
        }
    }
}

/// Opens the Codex sign-in page and returns its link. The result arrives as a
/// `codex-login` event: `null` once signed in, else the error.
#[tauri::command]
async fn codex_start(app: AppHandle, codex: State<'_, Codex>) -> Result<String, String> {
    codex.cancel();
    let sign_in = CodexSignIn::start().map_err(|e| e.to_string())?;
    let url = sign_in.url().to_owned();
    let paste = sign_in.paste();
    let done = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let error = sign_in.finish().await.err().map(|e| e.to_string());
        let _ = done.emit("codex-login", error);
    });
    *codex.0.lock().unwrap() = Some((paste, task));
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())?;
    Ok(url)
}

#[tauri::command]
async fn codex_paste(codex: State<'_, Codex>, url: String) -> Result<(), String> {
    let paste = codex
        .0
        .lock()
        .unwrap()
        .as_ref()
        .map(|(paste, _)| paste.clone());
    paste
        .ok_or("No Codex sign-in is waiting.")?
        .try_send(url)
        .map_err(|_| "Codex sign-in is no longer waiting for a link.".into())
}

#[tauri::command]
async fn codex_cancel(codex: State<'_, Codex>) -> Result<(), String> {
    codex.cancel();
    Ok(())
}

/// Diff text sent to the window at most; a huge diff would freeze it.
const MAX_DIFF: usize = 4 << 20;
/// Untracked files shown at most; each costs a `git diff` call.
const MAX_UNTRACKED: usize = 200;

#[derive(Serialize)]
struct Changes {
    /// The branch compared with, e.g. `dev`; `None` when there is none.
    base: Option<String>,
    /// Branches the user can compare with instead, for the pane's dropdown.
    bases: Vec<String>,
    branch: Option<String>,
    /// Unified diff of the working tree against where the branch left `base`,
    /// plus untracked files, as Claude and Codex show a branch's changes.
    diff: String,
    truncated: bool,
}

/// The session folder's changes for the session panel; `None` outside a git repository.
/// `base` is the branch the user picked; `None` picks the nearest one.
#[tauri::command]
async fn git_changes(folder: PathBuf, base: Option<String>) -> Option<Changes> {
    changes(&folder, base.as_deref())
}

/// Runs git in `folder`; `ok` lists the exit codes that still mean success.
fn git(folder: &Path, args: &[&str], ok: &[i32]) -> Option<Vec<u8>> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .ok()?;
    ok.contains(&out.status.code()?).then_some(out.stdout)
}

fn line(folder: &Path, args: &[&str]) -> Option<String> {
    let out = String::from_utf8(git(folder, args, &[0])?).ok()?;
    Some(out.trim().to_owned()).filter(|s| !s.is_empty())
}

/// Branch names that usually hold work in progress, nearest first when tied.
const KNOWN_BASES: [&str; 6] = ["dev", "develop", "development", "staging", "main", "master"];

/// Local and `origin` branch names, without duplicates or `current`.
fn branches(folder: &Path, current: Option<&str>) -> Vec<String> {
    let refs = line(
        folder,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    )
    .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for name in refs.lines().map(|r| r.strip_prefix("origin/").unwrap_or(r)) {
        // `origin/HEAD` lists as plain `origin`.
        if name != "origin"
            && name != "HEAD"
            && Some(name) != current
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// Where HEAD left `name` (local or `origin/`), and how many commits HEAD has since.
/// The nearer of the two wins: a stale local branch forks further back.
fn fork_point(folder: &Path, name: &str) -> Option<(String, u64)> {
    [name.to_owned(), format!("origin/{name}")]
        .iter()
        .filter_map(|r| {
            let fork = line(folder, &["merge-base", "HEAD", r])?;
            let ahead = line(folder, &["rev-list", "--count", &format!("{fork}..HEAD")])?;
            Some((fork, ahead.parse().ok()?))
        })
        .min_by_key(|(_, ahead)| *ahead)
}

/// The branch to compare with: `picked` when it exists, else the nearest of the
/// upstream, the default branch and the usual base names. A branch made from `dev`
/// compares with `dev`, so unreleased `dev` work does not show as its own.
fn pick_base(
    folder: &Path,
    current: Option<&str>,
    bases: &[String],
    picked: Option<&str>,
) -> Option<(String, String)> {
    if let Some(name) = picked.filter(|p| bases.iter().any(|b| b == p)) {
        if let Some((fork, _)) = fork_point(folder, name) {
            return Some((name.to_owned(), fork));
        }
    }
    // The upstream counts only when it is another branch, not this one's push target.
    let upstream = line(folder, &["rev-parse", "--abbrev-ref", "@{upstream}"])
        .map(|r| r.strip_prefix("origin/").map(str::to_owned).unwrap_or(r));
    let default = line(folder, &["rev-parse", "--abbrev-ref", "origin/HEAD"])
        .and_then(|r| r.strip_prefix("origin/").map(str::to_owned));
    let mut candidates: Vec<String> = Vec::new();
    for name in upstream
        .into_iter()
        .chain(default)
        .chain(KNOWN_BASES.iter().map(|b| b.to_string()))
    {
        if Some(name.as_str()) != current && bases.contains(&name) && !candidates.contains(&name) {
            candidates.push(name);
        }
    }
    // min_by_key keeps the first of equals, so ties go to the upstream, then the default.
    candidates
        .into_iter()
        .filter_map(|name| fork_point(folder, &name).map(|(fork, ahead)| (name, fork, ahead)))
        .min_by_key(|(_, _, ahead)| *ahead)
        .map(|(name, fork, _)| (name, fork))
}

fn changes(folder: &Path, picked: Option<&str>) -> Option<Changes> {
    line(folder, &["rev-parse", "--is-inside-work-tree"])?;
    let branch = line(folder, &["branch", "--show-current"]);
    let bases = branches(folder, branch.as_deref());
    let chosen = pick_base(folder, branch.as_deref(), &bases, picked);
    let fork = chosen.as_ref().map(|(_, fork)| fork.clone());
    // No shared history (or no commits yet): compare with HEAD, or with nothing.
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let from = fork
        .clone()
        .or_else(|| line(folder, &["rev-parse", "--verify", "--quiet", "HEAD"]))
        .unwrap_or_else(|| EMPTY_TREE.into());
    let mut diff = git(
        folder,
        &["diff", "--no-color", "--no-ext-diff", "--relative", &from],
        &[0],
    )
    .unwrap_or_default();
    let untracked = git(
        folder,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &[0],
    )
    .unwrap_or_default();
    for path in untracked
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .take(MAX_UNTRACKED)
    {
        if diff.len() > MAX_DIFF {
            break;
        }
        let path = String::from_utf8_lossy(path);
        // --no-index exits 1 when the files differ, which they always do here.
        let args = [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-index",
            "--",
            "/dev/null",
            &path,
        ];
        diff.extend(git(folder, &args, &[0, 1]).unwrap_or_default());
    }
    let truncated = diff.len() > MAX_DIFF;
    if truncated {
        // Cut at a line end so no half line is shown.
        let end = diff[..MAX_DIFF]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        diff.truncate(end);
    }
    Some(Changes {
        base: chosen.map(|(name, _)| name),
        bases,
        branch,
        diff: String::from_utf8_lossy(&diff).into_owned(),
        truncated,
    })
}

/// Opens a file the session wrote with its default app. Only files inside the
/// session folder: the path comes from model-written tool arguments.
#[tauri::command]
async fn open_file(folder: PathBuf, path: PathBuf) -> Result<(), String> {
    let file = inside(&folder, &path).ok_or("That file is outside the session folder.")?;
    tauri_plugin_opener::open_path(file, None::<&str>).map_err(|e| e.to_string())
}

fn inside(folder: &Path, path: &Path) -> Option<PathBuf> {
    let folder = folder.canonicalize().ok()?;
    let file = folder.join(path).canonicalize().ok()?;
    (file.starts_with(&folder) && file.is_file()).then_some(file)
}

/// The agent-browser the app bundles next to its own binary (`externalBin`).
fn agent_browser() -> Option<PathBuf> {
    let path = std::env::current_exe()
        .ok()?
        .parent()?
        .join("agent-browser");
    path.is_file().then_some(path)
}

/// Runs one agent-browser command for the browser pane and returns its `data`.
/// Only the few commands the pane needs are allowed, never ones that change
/// the page's content or read its storage.
#[tauri::command]
async fn browser(args: Vec<String>) -> Result<Value, String> {
    let argv = browser_args(&args)?;
    let bin = agent_browser().ok_or("The bundled agent-browser is missing")?;
    let output = tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new(bin)
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let reply: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| String::from_utf8_lossy(&output.stderr).trim().to_string())?;
    if reply["success"].as_bool() == Some(true) {
        Ok(reply["data"].clone())
    } else {
        Err(reply["error"]
            .as_str()
            .unwrap_or("agent-browser failed")
            .to_string())
    }
}

/// Ports common dev servers listen on: Vite, Next/CRA/Rails, Angular, Astro,
/// Storybook, and generic ones.
const DEV_PORTS: &[u16] = &[
    5173, 5174, 3000, 3001, 4200, 4321, 8080, 8000, 8888, 5000, 4000, 6006, 3333, 9000,
];

/// Local ports from `DEV_PORTS` that accept a connection, in that order.
#[tauri::command]
async fn dev_servers() -> Vec<u16> {
    tauri::async_runtime::spawn_blocking(|| {
        std::thread::scope(|scope| {
            let probes: Vec<_> = DEV_PORTS
                .iter()
                .map(|&port| {
                    scope.spawn(move || {
                        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
                        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(250))
                            .is_ok()
                            .then_some(port)
                    })
                })
                .collect();
            probes
                .into_iter()
                .filter_map(|p| p.join().ok().flatten())
                .collect()
        })
    })
    .await
    .unwrap_or_default()
}

/// Closes the app's browser, so it doesn't outlive the window. Waits at most
/// a few seconds, so a stuck browser never holds up quitting.
fn close_browser() {
    let Some(bin) = agent_browser() else {
        return;
    };
    let Ok(mut child) = std::process::Command::new(bin)
        .args(["--session", desktop::BROWSER_SESSION, "close"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while child.try_wait().ok().flatten().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.wait();
}

/// Finds the element at a point on the page for comment mode: a CSS path to
/// it, its text, the page URL and its box. `__X__` and `__Y__` are replaced
/// with checked numbers; nothing else from the window reaches the script.
const HIT: &str = r##"(() => {
  const e = document.elementFromPoint(__X__, __Y__);
  if (!e) return null;
  const path = [];
  for (let n = e; n && n.nodeType === 1 && n !== document.documentElement; n = n.parentElement) {
    if (n.id) { path.unshift("#" + CSS.escape(n.id)); break; }
    const same = n.parentElement ? [...n.parentElement.children].filter((c) => c.tagName === n.tagName) : [];
    path.unshift(n.tagName.toLowerCase() + (same.length > 1 ? `:nth-of-type(${same.indexOf(n) + 1})` : ""));
  }
  const r = e.getBoundingClientRect();
  const text = e.innerText || e.getAttribute("aria-label") || e.getAttribute("alt") || e.getAttribute("placeholder") || "";
  return { selector: path.join(" > "), text: text.trim().replace(/\s+/g, " ").slice(0, 80), url: location.href, x: r.x, y: r.y, width: r.width, height: r.height };
})()"##;

fn browser_args(args: &[String]) -> Result<Vec<String>, String> {
    let point = |n: &str| n.parse::<u32>().ok().filter(|n| *n <= 10_000);
    if let ["hit", x, y] = args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        let (Some(x), Some(y)) = (point(x), point(y)) else {
            return Err(format!("Not allowed: agent-browser {}", args.join(" ")));
        };
        let script = HIT
            .replace("__X__", &x.to_string())
            .replace("__Y__", &y.to_string());
        return Ok([
            "--session",
            desktop::BROWSER_SESSION,
            "eval",
            &script,
            "--json",
        ]
        .map(String::from)
        .to_vec());
    }
    let ok = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["stream", "status"] | ["back"] | ["forward"] | ["reload"] | ["close"] => true,
        // A leading dash would be read as a flag.
        ["get", "box", target] => !target.starts_with('-'),
        ["open", url] => url.starts_with("http://") || url.starts_with("https://"),
        // The pane sizes the page to fit itself.
        ["set", "viewport", w, h] => [w, h]
            .iter()
            .all(|n| n.parse::<u32>().is_ok_and(|n| (200..=4000).contains(&n))),
        _ => false,
    };
    if !ok {
        return Err(format!("Not allowed: agent-browser {}", args.join(" ")));
    }
    let mut argv = vec![
        "--session".to_string(),
        desktop::BROWSER_SESSION.to_string(),
    ];
    argv.extend(args.iter().cloned());
    argv.push("--json".into());
    Ok(argv)
}

/// A Dock- or menu-launched app gets a minimal environment, so tools would
/// miss the user's PATH and provider keys set in their shell profile. Copy
/// the login shell's environment, as a terminal launch would have it.
fn import_login_environment() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let Ok(mut child) = std::process::Command::new(&shell)
        .args(["-ilc", "printf '\\0__HIBISCUS_ENV__\\0'; env -0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return;
    };
    // A shell profile that waits for input must not hang the app.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while child.try_wait().ok().flatten().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let Ok(output) = child.wait_with_output() else {
        return;
    };
    apply_environment(&output.stdout);
}

fn apply_environment(output: &[u8]) {
    let marker = b"\0__HIBISCUS_ENV__\0";
    let Some(start) = output.windows(marker.len()).position(|w| w == marker) else {
        return;
    };
    for entry in output[start + marker.len()..].split(|&b| b == 0) {
        let Ok(entry) = std::str::from_utf8(entry) else {
            continue;
        };
        if let Some((name, value)) = entry.split_once('=') {
            if !name.is_empty() && !matches!(name, "PWD" | "OLDPWD" | "SHLVL" | "_") {
                std::env::set_var(name, value);
            }
        }
    }
}

fn main() {
    // Before any thread starts: set_var is not thread-safe.
    if std::env::var_os("HIBISCUS_DESKTOP_KEEP_ENV").is_none() {
        import_login_environment();
    }
    // Gives the engine its browser tool.
    if let Some(path) = agent_browser() {
        std::env::set_var("HIBISCUS_AGENT_BROWSER", path);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_websocket::init())
        .manage(App::default())
        .manage(Codex::default())
        .invoke_handler(tauri::generate_handler![
            open_project,
            send,
            close_project,
            sessions,
            rename_session,
            delete_session,
            providers,
            save_api_key,
            sign_out,
            training_denied,
            set_training_denied,
            codex_start,
            codex_paste,
            codex_cancel,
            git_changes,
            open_file,
            browser,
            dev_servers
        ])
        .build(tauri::generate_context!())
        .expect("failed to start Hibiscus")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Quit like the TUI: stop the run and wait for tools to stop.
                app.state::<App>().stop();
                close_browser();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_cover_the_branch_since_main_and_untracked_files() {
        let repo = std::env::temp_dir().join(format!("hibiscus-changes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "base"]);
        run(&["switch", "-qc", "work"]);
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        run(&["commit", "-qam", "on the branch"]);
        std::fs::write(repo.join("b.txt"), "uncommitted\n").unwrap();
        run(&["add", "b.txt"]);
        std::fs::write(repo.join("new file.txt"), "untracked\n").unwrap();

        let c = changes(&repo, None).unwrap();
        assert_eq!(c.base.as_deref(), Some("main"));
        assert_eq!(c.bases, ["main"]);
        assert_eq!(c.branch.as_deref(), Some("work"));
        assert!(!c.truncated);
        for expected in ["-one", "+two", "+uncommitted", "+untracked", "new file.txt"] {
            assert!(
                c.diff.contains(expected),
                "{expected} missing from:\n{}",
                c.diff
            );
        }
        assert!(changes(&std::env::temp_dir().join("hibiscus-not-a-repo-xyz"), None).is_none());
        std::fs::remove_dir_all(&repo).unwrap();
    }

    #[test]
    fn changes_compare_with_the_nearest_base_or_the_picked_one() {
        let repo = std::env::temp_dir().join(format!("hibiscus-bases-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "base"]);
        // Unreleased work on dev, then a docs branch made from dev.
        run(&["switch", "-qc", "dev"]);
        std::fs::write(repo.join("ci.yml"), "unreleased\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "dev work"]);
        run(&["switch", "-qc", "docs"]);
        std::fs::write(repo.join("README.md"), "docs\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "docs"]);

        let c = changes(&repo, None).unwrap();
        assert_eq!(c.base.as_deref(), Some("dev"));
        assert_eq!(c.bases, ["dev", "main"]);
        assert!(
            c.diff.contains("+docs") && !c.diff.contains("unreleased"),
            "{}",
            c.diff
        );

        let main = changes(&repo, Some("main")).unwrap();
        assert_eq!(main.base.as_deref(), Some("main"));
        assert!(main.diff.contains("+docs") && main.diff.contains("+unreleased"));

        // A branch that no longer exists falls back to the nearest one.
        assert_eq!(
            changes(&repo, Some("gone")).unwrap().base.as_deref(),
            Some("dev")
        );
        std::fs::remove_dir_all(&repo).unwrap();
    }

    #[test]
    fn only_files_inside_the_folder_open() {
        let root = std::env::temp_dir().join(format!("hibiscus-inside-{}", std::process::id()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        std::fs::write(root.join("project/a.txt"), "a").unwrap();
        std::fs::write(root.join("secret.txt"), "s").unwrap();
        let project = root.join("project");
        assert!(inside(&project, Path::new("a.txt")).is_some());
        assert!(inside(&project, &project.join("a.txt")).is_some());
        assert!(inside(&project, Path::new("../secret.txt")).is_none());
        assert!(inside(&project, &root.join("secret.txt")).is_none());
        assert!(inside(&project, Path::new(".")).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn login_environment_is_read_after_the_marker_only() {
        let mut output = b"motd PATH=/evil\n".to_vec();
        output.extend_from_slice(b"\0__HIBISCUS_ENV__\0");
        output.extend_from_slice(b"HIBISCUS_GLUE_TEST=a=b\0PWD=/nope\0broken\0");
        apply_environment(&output);
        assert_eq!(std::env::var("HIBISCUS_GLUE_TEST").unwrap(), "a=b");
        assert_ne!(std::env::var("PWD").ok().as_deref(), Some("/nope"));
        apply_environment(b"no marker HIBISCUS_GLUE_OTHER=x");
        assert!(std::env::var("HIBISCUS_GLUE_OTHER").is_err());
    }

    #[test]
    fn browser_pane_runs_only_its_own_commands() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            browser_args(&args(&["get", "box", "@e3"])).unwrap(),
            args(&["--session", "hibiscus", "get", "box", "@e3", "--json"])
        );
        let hit = browser_args(&args(&["hit", "12", "340"])).unwrap();
        assert_eq!(hit[2], "eval");
        assert!(hit[3].contains("elementFromPoint(12, 340)"));
        for bad in [
            &["eval", "1"][..],
            &["hit", "1", "x"],
            &["hit", "-1", "2"],
            &["hit", "1", "2", "3"],
            &["open", "file:///etc/passwd"],
            &["open", "--profile"],
            &["get", "box", "--json"],
            &["cookies"],
            &["stream", "enable"],
            &["set", "viewport", "100", "800"],
            &["set", "viewport", "-1", "800"],
        ] {
            assert!(browser_args(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
