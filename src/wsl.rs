//! WSL 2 connections for the Windows desktop app (#201).
//!
//! The app finds WSL 2 distros by itself, and the user connects a project
//! folder to one. While connected, the shell tool and the dev server run inside
//! that distro through `wsl.exe`, and Linux paths map to Windows ones. Parsing
//! and path mapping are plain functions so they are tested on every platform.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A WSL 2 distro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Distro {
    pub name: String,
    /// The distro `wsl.exe` uses when none is named.
    pub default: bool,
}

/// Docker Desktop's own distros, which are not for user work.
#[cfg_attr(not(windows), allow(dead_code))] // Detection only runs on Windows.
fn internal(name: &str) -> bool {
    name.eq_ignore_ascii_case("docker-desktop") || name.eq_ignore_ascii_case("docker-desktop-data")
}

/// Reads `wsl.exe --list --verbose`, which prints UTF-16. Keeps WSL 2 distros
/// only. The header row is localized, so rows are recognized by their last
/// column being a version number, not by the header's words.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_list(bytes: &[u8]) -> Vec<Distro> {
    let utf16 = bytes.len().is_multiple_of(2) && bytes.iter().skip(1).step_by(2).any(|b| *b == 0);
    let text = if utf16 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches('\u{feff}').trim_matches('\0');
            let default = line.trim_start().starts_with('*');
            let fields: Vec<_> = line
                .trim_start()
                .trim_start_matches('*')
                .split_whitespace()
                .collect();
            let [name, .., version] = fields.as_slice() else {
                return None;
            };
            (fields.len() >= 3 && *version == "2" && !internal(name)).then(|| Distro {
                name: (*name).to_owned(),
                default,
            })
        })
        .collect()
}

/// The Linux path for a Windows path, and the distro it belongs to when it is
/// inside one (`\\wsl.localhost\Ubuntu\home\me` is `/home/me` in Ubuntu).
/// Drive paths map to `/mnt/<drive>`. Other paths have no Linux form.
pub(crate) fn to_linux(path: &str) -> Option<(Option<String>, String)> {
    let path = path.replace('/', "\\");
    let path = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    };
    let unc = path
        .strip_prefix(r"\\")
        .and_then(|rest| rest.split_once('\\'))
        .filter(|(host, _)| {
            host.eq_ignore_ascii_case("wsl.localhost") || host.eq_ignore_ascii_case("wsl$")
        });
    if let Some((_, rest)) = unc {
        let mut parts = rest.split('\\').filter(|p| !p.is_empty());
        let distro = parts.next()?.to_owned();
        let linux: Vec<_> = parts.collect();
        return Some((Some(distro), format!("/{}", linux.join("/"))));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let rest: Vec<_> = path[2..].split('\\').filter(|p| !p.is_empty()).collect();
        let drive = (bytes[0] as char).to_ascii_lowercase();
        return Some((
            None,
            format!("/mnt/{drive}/{}", rest.join("/"))
                .trim_end_matches('/')
                .to_owned(),
        ));
    }
    None
}

/// The Windows path for a Linux path in `distro`: `/mnt/c/x` is `C:\x`, and
/// anything else is under `\\wsl.localhost\<distro>`.
pub(crate) fn to_windows(distro: &str, linux: &str) -> String {
    let parts: Vec<_> = linux.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        ["mnt", drive, rest @ ..]
            if drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() =>
        {
            format!("{}:\\{}", drive.to_ascii_uppercase(), rest.join("\\"))
        }
        _ => format!(r"\\wsl.localhost\{distro}\{}", parts.join("\\")),
    }
}

/// Folders connected to a distro, set by the desktop app.
static CONNECTED: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// Connects `folder` to `distro`, or disconnects it with `None`.
pub fn connect(folder: &Path, distro: Option<String>) {
    let mut connected = CONNECTED.lock().unwrap();
    connected.retain(|(f, _)| f != folder);
    if let Some(distro) = distro.filter(|d| !d.is_empty()) {
        connected.push((folder.to_owned(), distro));
    }
}

/// The distro `path` runs in: that of the closest connected folder holding it.
pub(crate) fn distro_for(path: &Path) -> Option<String> {
    CONNECTED
        .lock()
        .unwrap()
        .iter()
        .filter(|(folder, _)| path.starts_with(folder))
        .max_by_key(|(folder, _)| folder.as_os_str().len())
        .map(|(_, distro)| distro.clone())
}

/// The bash tool's description while connected.
pub(crate) fn description(distro: &str) -> String {
    format!(
        "Run a bash command in WSL ({distro}) in the workspace. Paths are Linux paths; Windows drives are under /mnt. Output and duration are bounded."
    )
}

/// One command running in a distro. Dropping it kills the command's whole
/// process group inside Linux, as process groups do for Unix shells: killing
/// `wsl.exe` alone can leave the Linux side running.
pub(crate) struct Run {
    distro: String,
    id: String,
}

impl Run {
    pub(crate) fn new(distro: String) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            distro,
            id: format!("{}-{n}", std::process::id()),
        }
    }

    /// `wsl.exe` running `script` in bash at `cwd`. The script runs in its own
    /// session (`setsid`), whose PID is written to a file so [`Drop`] can kill
    /// the group.
    pub(crate) fn command(&self, cwd: &Path, script: &str) -> crate::Result<std::process::Command> {
        let (owner, linux) =
            to_linux(&cwd.to_string_lossy()).ok_or("This folder has no path inside WSL")?;
        if let Some(owner) = owner.filter(|o| !o.eq_ignore_ascii_case(&self.distro)) {
            return Err(format!(
                "This folder is in the WSL distro {owner}; connect to {owner} instead"
            )
            .into());
        }
        let mut command = std::process::Command::new("wsl.exe");
        command.args(["-d", &self.distro, "--cd", &linux, "--exec", "bash", "-c"]).arg(
            r#"setsid bash -lc "$1" & p=$!; printf %s "$p" > "/tmp/hibiscus-$2.pid"; wait "$p""#,
        );
        command.args(["hibiscus", script, &self.id]);
        Ok(command)
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let mut command = std::process::Command::new("wsl.exe");
        command
            .args(["-d", &self.distro, "--exec", "sh", "-c"])
            .arg(r#"f="/tmp/hibiscus-$1.pid"; [ -f "$f" ] && kill -KILL -- "-$(cat "$f")" 2>/dev/null; rm -f "$f""#)
            .args(["sh", &self.id])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::sys::own_group(&mut command);
        if let Ok(mut child) = command.spawn() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// Runs `wsl.exe` with `args` and returns its output, or nothing when it
/// fails or takes longer than 10 seconds (a broken WSL must not hang the app).
#[cfg(windows)]
fn wsl(args: &[&str]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut command = std::process::Command::new("wsl.exe");
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::sys::own_group(&mut command);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => return reader.join().ok()?.ok(),
            Some(_) => return None,
            None if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}

/// The WSL 2 distros on this computer; empty when WSL is missing, and always
/// empty outside Windows. Falls back to the registry when `wsl.exe` fails.
pub fn detect() -> Vec<Distro> {
    #[cfg(windows)]
    {
        let found = wsl(&["--list", "--verbose"])
            .map(|bytes| parse_list(&bytes))
            .unwrap_or_default();
        if found.is_empty() {
            registry()
        } else {
            found
        }
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The distro's home folder as a Windows path, for the Add folder dialog.
pub fn home(distro: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let bytes = wsl(&["-d", distro, "--exec", "sh", "-c", r#"printf %s "$HOME""#])?;
        let home = String::from_utf8(bytes).ok()?;
        home.starts_with('/')
            .then(|| to_windows(distro, home.trim()))
    }
    #[cfg(not(windows))]
    {
        let _ = distro;
        None
    }
}

/// WSL's own record of installed distros, under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss`.
#[cfg(windows)]
fn registry() -> Vec<Distro> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    // SAFETY: plain registry reads into buffers that outlive each call; the
    // key opened here is closed before returning.
    unsafe {
        let string = |key: HKEY, sub: &[u16], name: &str| -> Option<String> {
            let mut buf = [0u16; 512];
            let mut len = (buf.len() * 2) as u32;
            let status = RegGetValueW(
                key,
                sub.as_ptr(),
                wide(name).as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut len,
            );
            (status == 0).then(|| {
                let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..end])
            })
        };
        let mut root: HKEY = std::ptr::null_mut();
        let path = wide(r"Software\Microsoft\Windows\CurrentVersion\Lxss");
        if RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut root) != 0 {
            return Vec::new();
        }
        let default = string(root, &[0], "DefaultDistribution");
        let mut found = Vec::new();
        for index in 0.. {
            let mut name = [0u16; 128];
            let mut len = name.len() as u32;
            let status = RegEnumKeyExW(
                root,
                index,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if status != 0 {
                break;
            }
            let sub: Vec<u16> = name[..len as usize].iter().copied().chain([0]).collect();
            let mut version = 0u32;
            let mut size = 4u32;
            let version_status = RegGetValueW(
                root,
                sub.as_ptr(),
                wide("Version").as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut version as *mut u32).cast(),
                &mut size,
            );
            let Some(distro) = string(root, &sub, "DistributionName") else {
                continue;
            };
            if version_status == 0 && version == 2 && !internal(&distro) {
                let guid = String::from_utf16_lossy(&name[..len as usize]);
                found.push(Distro {
                    name: distro,
                    default: default.as_deref() == Some(guid.as_str()),
                });
            }
        }
        RegCloseKey(root);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        [0xfeff_u16]
            .into_iter()
            .chain(text.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[test]
    fn list_keeps_wsl2_distros_marks_the_default_and_skips_docker() {
        let output = "  NAME                   STATE           VERSION\r\n\
                      * Ubuntu                 Running         2\r\n\
                      \x20 Debian                 Stopped         2\r\n\
                      \x20 Legacy                 Stopped         1\r\n\
                      \x20 docker-desktop         Running         2\r\n\
                      \x20 docker-desktop-data    Running         2\r\n";
        let expected = vec![
            Distro {
                name: "Ubuntu".into(),
                default: true,
            },
            Distro {
                name: "Debian".into(),
                default: false,
            },
        ];
        assert_eq!(parse_list(&utf16(output)), expected);
        assert_eq!(parse_list(output.as_bytes()), expected);
        // Localized headers (here German) are not mistaken for a distro.
        let german =
            "  NAME      STATUS      VERSION\r\n* Ubuntu-22.04    Wird ausgeführt    2\r\n";
        assert_eq!(
            parse_list(&utf16(german)),
            vec![Distro {
                name: "Ubuntu-22.04".into(),
                default: true
            }]
        );
        assert!(parse_list(&utf16(
            "Windows Subsystem for Linux has no installed distributions.\r\n"
        ))
        .is_empty());
        assert!(parse_list(b"").is_empty());
    }

    #[test]
    fn paths_map_between_windows_and_linux() {
        let ubuntu = |p: &str| Some((Some("Ubuntu".to_owned()), p.to_owned()));
        assert_eq!(
            to_linux(r"\\wsl.localhost\Ubuntu\home\me\app"),
            ubuntu("/home/me/app")
        );
        assert_eq!(to_linux(r"\\wsl$\Ubuntu\home\me\"), ubuntu("/home/me"));
        assert_eq!(
            to_linux(r"\\?\UNC\wsl.localhost\Ubuntu\home\me"),
            ubuntu("/home/me")
        );
        assert_eq!(to_linux(r"\\wsl.localhost\Ubuntu"), ubuntu("/"));
        assert_eq!(
            to_linux(r"C:\Users\me\app"),
            Some((None, "/mnt/c/Users/me/app".into()))
        );
        assert_eq!(
            to_linux(r"\\?\D:\work\"),
            Some((None, "/mnt/d/work".into()))
        );
        assert_eq!(to_linux(r"\\server\share\x"), None);
        assert_eq!(to_linux("relative"), None);

        assert_eq!(
            to_windows("Ubuntu", "/home/me/app"),
            r"\\wsl.localhost\Ubuntu\home\me\app"
        );
        assert_eq!(to_windows("Ubuntu", "/mnt/c/Users/me"), r"C:\Users\me");
        assert_eq!(
            to_windows("Ubuntu", "/mnt/data/x"),
            r"\\wsl.localhost\Ubuntu\mnt\data\x"
        );
        for path in [r"\\wsl.localhost\Ubuntu\home\me\app", r"C:\Users\me"] {
            let (_, linux) = to_linux(path).unwrap();
            assert_eq!(to_windows("Ubuntu", &linux), path);
        }
    }

    #[test]
    fn connected_folders_cover_their_subfolders_until_disconnected() {
        let root = Path::new("/wsl-test/project");
        connect(root, Some("Ubuntu".into()));
        assert_eq!(distro_for(&root.join("src")), Some("Ubuntu".into()));
        assert_eq!(distro_for(Path::new("/wsl-test/other")), None);
        connect(root, None);
        assert_eq!(distro_for(root), None);
    }

    #[test]
    fn commands_refuse_a_folder_in_another_distro() {
        let run = Run {
            distro: "Ubuntu".into(),
            id: "test".into(),
        };
        assert!(run
            .command(Path::new(r"\\wsl.localhost\Debian\home\me"), "ls")
            .is_err());
        let command = run
            .command(Path::new(r"\\wsl.localhost\Ubuntu\home\me"), "ls")
            .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[..4], ["-d", "Ubuntu", "--cd", "/home/me"]);
        assert_eq!(args[args.len() - 2..], ["ls", "test"]);
        // Don't run the cleanup in this test; it would call wsl.exe.
        std::mem::forget(run);
    }
}
