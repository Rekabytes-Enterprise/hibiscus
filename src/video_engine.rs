//! The video engine: Node.js, the HyperFrames CLI with its skills, and the
//! Chrome HyperFrames renders with. Nothing is bundled; the canvas's "Download
//! engine" button installs it once into `~/.hibiscus/runtime`. Node.js is pinned
//! and checked against nodejs.org's SHA-256; npm checks the HyperFrames package's
//! own integrity hash; HyperFrames fetches its pinned Chrome.
use crate::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const NODE: &str = "22.23.5";
/// HyperFrames (Apache-2.0); its npm package carries the CLI and its skills.
const HYPERFRAMES: &str = "0.9.59";

/// The Node.js archive for this machine and its published SHA-256.
fn node_asset() -> Option<(&'static str, &'static str)> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => (
            "darwin-arm64.tar.gz",
            "e7131995759b61a0170b646010d2ad35a5d37b4cb0d471f3e8317eac6f742f3d",
        ),
        ("macos", "x86_64") => (
            "darwin-x64.tar.gz",
            "c53383238206b9ae4db23c5d8a63645e2e753bec8ab51a49cc81939aa64e9422",
        ),
        ("linux", "x86_64") => (
            "linux-x64.tar.gz",
            "c54118104acc524633b5abe106bbf44a53126ec8b4e5fa105cb92b5d7de8a1a1",
        ),
        ("windows", "x86_64") => (
            "win-x64.zip",
            "4a191082e9706866830379d577d2cc8a50c776fcd9436a8d061b063ed0d9a0e3",
        ),
        _ => return None,
    })
}

fn runtime() -> Result<PathBuf> {
    Ok(crate::paths::home()?.join("runtime"))
}

/// `node-v22.23.5-darwin-arm64`, the folder the archive unpacks to.
fn node_name() -> Option<String> {
    let (asset, _) = node_asset()?;
    let platform = asset.trim_end_matches(".tar.gz").trim_end_matches(".zip");
    Some(format!("node-v{NODE}-{platform}"))
}

fn node_dir() -> Option<PathBuf> {
    Some(runtime().ok()?.join(node_name()?))
}

/// Where `node` and `npm` live: `bin` on macOS and Linux, the top folder on Windows.
fn node_bin() -> Option<PathBuf> {
    let dir = node_dir()?;
    Some(if cfg!(windows) { dir } else { dir.join("bin") })
}

fn node_exe() -> Option<PathBuf> {
    Some(node_bin()?.join(if cfg!(windows) { "node.exe" } else { "node" }))
}

fn hyperframes_prefix() -> Option<PathBuf> {
    Some(runtime().ok()?.join(format!("hyperframes-{HYPERFRAMES}")))
}

fn hyperframes_cli() -> Option<PathBuf> {
    Some(hyperframes_prefix()?.join("node_modules/hyperframes/bin/hyperframes.mjs"))
}

fn ready_marker() -> Option<PathBuf> {
    Some(
        runtime()
            .ok()?
            .join(format!("video-engine-{NODE}-{HYPERFRAMES}.ready")),
    )
}

/// Whether the whole engine is installed for these pinned versions.
pub fn installed() -> bool {
    ready_marker().is_some_and(|m| m.is_file())
        && node_exe().is_some_and(|n| n.is_file())
        && hyperframes_cli().is_some_and(|c| c.is_file())
}

/// Folders the agent's shell should find first: Node.js, the `hyperframes`
/// command, and the app's bundled ffmpeg. Empty until the engine is installed.
pub(crate) fn tool_paths() -> Vec<PathBuf> {
    if !installed() {
        return vec![];
    }
    let mut paths: Vec<PathBuf> = [
        node_bin(),
        hyperframes_prefix().map(|p| p.join("node_modules/.bin")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if let Some(ffmpeg) = crate::video::ffmpeg().and_then(|f| f.parent().map(Path::to_path_buf)) {
        paths.push(ffmpeg);
    }
    paths
}

/// Adds the engine to a command the agent runs: its tools first on `PATH`, and
/// HyperFrames' usage telemetry off, matching the app's privacy defaults.
pub(crate) fn prepare(command: &mut std::process::Command) {
    let tools = tool_paths();
    if tools.is_empty() {
        return;
    }
    let rest = std::env::var_os("PATH").unwrap_or_default();
    if let Ok(path) = std::env::join_paths(tools.into_iter().chain(std::env::split_paths(&rest))) {
        command.env("PATH", path);
    }
    command.env("HYPERFRAMES_NO_TELEMETRY", "1");
}

/// A folder of `hyperframes` and `ffmpeg` commands for a WSL-connected folder,
/// whose shell runs in Linux and can't run the Windows engine by name. Each
/// calls the Windows copy through WSL interop. `None` off Windows or before
/// the engine is installed.
pub(crate) fn wsl_bin() -> Option<PathBuf> {
    if !cfg!(windows) || !installed() {
        return None;
    }
    let node = node_exe()?;
    let mut tools = vec![(
        "hyperframes",
        wsl_wrapper(&node, hyperframes_cli().as_deref())?,
    )];
    if let Some(ffmpeg) = crate::video::ffmpeg() {
        tools.push(("ffmpeg", wsl_wrapper(&ffmpeg, None)?));
    }
    let bin = runtime().ok()?.join("wsl-bin");
    std::fs::create_dir_all(&bin).ok()?;
    for (name, text) in tools {
        let path = bin.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&path, text).ok()?;
        }
    }
    Some(bin)
}

/// A sh script that runs the Windows program `exe` (with `script` as its first
/// argument, for Node.js) and turns Linux path arguments into Windows ones.
/// shortcut: only arguments that are whole absolute paths are converted, not
/// `--out=/x`; convert `=` forms too if the agent's commands need them.
fn wsl_wrapper(exe: &Path, script: Option<&Path>) -> Option<String> {
    let (_, exe) = crate::wsl::to_linux(&exe.to_string_lossy())?;
    let script = script
        .map(|s| format!(" {}", crate::wsl::quote(&s.to_string_lossy())))
        .unwrap_or_default();
    Some(format!(
        "#!/bin/sh\n# Written by Hibiscus: runs the Windows video engine from WSL.\nfor a do\n  shift\n  case \"$a\" in /*) if [ -e \"$a\" ] || [ -d \"$(dirname \"$a\")\" ]; then a=$(wslpath -w \"$a\"); fi ;; esac\n  set -- \"$@\" \"$a\"\ndone\nexec {}{script} \"$@\"\n",
        crate::wsl::quote(&exe)
    ))
}

/// Serves a HyperFrames project's preview, attached so the app owns the process.
pub(crate) const PREVIEW: &str = "hyperframes preview --foreground --no-open";

/// Starts the preview server for `project`, or reuses the one already running,
/// and returns its local address. The app's dev server manager runs it, so it
/// outlives the agent's command (in WSL too, where a background process dies
/// with the command) and stops when the app quits.
pub async fn preview(project: &Path) -> Result<String> {
    crate::devserver::start(project, Some(PREVIEW))?;
    let status = crate::devserver::ready(project, Duration::from_secs(90))
        .await
        .ok_or("The preview server is gone.")?;
    status.url.ok_or_else(|| {
        format!(
            "The preview server didn't print its address. Last output:\n{}",
            crate::devserver::output(project, 20).join("\n")
        )
        .into()
    })
}

/// HyperFrames' own skills, shipped inside its npm package.
pub(crate) fn skills_dir() -> Option<PathBuf> {
    installed()
        .then(|| hyperframes_prefix().map(|p| p.join("node_modules/hyperframes/dist/skills")))
        .flatten()
        .filter(|d| d.is_dir())
}

/// A step of the install, for the button's label.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub step: &'static str,
    /// 0 to 100 while a download's size is known.
    pub percent: Option<u8>,
}

/// Downloads `url`, checking it against `sha256`, into `out`.
async fn download(
    url: &str,
    sha256: &str,
    out: &Path,
    step: &'static str,
    progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(1800))
        .build()?;
    let mut response = client.get(url).send().await?.error_for_status()?;
    let total = response.content_length();
    let mut hash = Sha256::new();
    let mut file = std::fs::File::create(out)?;
    let mut done = 0u64;
    while let Some(chunk) = response.chunk().await? {
        use std::io::Write;
        file.write_all(&chunk)?;
        hash.update(&chunk);
        done += chunk.len() as u64;
        progress(Progress {
            step,
            percent: total.map(|t| (done * 100 / t.max(1)).min(100) as u8),
        });
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha256 {
        let _ = std::fs::remove_file(out);
        return Err(format!("The download from {url} does not match its published checksum, so it was not installed.").into());
    }
    Ok(())
}

/// Runs a setup command and fails with its last output lines.
async fn run(mut command: tokio::process::Command, what: &str) -> Result<()> {
    crate::sys::own_group(command.as_std_mut());
    let output = command
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<_> = text
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Err(format!("{what} failed: {}", tail.join("\n")).into())
}

/// Installs whatever is missing, reporting each step.
pub async fn install(progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<()> {
    let (asset, sha256) =
        node_asset().ok_or("The video engine isn't available for this computer yet.")?;
    let runtime = runtime()?;
    std::fs::create_dir_all(&runtime)?;
    let node = node_exe().ok_or("No Node.js for this computer")?;

    if !node.is_file() {
        let archive = runtime.join(format!("node-v{NODE}-{asset}.part"));
        download(
            &format!("https://nodejs.org/dist/v{NODE}/node-v{NODE}-{asset}"),
            sha256,
            &archive,
            "Downloading Node.js",
            progress,
        )
        .await?;
        progress(Progress {
            step: "Unpacking Node.js",
            percent: None,
        });
        // bsdtar on macOS and Windows reads zip and tar.gz; GNU tar on Linux gets tar.gz.
        let mut tar = tokio::process::Command::new("tar");
        tar.arg(if asset.ends_with(".zip") {
            "-xf"
        } else {
            "-xzf"
        })
        .arg(&archive)
        .arg("-C")
        .arg(&runtime);
        let unpacked = run(tar, "Unpacking Node.js").await;
        let _ = std::fs::remove_file(&archive);
        unpacked?;
    }

    let prefix = hyperframes_prefix().ok_or("No HyperFrames folder")?;
    let npm = node_dir()
        .ok_or("No Node.js folder")?
        .join(if cfg!(windows) {
            "node_modules/npm/bin/npm-cli.js"
        } else {
            "lib/node_modules/npm/bin/npm-cli.js"
        });
    let path = std::env::join_paths(tool_dirs_for_setup().into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))?;
    if !hyperframes_cli().is_some_and(|c| c.is_file()) {
        progress(Progress {
            step: "Installing HyperFrames",
            percent: None,
        });
        std::fs::create_dir_all(&prefix)?;
        let mut install = tokio::process::Command::new(&node);
        install
            .arg(&npm)
            .args([
                "install",
                "--no-audit",
                "--no-fund",
                "--loglevel=error",
                "--prefix",
            ])
            .arg(&prefix)
            .arg(format!("hyperframes@{HYPERFRAMES}"))
            .env("PATH", &path);
        run(install, "Installing HyperFrames").await?;
    }

    progress(Progress {
        step: "Downloading Chrome for rendering",
        percent: None,
    });
    let mut chrome = tokio::process::Command::new(&node);
    chrome
        .arg(hyperframes_cli().ok_or("No HyperFrames")?)
        .args(["browser", "ensure"])
        .env("PATH", &path)
        .env("HYPERFRAMES_NO_TELEMETRY", "1");
    run(chrome, "Downloading Chrome").await?;

    std::fs::write(
        ready_marker().ok_or("No runtime folder")?,
        format!("node {NODE}\nhyperframes {HYPERFRAMES}\n"),
    )?;
    progress(Progress {
        step: "Ready",
        percent: Some(100),
    });
    Ok(())
}

/// The tool folders while installing, before the ready marker exists.
fn tool_dirs_for_setup() -> Vec<PathBuf> {
    [
        node_bin(),
        crate::video::ffmpeg().and_then(|f| f.parent().map(Path::to_path_buf)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_wrappers_call_the_windows_programs() {
        let text = wsl_wrapper(
            Path::new(r"C:\Users\me\.hibiscus\runtime\node\node.exe"),
            Some(Path::new(
                r"C:\Users\me\.hibiscus\runtime\hf\hyperframes.mjs",
            )),
        )
        .unwrap();
        assert!(text.starts_with("#!/bin/sh\n"));
        assert!(text.contains("a=$(wslpath -w \"$a\")"));
        assert!(text.ends_with(
            "exec '/mnt/c/Users/me/.hibiscus/runtime/node/node.exe' 'C:\\Users\\me\\.hibiscus\\runtime\\hf\\hyperframes.mjs' \"$@\"\n"
        ));
        assert!(wsl_wrapper(Path::new("/usr/bin/ffmpeg"), None).is_none());
    }

    #[test]
    fn every_supported_machine_has_a_pinned_node() {
        let (asset, sha) = node_asset().expect("tests run on a supported machine");
        assert!(asset.ends_with(".tar.gz") || asset.ends_with(".zip"));
        assert_eq!(sha.len(), 64);
        assert!(node_name().unwrap().starts_with(&format!("node-v{NODE}-")));
        assert!(!node_name().unwrap().ends_with(".gz"));
    }
}
