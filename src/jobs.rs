//! Commands the agent runs in the background (`bash` with `background: true`)
//! in the desktop app. They keep running between tool calls, show in the
//! app's Background tasks pane, and stop when the app quits.
use crate::devserver::{tail, Output, Process};
use crate::Result;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// Finished jobs kept for the pane, oldest dropped first.
const KEEP_FINISHED: usize = 50;

struct Job {
    id: u64,
    folder: PathBuf,
    label: String,
    command: String,
    started: SystemTime,
    clock: Instant,
    /// How long it ran, once it has stopped.
    took: Option<std::time::Duration>,
    exit_code: Option<i32>,
    /// Set by `stop`, so the pane says it was stopped rather than failed.
    stopped: bool,
    /// Outlives the process, so a finished job still shows what it printed.
    output: Arc<Mutex<Output>>,
    process: Option<Process>,
}

static JOBS: Mutex<(u64, Vec<Job>)> = Mutex::new((0, Vec::new()));

/// A job as the agent and the pane see it.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: u64,
    pub folder: PathBuf,
    pub label: String,
    pub command: String,
    /// Milliseconds since the Unix epoch.
    pub started: u64,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub stopped: bool,
    /// How long it has run, or ran.
    pub duration_ms: u64,
    pub output: Vec<String>,
}

/// Starts `command` in `folder` and returns its id.
pub fn start(folder: &Path, command: &str, label: &str) -> Result<u64> {
    let process = Process::spawn(folder, command)?;
    let mut jobs = JOBS.lock().unwrap();
    for job in jobs.1.iter_mut() {
        settle(job);
    }
    let finished = jobs.1.iter().filter(|j| j.took.is_some()).count();
    let mut extra = finished.saturating_sub(KEEP_FINISHED - 1);
    jobs.1.retain(|j| {
        let drop = extra > 0 && j.took.is_some();
        extra -= usize::from(drop);
        !drop
    });
    jobs.0 += 1;
    let id = jobs.0;
    let output = process.output.clone();
    jobs.1.push(Job {
        id,
        folder: folder.to_path_buf(),
        label: label.to_string(),
        command: command.to_string(),
        started: SystemTime::now(),
        clock: Instant::now(),
        took: None,
        exit_code: None,
        stopped: false,
        output,
        process: Some(process),
    });
    Ok(id)
}

/// Jobs in `folder` (all with None), newest first, each with its last `lines` of output.
pub fn list(folder: Option<&Path>, lines: usize) -> Vec<Info> {
    let mut jobs = JOBS.lock().unwrap();
    jobs.1
        .iter_mut()
        .rev()
        .filter(|j| folder.is_none_or(|f| j.folder == f))
        .map(|j| info(j, lines))
        .collect()
}

pub fn get(id: u64, lines: usize) -> Option<Info> {
    let mut jobs = JOBS.lock().unwrap();
    jobs.1
        .iter_mut()
        .find(|j| j.id == id)
        .map(|j| info(j, lines))
}

/// Stops a running job. False when there is no such job.
pub fn stop(id: u64) -> bool {
    let process = {
        let mut jobs = JOBS.lock().unwrap();
        let Some(job) = jobs.1.iter_mut().find(|j| j.id == id) else {
            return false;
        };
        settle(job);
        if job.took.is_none() {
            job.stopped = true;
            job.took = Some(job.clock.elapsed());
        }
        job.process.take()
    };
    if let Some(process) = process {
        process.end();
    }
    true
}

/// Forgets finished jobs in `folder`.
pub fn clear(folder: &Path) {
    let mut jobs = JOBS.lock().unwrap();
    for job in jobs.1.iter_mut() {
        settle(job);
    }
    jobs.1.retain(|j| j.folder != folder || j.took.is_none());
}

/// Stops every job, when the app quits.
pub fn stop_all() {
    let ids: Vec<u64> = JOBS.lock().unwrap().1.iter().map(|j| j.id).collect();
    for id in ids {
        stop(id);
    }
}

/// Notes the exit of a job that stopped by itself, and drops old finished jobs.
fn settle(job: &mut Job) {
    if job.took.is_some() {
        return;
    }
    if let Some(status) = job
        .process
        .as_mut()
        .and_then(|p| p.child.try_wait().ok().flatten())
    {
        job.took = Some(job.clock.elapsed());
        job.exit_code = status.code();
    }
}

fn info(job: &mut Job, lines: usize) -> Info {
    settle(job);
    Info {
        id: job.id,
        folder: job.folder.clone(),
        label: job.label.clone(),
        command: job.command.clone(),
        started: job
            .started
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        running: job.took.is_none(),
        exit_code: job.exit_code,
        stopped: job.stopped,
        duration_ms: job.took.unwrap_or_else(|| job.clock.elapsed()).as_millis() as u64,
        output: tail(&job.output, lines),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn runs_reports_and_stops_a_background_job() {
        let dir = std::env::temp_dir();
        let done = start(&dir, "echo hello", "Say hello").unwrap();
        let long = start(&dir, "echo started; sleep 30", "Wait").unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while get(done, 10).unwrap().running || get(long, 10).unwrap().output.is_empty() {
            assert!(Instant::now() < deadline, "jobs never reported");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let finished = get(done, 10).unwrap();
        assert_eq!(finished.label, "Say hello");
        assert_eq!(finished.exit_code, Some(0));
        assert_eq!(finished.output, vec!["hello"]);
        assert!(get(long, 10).unwrap().running);

        assert!(stop(long));
        let stopped = get(long, 10).unwrap();
        assert!(!stopped.running && stopped.stopped);
        assert_eq!(stopped.output, vec!["started"]);
        assert!(!stop(9_999_999));

        clear(&dir);
        assert!(get(done, 10).is_none() && get(long, 10).is_none());
    }
}
