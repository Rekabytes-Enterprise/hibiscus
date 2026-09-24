use crate::{
    tui::screen::{escape_key, Navigation, Screen},
    Result,
};
use serde_json::{json, Value};
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthOutcome {
    Success,
    Cancelled,
    Failed,
    Unavailable,
}

/// Only load the SDK belonging to the selected Pi installation. The override
/// is useful for custom Pi installs and for isolated mock tests.
fn sdk_entry() -> Option<PathBuf> {
    if let Some(entry) = env::var_os("HIBISCUS_AUTH_SDK") {
        let path = PathBuf::from(entry);
        return path.is_file().then_some(path);
    }
    let pi = env::var_os("HIBISCUS_PI").map(PathBuf::from).or_else(|| {
        env::split_paths(&env::var_os("PATH")?)
            .map(|dir| dir.join("pi"))
            .find(|path| path.is_file())
    })?;
    let pi = if pi.components().count() == 1 {
        env::split_paths(&env::var_os("PATH")?)
            .map(|dir| dir.join(&pi))
            .find(|path| path.is_file())?
    } else {
        pi
    };
    // Direct npm installs: node_modules/.bin/pi resolves into the package.
    if let Ok(real) = pi.canonicalize() {
        for parent in real.ancestors() {
            if parent.file_name().is_some_and(|name| name == "dist")
                && parent.parent().is_some_and(|pkg| {
                    pkg.file_name()
                        .is_some_and(|name| name == "pi-coding-agent")
                })
            {
                let entry = parent.join("index.js");
                if entry.is_file() {
                    return Some(entry);
                }
            }
        }
    }
    // Pi's managed launcher: ~/.pi/agent/bin/pi selects install/current-version.
    let agent = env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".pi/agent")))?;
    if !is_managed_pi(&pi, &agent) {
        return None;
    }
    managed_entry(&agent)
}

fn is_managed_pi(pi: &Path, agent: &Path) -> bool {
    pi.starts_with(agent.join("bin"))
        || matches!(
            (pi.canonicalize(), agent.join("bin/pi").canonicalize()),
            (Ok(path), Ok(launcher)) if path == launcher
        )
}

fn managed_entry(agent: &Path) -> Option<PathBuf> {
    let version = std::fs::read_to_string(agent.join("install/current-version")).ok()?;
    let version = version.trim();
    if version.is_empty()
        || matches!(version, "." | "..")
        || !version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "._+-".contains(ch))
    {
        return None;
    }
    let entry = agent
        .join("install/releases")
        .join(version)
        .join("node_modules/@earendil-works/pi-coding-agent/dist/index.js");
    entry.is_file().then_some(entry)
}

fn send(writer: &mut ChildStdin, message: Value) -> io::Result<()> {
    writeln!(writer, "{message}")?;
    writer.flush()
}

fn show_notice(output: &mut Screen, record: &Value) -> Result<()> {
    let safe = |field: &str| {
        record[field].as_str().map(|text| {
            text.chars()
                .filter(|ch| !ch.is_control())
                .take(2048)
                .collect::<String>()
        })
    };
    match record["kind"].as_str() {
        Some("auth_url") => {
            let link_ready = match record["url"].as_str() {
                Some(url) => output.set_auth_url(url)?,
                None => false,
            };
            if link_ready {
                writeln!(
                    output,
                    "  · Open Codex sign-in ↗ (Ctrl+click; Ctrl+Y to copy link)"
                )?;
                writeln!(output, "  · If the WSL callback fails, paste the redirect URL into the masked composer.")?;
            } else {
                writeln!(
                    output,
                    "  · Could not display a safe Codex sign-in URL. Cancel and use Pi /login."
                )?;
            }
        }
        Some("device_code") => {
            if let Some(url) = safe("verificationUri") {
                writeln!(output, "  · Visit: {url}")?;
            }
            if let Some(code) = safe("userCode") {
                writeln!(output, "  · Enter device code: {code}")?;
            }
        }
        _ => {
            if let Some(message) = safe("message") {
                writeln!(output, "  · Codex: {message}")?;
            }
        }
    }
    output.flush()?;
    Ok(())
}

pub(crate) fn codex_sign_in(output: &mut Screen, events: &Receiver<u8>) -> Result<AuthOutcome> {
    let Some(sdk) = sdk_entry() else {
        return Ok(AuthOutcome::Unavailable);
    };
    let node = env::var("HIBISCUS_NODE").unwrap_or_else(|_| "node".into());
    let mut child = match Command::new(node)
        .args([
            "--input-type=module",
            "--eval",
            include_str!("codex-auth.mjs"),
            "--",
        ])
        .arg(sdk)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AuthOutcome::Unavailable)
        }
        Err(error) => return Err(error.into()),
    };
    let mut writer = child.stdin.take().ok_or("auth helper stdin unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("auth helper stdout unavailable")?;
    let (tx, rx) = mpsc::channel::<Result<Value>>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let record = line
                .map_err(|_| "auth helper output ended unexpectedly".into())
                .and_then(|line| {
                    serde_json::from_str(&line).map_err(|_| "invalid auth helper record".into())
                });
            if tx.send(record).is_err() {
                break;
            }
        }
    });

    let result = (|| -> Result<AuthOutcome> {
        let mut pending: Option<Result<Value>> = None;
        let mut outcome = AuthOutcome::Failed;
        loop {
            // Esc cancels even during device polling; Ctrl+Y requests a copy
            // without exposing the URL as broken transcript text.
            if let Ok(key) = events.try_recv() {
                if key == 25 {
                    output.copy_auth_url()?;
                }
                if key == 27
                    && matches!(
                        escape_key(events),
                        Navigation::Escape | Navigation::EscapeWith(_)
                    )
                {
                    let _ = send(&mut writer, json!({"type":"cancel"}));
                    outcome = AuthOutcome::Cancelled;
                    break;
                }
            }
            let record = if let Some(record) = pending.take() {
                record?
            } else {
                match rx.recv_timeout(Duration::from_millis(60)) {
                    Ok(record) => record?,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };
            match record["type"].as_str() {
                Some("notice") => show_notice(output, &record)?,
                Some("prompt") => {
                    let id = record["id"].as_u64().ok_or("invalid auth prompt id")?;
                    let answer = if record["kind"] == "select" {
                        let options = record["options"].as_array().ok_or("invalid auth options")?;
                        let labels = options
                            .iter()
                            .map(|item| item["label"].as_str().unwrap_or("option").to_owned())
                            .collect::<Vec<_>>();
                        output
                            .select("Codex sign-in", &labels, None, events)?
                            .and_then(|index| options[index]["id"].as_str().map(str::to_owned))
                    } else {
                        let question = record["message"]
                            .as_str()
                            .unwrap_or("Enter Codex authorization response:");
                        let question: String = question
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .take(300)
                            .collect();
                        writeln!(output, "  · {question}")?;
                        output.flush()?;
                        let mut cancelled = false;
                        let secret = record["kind"] != "text";
                        let answer = output.auth_input(events, secret, || {
                            match rx.try_recv() {
                                Ok(Ok(event))
                                    if matches!(
                                        event["type"].as_str(),
                                        Some("prompt_cancelled" | "done")
                                    ) =>
                                {
                                    pending = Some(Ok(event));
                                    cancelled = true;
                                }
                                Ok(record) => {
                                    pending = Some(record);
                                    cancelled = true;
                                }
                                Err(mpsc::TryRecvError::Disconnected) => cancelled = true,
                                Err(mpsc::TryRecvError::Empty) => {}
                            }
                            Ok(cancelled)
                        })?;
                        answer
                    };
                    if let Some(value) = answer {
                        send(&mut writer, json!({"type":"answer","id":id,"value":value}))?;
                    } else if pending.is_none() {
                        let _ = send(&mut writer, json!({"type":"cancel"}));
                        outcome = AuthOutcome::Cancelled;
                        break;
                    }
                }
                Some("prompt_cancelled") => {}
                Some("done") => {
                    if record["success"] == true {
                        outcome = AuthOutcome::Success;
                    }
                    break;
                }
                _ => {}
            }
        }
        Ok(outcome)
    })();
    drop(writer);
    // `done` is authoritative: runtime.login has finished OAuth and storage.
    // Pi's callback server can retain a browser connection after server.close(),
    // keeping Node alive indefinitely. Never wait for that connection to close.
    // This dedicated helper has no remaining work once the protocol loop ends.
    let cleanup = (|| -> io::Result<()> {
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        child.wait()?;
        Ok(())
    })();
    let cleared = output.clear_auth_url();
    cleanup?;
    cleared?;
    // A deliberate cleanup kill must not turn a completed login into failure.
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_pi_launcher_linked_into_local_bin_keeps_sdk_auth_available() {
        let root = env::temp_dir().join(format!("hibiscus-pi-link-test-{}", std::process::id()));
        let agent = root.join(".pi/agent");
        let local = root.join(".local/bin");
        std::fs::create_dir_all(agent.join("bin")).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        let launcher = agent.join("bin/pi");
        std::fs::write(&launcher, "#!/bin/sh\n").unwrap();
        let link = local.join("pi");
        std::os::unix::fs::symlink(&launcher, &link).unwrap();
        assert!(is_managed_pi(&link, &agent));
        assert!(!is_managed_pi(&root.join("other/pi"), &agent));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_pi_sdk_resolution_rejects_traversal() {
        let root = env::temp_dir().join(format!("hibiscus-sdk-test-{}", std::process::id()));
        let entry = root.join(
            "install/releases/0.1/node_modules/@earendil-works/pi-coding-agent/dist/index.js",
        );
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "export class ModelRuntime {}").unwrap();
        std::fs::write(root.join("install/current-version"), "0.1\n").unwrap();
        assert_eq!(managed_entry(&root), Some(entry));
        std::fs::write(root.join("install/current-version"), "../outside").unwrap();
        assert_eq!(managed_entry(&root), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
