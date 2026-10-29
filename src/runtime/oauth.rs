//! Native Codex subscription OAuth. No external agent or credential-store import.
//! Protocol reference: Pi 1.1.0's legacy openai-codex adapter, not its separate
//! dynamically registered openai-chatgpt flow. Client ID is a public identifier.
mod protocol;
mod store;

use crate::{tui::screen::Screen, Result};
use std::{sync::mpsc::Receiver, time::Duration};
pub(super) const RESOURCE: &str = "https://chatgpt.com/backend-api/codex";
pub(super) use store::Token;

pub(super) fn stored_access_token() -> Result<String> {
    store::Store::app()?
        .read()?
        .map(|t| t.access)
        .ok_or_else(|| {
            "Authentication required for openai-codex. Use /login → Browser Login → Codex.".into()
        })
}

pub(super) async fn credential() -> Result<Token> {
    credential_at(&store::Store::app()?, &protocol::Endpoints::production()).await
}

async fn credential_at(store: &store::Store, endpoints: &protocol::Endpoints) -> Result<Token> {
    let _lock = store.lock().await?;
    let token = store.read()?.ok_or("Codex is signed out. Use /login.")?;
    if token.expires > protocol::now().saturating_add(60) {
        return Ok(token);
    }
    let refreshed = protocol::refresh(endpoints, &token).await?;
    store.write(&refreshed)?;
    Ok(refreshed)
}

pub(super) fn remove() -> Result<()> {
    let store = store::Store::app()?;
    let _lock = store.try_lock()?;
    match std::fs::remove_file(store.path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn sign_in(
    output: &mut Screen,
    events: &Receiver<u8>,
) -> Result<super::auth::AuthOutcome> {
    let store = store::Store::app()?;
    let pending = protocol::Pending::bind()?;
    let url = pending.url(&protocol::Endpoints::production())?;
    output.set_auth_url(url.as_str())?;
    let (manual_tx, manual_rx) = tokio::sync::mpsc::channel(1);
    let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<Token> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let endpoints = protocol::Endpoints::production();
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => Err("Codex sign-in cancelled".into()),
                    result = tokio::time::timeout(Duration::from_secs(600), pending.complete(manual_rx, &endpoints)) =>
                        result.map_err(|_| "Codex sign-in timed out. Retry /login.")?,
                }
            })
        })();
        let _ = done_tx.send(result);
    });
    // Launch only the validated fixed-origin authorization URL, without a shell.
    open_browser(url.as_str());
    let mut completed = None;
    let ui_result = (|| -> Result<()> {
        let mut submitted = false;
        loop {
            let placeholder = if submitted {
                "Completing sign-in… Esc cancels"
            } else {
                "Finish in browser, or paste the full redirect URL"
            };
            let answer = output.auth_prompt(
                events,
                "Codex browser sign-in · Ctrl+Y copy link",
                placeholder,
                true,
                || match done_rx.try_recv() {
                    Ok(result) => {
                        completed = Some(result);
                        Ok(true)
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => Ok(false),
                    Err(_) => Err("Codex login worker stopped unexpectedly".into()),
                },
            )?;
            if completed.is_some() || answer.is_none() {
                break;
            }
            if !submitted {
                manual_tx
                    .try_send(answer.unwrap())
                    .map_err(|_| "Codex login is no longer waiting for a redirect")?;
                submitted = true;
            }
        }
        Ok(())
    })();
    cancel.send_replace(true);
    let joined = worker.join();
    let cleared = output.clear_auth_url();
    ui_result?;
    joined.map_err(|_| "Codex login worker stopped unexpectedly")?;
    cleared?;
    let Some(token) = completed else {
        return Ok(super::auth::AuthOutcome::Cancelled);
    };
    let token = token?;
    // Cancellation never commits, and failed attempts preserve the previous login.
    let _lock = store.try_lock()?;
    store.write(&token)?;
    Ok(super::auth::AuthOutcome::Success)
}

#[cfg(test)]
mod tests;

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let program = "/usr/bin/open";
    #[cfg(not(target_os = "macos"))]
    let program = "xdg-open";
    if let Ok(mut child) = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}
