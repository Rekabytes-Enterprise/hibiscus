//! Explicit checks run only during managed updates, never during discovery.
use super::{
    manifest,
    versions::{Candidate, CheckResult},
    Registry,
};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Check {
    Exec {
        command: Vec<String>,
        #[serde(default = "timeout", rename = "timeoutSeconds")]
        timeout_seconds: u64,
    },
    Tool {
        name: String,
        arguments: Value,
    },
    Command {
        name: String,
        input: String,
    },
}
fn timeout() -> u64 {
    30
}
pub(super) fn declarations(checks: &[Check]) -> Result<()> {
    if checks.len() > 8 {
        return Err("At most eight extension regression checks are allowed".into());
    }
    for check in checks {
        match check {
            Check::Exec {
                command,
                timeout_seconds,
            } => {
                if command.is_empty()
                    || command.len() > 32
                    || command
                        .iter()
                        .any(|s| s.is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
                    || !(1..=600).contains(timeout_seconds)
                {
                    return Err("Regression command needs 1–32 bounded arguments and timeoutSeconds of 1–600".into());
                }
            }
            Check::Tool { name, arguments } => {
                if !manifest::identifier(name)
                    || !arguments.is_object()
                    || serde_json::to_vec(arguments)?.len() > 65536
                {
                    return Err("Regression tool check needs a local tool name and bounded arguments object".into());
                }
            }
            Check::Command { name, input } => {
                if !manifest::identifier(name) || input.len() > 65536 {
                    return Err(
                        "Regression command check needs a local command name and bounded input"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
}
fn executable_path(value: &str, directory: &Path) -> Result<PathBuf> {
    let expanded = value.replace(
        "{extensionDir}",
        directory.to_str().ok_or("Extension path must be UTF-8")?,
    );
    let path = Path::new(&expanded);
    if let Some(relative) = value.strip_prefix("{extensionDir}/") {
        super::versions::relative(relative)?;
    }
    if !path.is_absolute() && expanded.contains('/') {
        super::versions::relative(expanded.trim_start_matches("./"))?;
    }
    if path.is_absolute() {
        return Ok(path.into());
    }
    if expanded.contains('/') {
        return Ok(directory.join(path));
    }
    std::env::var_os("PATH")
        .and_then(|env| {
            std::env::split_paths(&env)
                .map(|p| p.join(path))
                .find(|p| is_executable(p))
        })
        .ok_or_else(|| format!("Extension executable is unavailable on PATH: {expanded}").into())
}
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
pub(super) fn executable(manifest: &manifest::Manifest, directory: &Path) -> Result<()> {
    let directory = directory.canonicalize()?;
    if !is_executable(&executable_path(&manifest.command[0], &directory)?) {
        return Err("Extension command must resolve to an executable regular file".into());
    }
    for arg in &manifest.command[1..] {
        if arg.starts_with("{extensionDir}/") {
            let relative = arg.trim_start_matches("{extensionDir}/");
            let path = directory.join(super::versions::relative(relative)?);
            if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
                return Err("Extension command refers to a missing implementation file".into());
            }
        }
    }
    for check in &manifest.checks {
        match check {
            Check::Tool { name, .. } if !manifest.tools.iter().any(|t| &t.name == name) => {
                return Err("Regression check refers to an unknown local tool".into())
            }
            Check::Command { name, .. } if !manifest.commands.iter().any(|c| &c.name == name) => {
                return Err("Regression check refers to an unknown local command".into())
            }
            _ => {}
        }
    }
    Ok(())
}
pub(super) async fn run(
    candidate: &Candidate,
    context: (&Path, &str),
    cancel: &mut watch::Receiver<bool>,
) -> Result<Vec<CheckResult>> {
    let (manifest, _) = manifest::read(&candidate.files())?;
    let mut results = Vec::new();
    if manifest.checks.is_empty() {
        return Ok(results);
    }
    // A check may generate caches or test reports. Never let these alter the
    // implementation that was validated and will become the retained version.
    let copy = candidate.check_copy()?;
    let mut registry = Registry::default();
    registry.add_directory(&manifest.id, &copy, None)?;
    for (index, check) in manifest.checks.iter().enumerate() {
        let name = format!("check-{}", index + 1);
        let start = Instant::now();
        let result: Result<String> = async {
            if *cancel.borrow() {
                return Err("Extension validation cancelled".into());
            }
            match check {
                Check::Exec {
                    command,
                    timeout_seconds,
                } => {
                    let mut process = manifest.inherit_env.command(
                        executable_path(&command[0], &copy)?,
                        &copy,
                        context.0,
                    )?;
                    process
                        .args(
                            command[1..]
                                .iter()
                                .map(|s| s.replace("{extensionDir}", &copy.to_string_lossy())),
                        )
                        .current_dir(&copy);
                    let output = crate::process::run(
                        process,
                        None,
                        Duration::from_secs(*timeout_seconds),
                        cancel,
                    )
                    .await?;
                    if !output.status.success() {
                        return Err(format!(
                            "Regression command exited with {}\n{}",
                            output.status,
                            String::from_utf8_lossy(&output.stderr)
                        )
                        .into());
                    }
                    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
                }
                Check::Tool { name, arguments } => {
                    let value = registry
                        .execute_tool(
                            &name_for_call(index),
                            &format!("{}__{name}", manifest.id),
                            arguments,
                            context,
                            cancel,
                        )
                        .await?;
                    if value["isError"] == true {
                        return Err(value["content"][0]["text"]
                            .as_str()
                            .unwrap_or("Regression tool failed")
                            .to_owned()
                            .into());
                    }
                    Ok(value["content"][0]["text"].as_str().unwrap_or("").into())
                }
                Check::Command { name, input } => {
                    let command = manifest
                        .commands
                        .iter()
                        .find(|c| &c.name == name)
                        .ok_or("Unknown regression command")?;
                    let value = registry
                        .execute_command(
                            &name_for_call(index),
                            &command.public_name(&manifest.id),
                            input,
                            context,
                            cancel,
                        )
                        .await?;
                    if value["isError"] == true {
                        return Err(value["text"]
                            .as_str()
                            .unwrap_or("Regression command failed")
                            .to_owned()
                            .into());
                    }
                    Ok(value["text"].as_str().unwrap_or("").into())
                }
            }
        }
        .await;
        let passed = result.is_ok();
        let text = match result {
            Ok(text) => text,
            Err(error) => error.to_string(),
        };
        results.push(CheckResult {
            name,
            passed,
            text: if text.is_empty() {
                String::new()
            } else {
                crate::runtime::error::safe_message(&text)
            },
            duration_millis: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        });
        if !passed {
            break;
        }
    }
    Ok(results)
}
fn name_for_call(index: usize) -> String {
    format!("extension-validation-{index}")
}
