//! The /login prompts. Credential storage stays in the runtime (auth, oauth).
use crate::{
    runtime::{auth, oauth},
    ui::Frontend,
    Result,
};
use std::{sync::mpsc::Receiver, time::Duration};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthOutcome {
    Success,
    Cancelled,
}

pub(crate) fn sign_in(
    output: &mut Frontend,
    events: &Receiver<u8>,
    provider: &str,
    auth_type: &str,
) -> Result<AuthOutcome> {
    if auth_type == "oauth" && provider == "openai-codex" {
        return codex(output, events);
    }
    if auth_type != "api_key" {
        return Err("This provider requires a supported native authentication flow. Use a configured API-key provider.".into());
    }
    auth::known_api_key_provider(provider)?;
    let answer = match crate::runtime::catalog::key_page(provider) {
        Some(page) => output.auth_prompt(
            events,
            &format!("{provider} API key · create one at {page}"),
            "Enter API key",
            true,
            || Ok(false),
        )?,
        None => output.auth_input(events, true, || Ok(false))?,
    };
    let Some(answer) = answer else {
        return Ok(AuthOutcome::Cancelled);
    };
    auth::save_api_key(provider, answer)?;
    Ok(AuthOutcome::Success)
}

fn codex(output: &mut Frontend, events: &Receiver<u8>) -> Result<AuthOutcome> {
    let sign_in = oauth::BrowserSignIn::start()?;
    let url = sign_in.url().to_owned();
    output.set_auth_url(&url)?;
    let (manual_tx, manual_rx) = tokio::sync::mpsc::channel(1);
    let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<oauth::Token> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => Err("Codex sign-in cancelled".into()),
                    result = tokio::time::timeout(Duration::from_secs(600), sign_in.complete(manual_rx)) =>
                        result.map_err(|_| "Codex sign-in timed out. Retry /login.")?,
                }
            })
        })();
        let _ = done_tx.send(result);
    });
    // Launch only the validated fixed-origin authorization URL, without a shell.
    open_browser(&url);
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
        return Ok(AuthOutcome::Cancelled);
    };
    let token = token?;
    // Cancellation never commits, and failed attempts preserve the previous login.
    oauth::save(&token)?;
    Ok(AuthOutcome::Success)
}

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
