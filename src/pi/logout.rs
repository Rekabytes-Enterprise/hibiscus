use super::auth::sdk_entry;
use crate::{
    tui::screen::{escape_key, Navigation, Screen},
    Result,
};
use serde_json::{json, Value};
use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub(crate) enum LogoutOutcome {
    Removed {
        provider: String,
        synchronization_warning: bool,
    },
    Cancelled,
    Empty,
    Unavailable,
}

struct Helper(Child);
impl Drop for Helper {
    fn drop(&mut self) {
        // SAFETY: the helper is created in its own process group. Stop before
        // reaping, including any child retaining an output pipe.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn record(
    receiver: &Receiver<Value>,
    events: &Receiver<u8>,
    output: &mut Screen,
    cancellable: bool,
) -> Result<Option<Value>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() >= deadline {
            return Err(if cancellable {
                "Could not read stored providers in time. Credentials were not changed."
            } else {
                "Logout helper timed out. Credential removal could not be confirmed; check /logout again."
            }.into());
        }
        while let Ok(key) = events.try_recv() {
            let cancel = match key {
                3 | 4 => true,
                27 => match escape_key(events) {
                    Navigation::Escape => true,
                    Navigation::EscapeWith(next) => {
                        if cancellable {
                            output.defer_input(next);
                        }
                        true
                    }
                    _ => false,
                },
                _ => false,
            };
            if cancel && cancellable {
                return Ok(None);
            }
        }
        match receiver.recv_timeout(Duration::from_millis(40)) {
            Ok(value) => return Ok(Some(value)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                return Err(if cancellable {
                    "Could not read stored providers. Credentials were not changed."
                } else {
                    "Logout helper stopped without confirming the result. Check /logout again."
                }
                .into());
            }
        }
    }
}

/// List only credential metadata. The caller closes the idle RPC process after
/// confirmation but before the helper is authorized to mutate Pi's auth store.
pub(crate) fn sign_out(
    output: &mut Screen,
    events: &Receiver<u8>,
    mut before_remove: impl FnMut() -> Result<()>,
) -> Result<LogoutOutcome> {
    let Some(sdk) = sdk_entry() else {
        return Ok(LogoutOutcome::Unavailable);
    };
    let node = env::var("HIBISCUS_NODE").unwrap_or_else(|_| "node".into());
    let child = match Command::new(node)
        .args([
            "--input-type=module",
            "--eval",
            include_str!("logout.mjs"),
            "--",
        ])
        .arg(sdk)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LogoutOutcome::Unavailable)
        }
        Err(_) => return Err("Could not start Pi's logout helper.".into()),
    };
    let mut helper = Helper(child);
    let mut writer = helper
        .0
        .stdin
        .take()
        .ok_or("Logout helper stdin unavailable")?;
    let stdout = helper
        .0
        .stdout
        .take()
        .ok_or("Logout helper stdout unavailable")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            if line.len() > 64 * 1024 {
                break;
            }
            let Ok(value) = serde_json::from_str(&line) else {
                break;
            };
            if sender.send(value).is_err() {
                break;
            }
        }
    });
    writeln!(output, "  · Reading stored providers… (Esc to cancel)")?;
    output.flush()?;
    let Some(response) = record(&receiver, events, output, true)? else {
        return Ok(LogoutOutcome::Cancelled);
    };
    if response["type"] == "unavailable" {
        return Ok(LogoutOutcome::Unavailable);
    }
    let providers = response["providers"]
        .as_array()
        .filter(|_| response["type"] == "providers")
        .ok_or(
            "Could not list stored providers through the Pi SDK. Credentials were not changed.",
        )?;
    if providers.is_empty() {
        return Ok(LogoutOutcome::Empty);
    }
    // Validate IDs before using them as display text or authorizing removal.
    let entries = providers
        .iter()
        .map(|provider| {
            let id = provider["id"]
                .as_str()
                .ok_or("Invalid logout provider ID")?;
            if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
                return Err("Invalid logout provider ID");
            }
            let kind = match provider["type"].as_str() {
                Some("oauth") => "OAuth",
                Some("api_key") => "API key",
                _ => return Err("Invalid stored credential type"),
            };
            Ok((id.to_owned(), format!("{id} · {kind}")))
        })
        .collect::<std::result::Result<Vec<_>, &str>>()?;
    let labels = entries
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let Some(index) = output.select("Sign out of", &labels, None, events)? else {
        return Ok(LogoutOutcome::Cancelled);
    };
    let provider = entries[index].0.clone();
    writeln!(output, "  · Logout removes the stored credential shared by Pi and Hibiscus. Environment variables and models.json are unchanged; provider tokens are not revoked.")?;
    if output.select(
        &format!("Remove {provider} credentials?"),
        &["Cancel".into(), "Remove stored credential".into()],
        None,
        events,
    )? != Some(1)
    {
        return Ok(LogoutOutcome::Cancelled);
    }
    // No writes to auth.json are performed by Hibiscus. Pi's public API owns
    // deletion, storage locking and credential synchronization.
    before_remove()?;
    writeln!(output, "  · Removing stored credential…")?;
    output.flush()?;
    writeln!(writer, "{}", json!({"type":"logout", "provider":provider}))
        .and_then(|_| writer.flush())
        .map_err(|_| "Could not confirm logout. Check /logout again.")?;
    // Once committed, cancellation cannot promise a rollback. Wait for the
    // bounded result, and always reconnect the caller afterward, even on error.
    let response = record(&receiver, events, output, false)?.ok_or("Logout result unavailable")?;
    if response["type"] != "done" || response["removed"] != true {
        return Err("Logout failed or removal could not be confirmed. Check /logout again; environment credentials may still provide access.".into());
    }
    Ok(LogoutOutcome::Removed {
        provider,
        synchronization_warning: response["synchronizationWarning"] == true,
    })
}
