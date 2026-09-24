//! Opt-in self-update for prebuilt installs. The embedded, checksum-verifying
//! installer is also used for first-time installation; no remote script runs.
use crate::Result;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const REPOSITORY: &str = "Rekabytes-Enterprise/hibiscus";
const LATEST: &str = "https://api.github.com/repos/Rekabytes-Enterprise/hibiscus/releases/latest";

fn version(text: &str) -> Option<[u64; 3]> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let mut next = || {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        part.parse::<u64>().ok()
    };
    let parsed = [next()?, next()?, next()?];
    parts.next().is_none().then_some(parsed)
}

pub(crate) fn check() -> Result<Option<String>> {
    let result = Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "3",
            "--max-time",
            "5",
            "--max-filesize",
            "1048576",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: hibiscus-update",
            LATEST,
        ])
        .output()?;
    if !result.status.success() {
        return Err(
            "could not check GitHub Releases (offline, rate-limited, or no public release)".into(),
        );
    }
    let data: Value = serde_json::from_slice(&result.stdout)?;
    let tag = data["tag_name"]
        .as_str()
        .ok_or("release has no version tag")?;
    let latest = version(tag).ok_or("release tag must be vMAJOR.MINOR.PATCH")?;
    let current = version(env!("CARGO_PKG_VERSION")).ok_or("invalid installed version")?;
    Ok((latest > current).then(|| tag.to_owned()))
}

fn install_path() -> Result<PathBuf> {
    let dir = if let Some(custom) = env::var_os("HIBISCUS_INSTALL_DIR") {
        PathBuf::from(custom)
    } else {
        PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?).join(".local/bin")
    };
    let expected = dir.join("hibiscus");
    let executable = env::current_exe()?.canonicalize()?;
    if fs::symlink_metadata(&expected).is_err()
        || fs::symlink_metadata(&expected)?.file_type().is_symlink()
        || expected.canonicalize()? != executable
    {
        return Err("self-update is only available for a prebuilt hibiscus in ~/.local/bin (or HIBISCUS_INSTALL_DIR); source/Cargo installs should be updated with cargo install --path .".into());
    }
    Ok(expected)
}

pub(crate) fn installed_prebuilt() -> bool {
    install_path().is_ok()
}

pub(crate) fn install(tag: &str) -> Result<()> {
    version(tag).ok_or("invalid release version")?;
    if !tag.starts_with('v') {
        return Err("release tag must start with v".into());
    }
    let path = install_path()?;
    let mut child = Command::new("sh")
        .arg("-s")
        .env("HIBISCUS_REPOSITORY", REPOSITORY)
        .env("HIBISCUS_VERSION", tag)
        .env(
            "HIBISCUS_INSTALL_DIR",
            path.parent().ok_or("no install directory")?,
        )
        .env_remove("GITHUB_TOKEN")
        .stdin(Stdio::piped())
        .spawn()?;
    let write_result = child
        .stdin
        .take()
        .ok_or("installer stdin unavailable")?
        .write_all(include_str!("../install.sh").as_bytes());
    let status = child.wait()?;
    write_result?;
    if !status.success() {
        return Err(format!("installer failed: {status}; existing binary was not replaced").into());
    }
    Ok(())
}

pub(crate) fn update() -> Result<()> {
    install_path()?;
    match check()? {
        Some(tag) => {
            install(&tag)?;
            println!("Restart Hibiscus to use {tag}.");
        }
        None => println!("Hibiscus {} is up to date.", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}

/// Don't recheck or repeat a dismissed notice on each chat startup. An explicit
/// `hibiscus update` always performs a fresh request.
pub(crate) fn check_startup() -> Option<String> {
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?
        .join("hibiscus/update-check");
    if let Ok(age) = fs::metadata(&cache)
        .and_then(|m| m.modified())
        .and_then(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .map_err(std::io::Error::other)
        })
    {
        if age < Duration::from_secs(24 * 60 * 60) {
            return None;
        }
    }
    let available = check().ok()?;
    if let Some(parent) = cache.parent() {
        if fs::create_dir_all(parent).is_ok() {
            let _ = fs::write(&cache, available.as_deref().unwrap_or("current"));
        }
    }
    available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_only_strict_numeric_release_versions() {
        assert_eq!(version("v1.10.0"), Some([1, 10, 0]));
        assert!(version("v1.10.0").unwrap() > version("v1.9.12").unwrap());
        for invalid in [
            "v1.2",
            "1.2.3-beta",
            "v1.2.3/evil",
            "v01.2.3",
            "v1.2.3.4",
            "v1..3",
            "v1.2.+3",
        ] {
            assert!(version(invalid).is_none(), "{invalid}");
        }
    }
}
