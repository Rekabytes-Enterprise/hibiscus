//! Bounded subprocess execution shared by native shell tools and extensions.
use std::{
    io,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::watch,
};

pub(crate) const OUTPUT_LIMIT: usize = 65536;
pub(crate) struct Output {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}
#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    OutputLimit(usize),
    TimedOut,
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Process I/O failed: {e}"),
            Self::OutputLimit(limit) => write!(
                f,
                "Process output exceeded {} KiB; command group terminated",
                limit / 1024
            ),
            Self::TimedOut => f.write_str("Process timed out; command group terminated"),
            Self::Cancelled => f.write_str("Process cancelled; command group terminated"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
async fn read(stream: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    stream
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(Error::OutputLimit(limit));
    }
    Ok(bytes)
}
pub(crate) async fn run(
    command: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Output, Error> {
    run_with_stdout_limit(command, input, timeout, cancel, OUTPUT_LIMIT).await
}

pub(crate) async fn run_with_stdout_limit(
    mut command: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
    cancel: &mut watch::Receiver<bool>,
    stdout_limit: usize,
) -> Result<Output, Error> {
    if *cancel.borrow() {
        return Err(Error::Cancelled);
    }
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::sys::own_group(command.as_std_mut());
    let mut child = command.spawn()?;
    let group = crate::sys::ProcessTree::new(
        child
            .id()
            .ok_or_else(|| io::Error::other("Process PID unavailable"))?,
    );
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Process stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Process stderr unavailable"))?;
    let running = async {
        let writing = async {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin.write_all(&input).await?;
                stdin.shutdown().await?;
            }
            Ok::<_, Error>(())
        };
        let waiting = async { child.wait().await.map_err(Error::from) };
        let (_, stdout, stderr, status) = tokio::try_join!(
            writing,
            read(stdout, stdout_limit),
            read(stderr, OUTPUT_LIMIT),
            waiting
        )?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    };
    let result = tokio::select! {
        result = tokio::time::timeout(timeout, running) => result.unwrap_or(Err(Error::TimedOut)),
        _ = cancel.changed() => Err(Error::Cancelled),
    };
    drop(group);
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}
