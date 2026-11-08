//! Engine handle for embedding Hibiscus without the terminal UI (the desktop
//! app). Commands and events are the engine's JSON records; the transport keeps
//! the TUI's byte/record limits, write deadlines and shutdown behaviour.
use super::bridge::{spawn_engine, SessionStart, SocketReader};
use super::transport::{Events, Inbox, Limits, PipeWriter};
use crate::Result;
use serde_json::Value;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{mpsc::RecvTimeoutError, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Which session the engine opens.
pub enum Start {
    New,
    /// The most recent session for the workspace, or a new one.
    Latest,
    Selected(PathBuf),
}

pub struct Engine {
    worker: Option<JoinHandle<()>>,
    control: UnixStream,
    input: Option<PipeWriter<UnixStream>>,
    output: Inbox,
    next_id: u64,
    failure: Arc<Mutex<Option<String>>>,
}

impl Engine {
    /// Starts an engine whose session and tools work in `workspace`.
    pub fn start(start: Start, workspace: &Path) -> Result<Self> {
        let start = match start {
            Start::New => SessionStart::New,
            Start::Latest => SessionStart::Latest,
            Start::Selected(path) => SessionStart::Selected(path),
        };
        let failure = Arc::new(Mutex::new(None));
        let record = failure.clone();
        let (worker, client) = spawn_engine(&start, workspace, move |error| {
            *record.lock().unwrap() = Some(error);
        })?;
        let control = client.try_clone()?;
        let output = Inbox::start(SocketReader(client.try_clone()?), Limits::from_env()?);
        let input = output.writer(client)?;
        Ok(Self {
            worker: Some(worker),
            control,
            input: Some(input),
            output,
            next_id: 0,
            failure,
        })
    }

    /// Sends one command object such as `{"type":"prompt","message":"…"}`.
    /// Its `id` is replaced; the returned id matches the engine's `response`.
    pub fn send(&mut self, mut command: Value) -> Result<String> {
        if !command.get("type").is_some_and(Value::is_string) {
            return Err("command must be an object with a string type".into());
        }
        self.next_id += 1;
        let id = format!("desktop-{}", self.next_id);
        command["id"] = id.clone().into();
        let mut line = serde_json::to_vec(&command)?;
        line.push(b'\n');
        let input = self.input.as_mut().ok_or("engine is shut down")?;
        input.write_all(&line)?;
        input.flush()?;
        Ok(id)
    }

    /// Next engine event, or `None` if none arrives within `timeout`.
    /// Fails once the engine has stopped.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<Value>> {
        match self.output.recv_timeout(timeout) {
            Ok(event) => event.map(Some),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(self
                .failure
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "engine stopped".into())
                .into()),
        }
    }

    /// Stops the engine and waits for its running tools to stop.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.input.take();
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}
