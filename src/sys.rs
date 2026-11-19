//! Platform differences for files and processes, kept in one place.
//!
//! On Unix these are the std traits and libc values. On Windows, Unix file
//! modes have no meaning: files under the user's profile are already private
//! to that user through the profile's default ACL, so modes are ignored and
//! permission checks see a private file.
#[cfg(unix)]
pub(crate) use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
pub(crate) const O_NOFOLLOW: i32 = libc::O_NOFOLLOW;
#[cfg(unix)]
pub(crate) const O_NONBLOCK: i32 = libc::O_NONBLOCK;

#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(windows)]
mod windows {
    // shortcut: Windows opens don't refuse symlinks here; upgrade to
    // FILE_FLAG_OPEN_REPARSE_POINT checks if Hibiscus data moves out of the profile.
    pub(crate) const O_NOFOLLOW: i32 = 0;
    pub(crate) const O_NONBLOCK: i32 = 0;

    pub(crate) trait OpenOptionsExt {
        fn mode(&mut self, mode: u32) -> &mut Self;
        fn custom_flags(&mut self, flags: i32) -> &mut Self;
    }
    impl OpenOptionsExt for std::fs::OpenOptions {
        fn mode(&mut self, _: u32) -> &mut Self {
            self
        }
        fn custom_flags(&mut self, _: i32) -> &mut Self {
            self
        }
    }
    pub(crate) trait DirBuilderExt {
        fn mode(&mut self, mode: u32) -> &mut Self;
    }
    impl DirBuilderExt for std::fs::DirBuilder {
        fn mode(&mut self, _: u32) -> &mut Self {
            self
        }
    }
    pub(crate) trait PermissionsExt {
        fn mode(&self) -> u32;
    }
    impl PermissionsExt for std::fs::Permissions {
        /// Owner-only, so private-file checks pass. Not an executable bit:
        /// see [`super::executable`].
        fn mode(&self) -> u32 {
            if self.readonly() {
                0o400
            } else {
                0o600
            }
        }
    }
}

/// Whether a regular file can be run directly: the execute bit on Unix, the
/// file extension on Windows.
pub(crate) fn executable(path: &std::path::Path, metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        let _ = path;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        metadata.is_file()
            && path.extension().is_some_and(|e| {
                ["exe", "cmd", "bat", "com"]
                    .iter()
                    .any(|x| e.eq_ignore_ascii_case(x))
            })
    }
}

/// This computer's name, for labelling a signed-in device.
pub(crate) fn hostname() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: the buffer outlives the call and its length is passed.
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }
    #[cfg(windows)]
    {
        std::env::var("COMPUTERNAME").ok()
    }
}

/// A command that runs `script` in the platform shell: `/bin/sh` on Unix. On
/// Windows, Git Bash when it is installed, so POSIX commands keep working,
/// else PowerShell. System32's `bash.exe` is the WSL launcher and is skipped.
pub(crate) fn shell(script: &str) -> std::process::Command {
    let mut command = base_shell(script);
    // Once the video engine is installed, Node.js, hyperframes and ffmpeg are on PATH.
    crate::video_engine::prepare(&mut command);
    command
}

fn base_shell(script: &str) -> std::process::Command {
    #[cfg(unix)]
    {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
    #[cfg(windows)]
    {
        if let Some(bash) = git_bash() {
            let mut command = std::process::Command::new(bash);
            command.args(["-c", script]);
            command
        } else {
            let mut command = std::process::Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            command
        }
    }
}

/// The bash tool's description, so the model writes for the shell [`shell`] runs.
pub(crate) fn shell_description() -> &'static str {
    #[cfg(unix)]
    {
        "Run a POSIX /bin/sh command in the workspace. Output and duration are bounded."
    }
    #[cfg(windows)]
    {
        if git_bash().is_some() {
            "Run a bash command (Git Bash on Windows) in the workspace. Output and duration are bounded."
        } else {
            "Run a Windows PowerShell command in the workspace. POSIX shell syntax does not work here. Output and duration are bounded."
        }
    }
}

#[cfg(windows)]
fn git_bash() -> Option<&'static std::path::Path> {
    static BASH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    BASH.get_or_init(|| {
        ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|v| std::env::var_os(v).map(std::path::PathBuf::from))
            .chain(
                std::env::var_os("LOCALAPPDATA")
                    .map(|d| std::path::PathBuf::from(d).join("Programs")),
            )
            .map(|root| root.join("Git").join("bin").join("bash.exe"))
            .find(|bash| bash.is_file())
    })
    .as_deref()
}

/// Puts a command in its own process group on Unix, so [`ProcessTree`] can
/// stop everything it starts. On Windows it starts without a console window:
/// the desktop app is a GUI process, and each console child would otherwise
/// open a visible one.
pub(crate) fn own_group(command: &mut std::process::Command) {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::os::windows::process::CommandExt::creation_flags(command, CREATE_NO_WINDOW);
    }
}

/// A child process and everything it starts, killed when this is dropped.
/// Unix: the child's own process group (see [`own_group`]). Windows: a Job
/// Object that kills its processes when its last handle closes.
pub(crate) struct ProcessTree(
    #[cfg(unix)] i32,
    #[cfg(windows)] windows_sys::Win32::Foundation::HANDLE,
);

// SAFETY: the job handle is owned here and only used through thread-safe
// Win32 calls; Unix holds a plain PID.
unsafe impl Send for ProcessTree {}
unsafe impl Sync for ProcessTree {}

impl ProcessTree {
    /// Takes the child's PID right after spawning it.
    #[cfg(unix)]
    pub(crate) fn new(pid: u32) -> Self {
        Self(pid as i32)
    }
    // shortcut: processes the child starts before it joins the job escape it;
    // upgrade to CREATE_SUSPENDED + resume if that shows up in practice.
    #[cfg(windows)]
    pub(crate) fn new(pid: u32) -> Self {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                },
                Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
            },
        };
        // SAFETY: plain Win32 calls on handles created and closed here.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Self(job);
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                AssignProcessToJobObject(job, process);
                CloseHandle(process);
            }
            Self(job)
        }
    }
    /// Asks the tree to stop: SIGTERM on Unix. Windows has no polite signal
    /// for console processes, so this kills.
    pub(crate) fn terminate(&self) {
        #[cfg(unix)]
        // SAFETY: signals the group created at spawn.
        unsafe {
            libc::kill(-self.0, libc::SIGTERM);
        }
        #[cfg(windows)]
        self.kill();
    }
    pub(crate) fn kill(&self) {
        #[cfg(unix)]
        // SAFETY: negative PID targets the group, including descendants that
        // kept output pipes open.
        unsafe {
            libc::kill(-self.0, libc::SIGKILL);
        }
        #[cfg(windows)]
        if !self.0.is_null() {
            // SAFETY: the job handle is live until drop.
            unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1) };
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.kill();
        #[cfg(windows)]
        if !self.0.is_null() {
            // SAFETY: closed once, here.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}

/// Flushes a directory entry after a rename into it. Windows journals renames
/// on NTFS and can't open a directory as a plain file, so this is Unix only.
pub(crate) fn sync_dir(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)?.sync_all()
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn shell_runs_a_command() {
        let output = shell("echo hibiscus").output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("hibiscus"));
    }

    #[test]
    fn dropping_a_process_tree_stops_the_child() {
        // `sleep` is POSIX and also a PowerShell alias for Start-Sleep.
        let mut command = shell("sleep 30");
        own_group(&mut command);
        let mut child = command.spawn().unwrap();
        drop(ProcessTree::new(child.id()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "child survived its process tree");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn executables_are_recognised() {
        let exe = std::env::current_exe().unwrap();
        assert!(executable(&exe, &std::fs::metadata(&exe).unwrap()));
        let source = std::path::Path::new(file!());
        if let Ok(metadata) = std::fs::metadata(source) {
            assert!(!executable(source, &metadata));
        }
    }
}
