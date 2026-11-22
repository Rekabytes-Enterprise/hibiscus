//! Local PDF preparation for Desktop. The provider receives a reference to
//! extracted text, and the document never supplies shell or installer arguments.
use crate::{process, Result};
use std::{path::PathBuf, time::Duration};
use tokio::{process::Command, sync::watch};

const PDF_LIMIT: usize = 32 * 1024 * 1024;
const TEXT_LIMIT: usize = 4 * 1024 * 1024;
static SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Extracts a dropped PDF to a private, pageable text file. Poppler receives
/// the PDF over stdin, so no raw document or original filename is staged on disk.
pub async fn prepare(bytes: Vec<u8>) -> Result<PathBuf> {
    if bytes.len() > PDF_LIMIT {
        return Err("Use a PDF of at most 32 MiB.".into());
    }
    if !bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|s| s == b"%PDF-")
    {
        return Err("This file does not have a PDF header.".into());
    }
    let (_keep_open, mut cancel) = watch::channel(false);
    let mut command = converter(&mut cancel).await?;
    command.args(["-enc", "UTF-8", "-", "-"]);
    let output = process::run_with_stdout_limit(
        command,
        Some(bytes),
        Duration::from_secs(30),
        &mut cancel,
        TEXT_LIMIT,
    )
    .await
    .map_err(|error| match error {
        process::Error::OutputLimit(TEXT_LIMIT) => {
            "The PDF produces too much text. Attach a smaller page range as a separate PDF."
                .to_owned()
        }
        error => format!("Could not read PDF: {error}"),
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        return Err(if output.status.code() == Some(3) || stderr.contains("password") {
            "This PDF is password protected or restricts text copying. Attach an unlocked copy."
        } else {
            "Could not extract text from this PDF. Check that it opens in a PDF viewer and try again."
        }
        .into());
    }
    let text = numbered_pages(std::str::from_utf8(&output.stdout)?)?;
    let path = crate::paths::home()?
        .join("attachments")
        .join(format!("pdf-{}.txt", crate::engine::config::unique()));
    crate::engine::config::atomic(&path, text.as_bytes())?;
    Ok(path)
}

fn numbered_pages(text: &str) -> Result<String> {
    use std::fmt::Write;
    let mut pages = String::new();
    let mut has_text = false;
    for (index, page) in text.split_terminator('\x0c').enumerate() {
        let page = page.trim();
        has_text |= !page.is_empty();
        writeln!(
            pages,
            "[PDF page {}]\n{}\n",
            index + 1,
            if page.is_empty() {
                "[No text layer]"
            } else {
                page
            }
        )?;
        if pages.len() > TEXT_LIMIT {
            return Err("The extracted PDF text is too large. Attach a smaller PDF.".into());
        }
    }
    if !has_text {
        return Err("This PDF has no extractable text. Apply OCR to the scanned pages and attach the searchable copy.".into());
    }
    Ok(pages)
}

async fn converter(cancel: &mut watch::Receiver<bool>) -> Result<Command> {
    let _setup = SETUP.lock().await;
    if let Some(command) = find_converter(cancel).await {
        return Ok(command);
    }

    let command = install_command()?;
    let output = process::run(command, None, Duration::from_secs(300), cancel)
        .await
        .map_err(|error| format!("Could not install Poppler for PDF reading: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty());
        return Err(match detail {
            Some(line) => format!("Could not install Poppler: {}", line.chars().take(240).collect::<String>()),
            None => "Poppler setup was cancelled or failed. Retry and approve the system permission prompt if one appears.".into(),
        }.into());
    }
    find_converter(cancel)
        .await
        .ok_or_else(|| "Poppler installed, but Hibiscus could not find pdftotext. Restart Hibiscus and try again.".into())
}

async fn find_converter(cancel: &mut watch::Receiver<bool>) -> Option<Command> {
    let candidates = [PathBuf::from(if cfg!(windows) {
        "pdftotext.exe"
    } else {
        "pdftotext"
    })]
    .into_iter();
    #[cfg(target_os = "macos")]
    let candidates = candidates.chain(
        [
            "/opt/homebrew/bin/pdftotext",
            "/usr/local/bin/pdftotext",
            "/opt/local/bin/pdftotext",
        ]
        .map(PathBuf::from),
    );
    #[cfg(windows)]
    let candidates = candidates.chain(
        std::env::var_os("LOCALAPPDATA")
            .map(|data| PathBuf::from(data).join("Microsoft/WinGet/Links/pdftotext.exe")),
    );
    for executable in candidates {
        if usable(&executable, cancel).await {
            return Some(Command::new(executable));
        }
    }
    #[cfg(windows)]
    {
        // WinGet can update the registered PATH after this app was launched.
        let mut paths = Command::new("powershell.exe");
        paths.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; foreach ($scope in 'User','Machine') { $path = [Environment]::GetEnvironmentVariable('Path', $scope); if ($path) { [Environment]::ExpandEnvironmentVariables($path) } }",
        ]);
        let paths = process::run(paths, None, Duration::from_secs(10), cancel)
            .await
            .ok()?;
        for line in std::str::from_utf8(&paths.stdout).ok()?.lines() {
            for dir in std::env::split_paths(line) {
                if !dir.is_absolute() {
                    continue;
                }
                let executable = dir.join("pdftotext.exe");
                if usable(&executable, cancel).await {
                    return Some(Command::new(executable));
                }
            }
        }
    }
    None
}

async fn usable(executable: &std::path::Path, cancel: &mut watch::Receiver<bool>) -> bool {
    let mut probe = Command::new(executable);
    probe.arg("-v");
    process::run(probe, None, Duration::from_secs(5), cancel)
        .await
        .is_ok_and(|output| output.status.success())
}

#[cfg(target_os = "macos")]
fn install_command() -> Result<Command> {
    for brew in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
        if std::path::Path::new(brew).is_file() {
            let mut command = crate::sys::shell(&format!("exec {brew} install poppler"));
            command.env("HOMEBREW_NO_AUTO_UPDATE", "1");
            return Ok(Command::from(command));
        }
    }
    if std::path::Path::new("/opt/local/bin/port").is_file() {
        let mut command = Command::new("/usr/bin/osascript");
        command.args([
            "-e",
            "do shell script \"/opt/local/bin/port -N install poppler\" with administrator privileges",
        ]);
        return Ok(command);
    }
    Err("Install Homebrew or MacPorts to let Hibiscus set up Poppler, then retry the PDF.".into())
}

#[cfg(target_os = "linux")]
fn install_command() -> Result<Command> {
    let packages: &[(&str, &[&str])] = &[
        ("/usr/bin/apt-get", &["install", "-y", "poppler-utils"]),
        ("/usr/bin/dnf", &["install", "-y", "poppler-utils"]),
        (
            "/usr/bin/pacman",
            &["-S", "--needed", "--noconfirm", "poppler"],
        ),
    ];
    let (manager, args) = packages
        .iter()
        .find(|(path, _)| std::path::Path::new(path).is_file())
        .ok_or("Install poppler-utils (or poppler) for your distribution, then retry the PDF.")?;
    if !std::path::Path::new("/usr/bin/pkexec").is_file() {
        return Err("A graphical Polkit authorization agent is required for automatic setup. Install pdftotext through your package manager, then retry the PDF.".into());
    }
    let mut command = Command::new("/usr/bin/pkexec");
    command
        .args(["--disable-internal-agent", manager])
        .args(*args);
    Ok(command)
}

#[cfg(windows)]
fn install_command() -> Result<Command> {
    let mut command = Command::new("winget.exe");
    command.args([
        "install",
        "--id",
        "oschwartz10612.Poppler",
        "--exact",
        "--source",
        "winget",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]);
    Ok(command)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn install_command() -> Result<Command> {
    Err("Install pdftotext on this platform, then retry the PDF.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_keeps_original_page_numbers() {
        assert_eq!(
            numbered_pages("First page\x0c\x0cThird page\x0c").unwrap(),
            "[PDF page 1]\nFirst page\n\n[PDF page 2]\n[No text layer]\n\n[PDF page 3]\nThird page\n\n"
        );
        assert!(numbered_pages(" \x0c\x0c")
            .unwrap_err()
            .to_string()
            .contains("OCR"));
    }

    #[tokio::test]
    async fn rejects_non_pdf_before_run_or_install() {
        let error = prepare(b"plain text".to_vec()).await.unwrap_err();
        assert!(error.to_string().contains("PDF header"));
    }
}
