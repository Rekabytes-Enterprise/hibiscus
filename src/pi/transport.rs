//! Bounded RPC ingress and deadline-controlled duplex writes. The reader never
//! blocks on a full mailbox: overload invalidates the connection explicitly.
use super::error;
use crate::Result;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{value::RawValue, Value};
use std::{
    collections::VecDeque,
    io::{self, BufRead, BufReader, Read, Write},
    os::fd::AsRawFd,
    sync::{mpsc::RecvTimeoutError, Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

const SEND_FAILURE: &str = "Pi RPC stream closed or receive limit/failure interrupted command delivery; outcome is uncertain";

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub bytes: usize,
    pub records: usize,
}
impl Limits {
    pub fn from_env() -> Result<Self> {
        fn setting(name: &str, default: usize, max: usize) -> Result<usize> {
            match std::env::var(name) {
                Err(std::env::VarError::NotPresent) => Ok(default),
                Ok(value) => value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=max).contains(n))
                    .ok_or_else(|| format!("{name} must be an integer from 1 to {max}").into()),
                Err(_) => Err(format!("{name} must be valid UTF-8").into()),
            }
        }
        Ok(Self {
            bytes: setting("HIBISCUS_RPC_BUFFER_MIB", 64, 512)? * 1024 * 1024,
            records: setting("HIBISCUS_RPC_MAX_RECORDS", 4096, 65536)?,
        })
    }
}

#[derive(Default)]
struct State {
    frames: VecDeque<Vec<u8>>,
    bytes: usize,
    peak_bytes: usize,
    peak_records: usize,
    closed: bool,
    failure: Option<&'static str>,
    reported: bool,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}
impl Shared {
    fn fail(&self, message: &'static str) {
        let mut state = self.state.lock().unwrap();
        if state.failure.is_none() {
            state.failure = Some(message);
        }
        state.closed = true;
        state.frames.clear();
        state.bytes = 0;
        self.ready.notify_all();
    }
    fn push(&self, frame: Vec<u8>, limits: Limits) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return false;
        }
        // Account allocated capacity, not just payload length.
        if frame.capacity() > limits.bytes.saturating_sub(state.bytes)
            || state.frames.len() >= limits.records
        {
            drop(state);
            self.fail("Pi RPC backlog limit exceeded; connection outcome is uncertain. Adjust HIBISCUS_RPC_BUFFER_MIB / HIBISCUS_RPC_MAX_RECORDS only for trusted workloads");
            return false;
        }
        state.bytes += frame.capacity();
        state.frames.push_back(frame);
        state.peak_bytes = state.peak_bytes.max(state.bytes);
        state.peak_records = state.peak_records.max(state.frames.len());
        self.ready.notify_one();
        true
    }
}

pub(super) trait Events {
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Result<Value>, RecvTimeoutError>;
}
#[cfg(test)]
impl Events for std::sync::mpsc::Receiver<Result<Value>> {
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Result<Value>, RecvTimeoutError> {
        std::sync::mpsc::Receiver::recv_timeout(self, timeout)
    }
}

pub(super) struct Inbox {
    shared: Arc<Shared>,
}
impl Inbox {
    pub fn start(reader: impl Read + Send + 'static, limits: Limits) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            loop {
                if worker.state.lock().unwrap().closed {
                    break;
                }
                match read_frame(&mut reader, limits.bytes) {
                    Ok(Some(frame)) => {
                        if !worker.push(frame, limits) {
                            break;
                        }
                    }
                    Ok(None) => {
                        worker.state.lock().unwrap().closed = true;
                        worker.ready.notify_all();
                        break;
                    }
                    Err(message) => {
                        worker.fail(message);
                        break;
                    }
                }
            }
        });
        Self { shared }
    }
    pub fn failed(&self) -> bool {
        self.shared.state.lock().unwrap().failure.is_some()
    }
    pub fn closed_cleanly(&self) -> bool {
        let state = self.shared.state.lock().unwrap();
        state.closed && state.failure.is_none()
    }
    pub fn writer<W: Write + AsRawFd>(&self, pipe: W) -> io::Result<PipeWriter<W>> {
        let fd = pipe.as_raw_fd();
        // SAFETY: this is the owned stdin write end, not a cloned terminal fd.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(PipeWriter {
            pipe,
            shared: self.shared.clone(),
            deadline: None,
            timeout: Duration::from_secs(30),
            last_service: None,
        })
    }
}
impl Events for Inbox {
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Result<Value>, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.state.lock().unwrap();
        loop {
            if let Some(message) = state.failure.filter(|_| !state.reported) {
                state.reported = true;
                return Ok(Err(error::transport(message).into()));
            }
            if let Some(frame) = state.frames.pop_front() {
                state.bytes -= frame.capacity();
                drop(state);
                // Only the consumer builds a JSON tree. Queued frames remain
                // compact bytes; never print payloads in parse diagnostics.
                let result = decode_record(&frame)
                    .map_err(|_| error::protocol("Invalid Pi RPC JSON record").into());
                if result.is_err() {
                    self.shared.fail("Invalid Pi RPC JSON record");
                }
                return Ok(result);
            }
            if state.closed {
                return Err(RecvTimeoutError::Disconnected);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(RecvTimeoutError::Timeout);
            }
            state = self.shared.ready.wait_timeout(state, remaining).unwrap().0;
        }
    }
}
// Unlike IgnoredAny, this walks strings through serde_json's normal escape
// decoder (including surrogate validation), but never retains array elements.
struct ValidateValue;
impl<'de> DeserializeSeed<'de> for ValidateValue {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        struct Walk;
        impl<'de> Visitor<'de> for Walk {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("valid JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_string<E: serde::de::Error>(self, _: String) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<(), D::Error> {
                d.deserialize_any(Walk)
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> std::result::Result<(), S::Error> {
                while seq.next_element_seed(ValidateValue)?.is_some() {}
                Ok(())
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<(), M::Error> {
                while map.next_key::<String>()?.is_some() {
                    map.next_value_seed(ValidateValue)?;
                }
                Ok(())
            }
        }
        deserializer.deserialize_any(Walk)
    }
}

fn validate_raw(raw: &RawValue) -> serde_json::Result<()> {
    let mut validator = serde_json::Deserializer::from_str(raw.get());
    ValidateValue.deserialize(&mut validator)?;
    validator.end()
}

// User message_start is the delivery signal. Hibiscus reads only user text
// and image block counts; Pi remains responsible for storing the full image.
// Other message roles/types and all unknown blocks retain their original Value.
fn decode_user_echo(raw: &RawValue) -> serde_json::Result<Value> {
    struct Echo(Value);
    impl<'de> serde::Deserialize<'de> for Echo {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
            struct EchoVisitor;
            impl<'de> Visitor<'de> for EchoVisitor {
                type Value = Echo;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a message object")
                }
                fn visit_map<M: MapAccess<'de>>(
                    self,
                    mut map: M,
                ) -> std::result::Result<Self::Value, M::Error> {
                    let mut fields = serde_json::Map::new();
                    let mut content = None::<&RawValue>;
                    while let Some(key) = map.next_key::<String>()? {
                        if key == "content" {
                            let raw: &RawValue = map.next_value()?;
                            validate_raw(raw).map_err(serde::de::Error::custom)?;
                            content = Some(raw);
                            fields.remove("content");
                        } else {
                            fields.insert(key, map.next_value()?);
                        }
                    }
                    if let Some(raw) = content {
                        let parsed = if fields.get("role").and_then(Value::as_str) == Some("user")
                            && raw.get().trim_start().starts_with('[')
                        {
                            let blocks: Vec<&RawValue> = serde_json::from_str(raw.get())
                                .map_err(serde::de::Error::custom)?;
                            Value::Array(
                                blocks
                                    .into_iter()
                                    .map(|block| decode_user_block(block.get()))
                                    .collect::<serde_json::Result<Vec<_>>>()
                                    .map_err(serde::de::Error::custom)?,
                            )
                        } else {
                            serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?
                        };
                        fields.insert("content".into(), parsed);
                    }
                    Ok(Echo(Value::Object(fields)))
                }
            }
            d.deserialize_any(EchoVisitor)
        }
    }
    // Malformed/non-object messages must still be handled as normal values;
    // the caller's check_record retains authority over event shape.
    if !raw.get().trim_start().starts_with('{') {
        return serde_json::from_str(raw.get());
    }
    serde_json::from_str::<Echo>(raw.get()).map(|echo| echo.0)
}

fn decode_user_block(raw: &str) -> serde_json::Result<Value> {
    struct Block(Value);
    impl<'de> serde::Deserialize<'de> for Block {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
            struct BlockVisitor;
            impl<'de> Visitor<'de> for BlockVisitor {
                type Value = Block;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a content block")
                }
                fn visit_map<M: MapAccess<'de>>(
                    self,
                    mut map: M,
                ) -> std::result::Result<Self::Value, M::Error> {
                    let mut fields = serde_json::Map::new();
                    let mut data = None::<&RawValue>;
                    while let Some(key) = map.next_key::<String>()? {
                        if key == "data" {
                            let raw: &RawValue = map.next_value()?;
                            validate_raw(raw).map_err(serde::de::Error::custom)?;
                            data = Some(raw);
                            fields.remove("data");
                        } else {
                            fields.insert(key, map.next_value()?);
                        }
                    }
                    if let Some(raw) = data {
                        if fields.get("type").and_then(Value::as_str) != Some("image") {
                            fields.insert(
                                "data".into(),
                                serde_json::from_str(raw.get())
                                    .map_err(serde::de::Error::custom)?,
                            );
                        }
                    }
                    Ok(Block(Value::Object(fields)))
                }
            }
            d.deserialize_any(BlockVisitor)
        }
    }
    if !raw.trim_start().starts_with('{') {
        return serde_json::from_str(raw);
    }
    serde_json::from_str::<Block>(raw).map(|block| block.0)
}

/// Retain all Pi event fields except the partial result of a tool update:
/// Hibiscus does not display tool_execution_update, but does consume the
/// correlated start/end, goal details, errors and settlement separately.
/// RawValue captures the skipped region; ValidateValue checks its contents
/// without constructing a potentially huge tree. For
/// every other event, preserve the exact Value semantics, including unknown
/// fields/events. Field order is irrelevant, even when type is last.
fn decode_record(bytes: &[u8]) -> serde_json::Result<Value> {
    struct Decoded(Value);
    impl<'de> serde::Deserialize<'de> for Decoded {
        fn deserialize<D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> std::result::Result<Self, D::Error> {
            struct RecordVisitor;
            impl<'de> Visitor<'de> for RecordVisitor {
                type Value = Decoded;
                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an RPC JSON object")
                }
                fn visit_map<M: MapAccess<'de>>(
                    self,
                    mut input: M,
                ) -> std::result::Result<Self::Value, M::Error> {
                    let mut fields = serde_json::Map::new();
                    let mut partial = None::<&RawValue>;
                    let mut message = None::<&RawValue>;
                    while let Some(key) = input.next_key::<String>()? {
                        if key == "partialResult" {
                            let raw: &RawValue = input.next_value()?;
                            // Validate even earlier duplicate fields; the ordinary
                            // Value decoder validates each occurrence before replacing it.
                            validate_raw(raw).map_err(serde::de::Error::custom)?;
                            partial = Some(raw);
                            fields.remove("partialResult");
                        } else if key == "message" {
                            let raw: &RawValue = input.next_value()?;
                            validate_raw(raw).map_err(serde::de::Error::custom)?;
                            message = Some(raw);
                            fields.remove("message");
                        } else {
                            fields.insert(key, input.next_value()?);
                        }
                    }
                    if let Some(raw) = partial {
                        if fields.get("type").and_then(Value::as_str)
                            != Some("tool_execution_update")
                        {
                            fields.insert(
                                "partialResult".into(),
                                serde_json::from_str(raw.get())
                                    .map_err(serde::de::Error::custom)?,
                            );
                        }
                    }
                    if let Some(raw) = message {
                        let parsed = if fields.get("type").and_then(Value::as_str)
                            == Some("message_start")
                        {
                            decode_user_echo(raw).map_err(serde::de::Error::custom)?
                        } else {
                            serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?
                        };
                        fields.insert("message".into(), parsed);
                    }
                    Ok(Decoded(Value::Object(fields)))
                }
            }
            deserializer.deserialize_map(RecordVisitor)
        }
    }
    serde_json::from_slice::<Decoded>(bytes).map(|decoded| decoded.0)
}

impl Drop for Inbox {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().unwrap();
        state.closed = true;
        state.frames.clear();
        state.bytes = 0;
        if std::env::var("HIBISCUS_RPC_METRICS").as_deref() == Ok("1") {
            eprintln!(
                "hibiscus RPC metrics: peak_queued_bytes={} peak_records={} failed={}",
                state.peak_bytes,
                state.peak_records,
                state.failure.is_some()
            );
        }
        self.shared.ready.notify_all();
    }
}

pub(super) fn read_frame(
    reader: &mut impl BufRead,
    limit: usize,
) -> std::result::Result<Option<Vec<u8>>, &'static str> {
    let mut frame = Vec::new();
    loop {
        let bytes = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("Pi RPC stdout read failed"),
        };
        if bytes.is_empty() {
            return Ok((!frame.is_empty()).then_some(frame));
        }
        let take = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| i + 1);
        if take > limit.saturating_sub(frame.len()) {
            return Err("Pi RPC record limit exceeded; connection outcome is uncertain. Adjust HIBISCUS_RPC_BUFFER_MIB only for trusted large sessions");
        }
        // Geometric growth capped at the record limit, including no-LF streams.
        if frame.capacity() - frame.len() < take {
            let capacity = (frame.capacity().saturating_mul(2))
                .max(frame.len() + take)
                .min(limit);
            frame.reserve_exact(capacity - frame.len());
        }
        frame.extend_from_slice(&bytes[..take]);
        let done = bytes[take - 1] == b'\n';
        reader.consume(take);
        if done {
            return Ok(Some(frame));
        }
    }
}

pub(super) struct PipeWriter<W> {
    pipe: W,
    shared: Arc<Shared>,
    deadline: Option<Instant>,
    timeout: Duration,
    last_service: Option<Instant>,
}
impl<W: Write + AsRawFd> PipeWriter<W> {
    fn write_checked(
        &mut self,
        bytes: &[u8],
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<usize> {
        let deadline = *self
            .deadline
            .get_or_insert_with(|| Instant::now() + self.timeout);
        loop {
            if self.shared.state.lock().unwrap().closed {
                return Err(io::Error::other(SEND_FAILURE));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Pi RPC write timed out; command delivery is uncertain",
                ));
            }
            if self
                .last_service
                .is_none_or(|time| time.elapsed() >= Duration::from_millis(16))
            {
                check()?;
                self.last_service = Some(Instant::now());
            }
            // Serialize the failure transition with this nonblocking write.
            // Never hold the mailbox lock during callbacks or readiness waits.
            let written = {
                let state = self.shared.state.lock().unwrap();
                if state.closed {
                    return Err(io::Error::other(SEND_FAILURE));
                }
                self.pipe.write(&bytes[..bytes.len().min(16 * 1024)])
            };
            match written {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let mut poll = libc::pollfd {
                        fd: self.pipe.as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    // SAFETY: one live owned pipe/socket descriptor. A short
                    // wait services cancellation/failure even without POLLOUT.
                    if unsafe { libc::poll(&mut poll, 1, 10) } < 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() != io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
                }
                result => return result,
            }
        }
    }
}
impl<W: Write + AsRawFd> Write for PipeWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_checked(bytes, &mut || Ok(()))
    }
    fn flush(&mut self) -> io::Result<()> {
        self.pipe.flush()?;
        self.deadline = None;
        Ok(())
    }
}
pub(super) trait WireWrite: Write {
    fn write_serviced(
        &mut self,
        bytes: &[u8],
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<usize> {
        check()?;
        self.write(bytes)
    }
    fn begin_service(&mut self) {}
    fn checked<F: FnMut() -> io::Result<()>>(&mut self, check: F) -> Checked<'_, Self, F>
    where
        Self: Sized,
    {
        self.begin_service();
        Checked {
            writer: self,
            check,
        }
    }
}
impl<W: Write + AsRawFd> WireWrite for PipeWriter<W> {
    fn write_serviced(
        &mut self,
        bytes: &[u8],
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<usize> {
        self.write_checked(bytes, check)
    }
    fn begin_service(&mut self) {
        self.last_service = None;
    }
}
#[cfg(test)]
impl WireWrite for Vec<u8> {}

pub(super) struct Checked<'a, W, F> {
    writer: &'a mut W,
    check: F,
}
impl<W: WireWrite, F: FnMut() -> io::Result<()>> Write for Checked<'_, W, F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writer.write_serviced(bytes, &mut self.check)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::net::UnixStream;
    fn wait_closed(inbox: &Inbox) {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut state = inbox.shared.state.lock().unwrap();
        while !state.closed {
            assert!(
                Instant::now() < deadline,
                "reader must not block on mailbox capacity"
            );
            state = inbox
                .shared
                .ready
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap()
                .0;
        }
    }
    fn empty() -> Inbox {
        Inbox {
            shared: Arc::new(Shared::default()),
        }
    }

    #[test]
    fn slow_consumer_gets_exact_fifo_including_controls_then_eof() {
        let kinds = [
            "response",
            "message_start",
            "queue_update",
            "extension_ui_request",
            "auto_retry_end",
            "agent_settled",
        ];
        let wire = kinds
            .iter()
            .map(|kind| format!("{{\"type\":\"{kind}\",\"text\":\"first\u{2028}second\"}}\r\n"))
            .collect::<String>();
        let inbox = Inbox::start(
            io::Cursor::new(wire),
            Limits {
                bytes: 4096,
                records: 16,
            },
        );
        wait_closed(&inbox);
        assert!(!inbox.failed());
        for kind in kinds {
            let record = inbox.recv_timeout(Duration::ZERO).unwrap().unwrap();
            assert_eq!(record["type"], kind);
            assert_eq!(record["text"], "first\u{2028}second");
        }
        assert!(matches!(
            inbox.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        ));
        assert_eq!(inbox.shared.state.lock().unwrap().bytes, 0);
    }

    #[test]
    fn byte_and_count_overloads_invalidate_instead_of_silent_dropping() {
        for limits in [
            Limits {
                bytes: 4096,
                records: 2,
            },
            Limits {
                bytes: 64,
                records: 100,
            },
        ] {
            let wire = "{\"type\":\"response\",\"id\":\"private-marker\"}\n".repeat(1000);
            let inbox = Inbox::start(io::Cursor::new(wire), limits);
            wait_closed(&inbox);
            let error = inbox
                .recv_timeout(Duration::ZERO)
                .unwrap()
                .unwrap_err()
                .to_string();
            assert!(error.contains("backlog limit"));
            assert!(!error.contains("private-marker"));
            let state = inbox.shared.state.lock().unwrap();
            assert!(state.peak_bytes <= limits.bytes);
            assert!(state.peak_records <= limits.records);
            assert_eq!(state.bytes, 0);
            assert!(state.frames.is_empty());
        }
    }

    #[test]
    fn unused_tool_update_is_validated_without_building_partial_result() {
        for wire in [
            r#"{"type":"tool_execution_update","toolCallId":"call-1","partialResult":{"content":[{"text":"private"}],"details":[1,2,3]}}"#,
            r#"{"partialResult":[1,2,3],"toolCallId":"call-1","type":"tool_execution_update"}"#,
        ] {
            let record = decode_record(wire.as_bytes()).unwrap();
            assert_eq!(record["type"], "tool_execution_update");
            assert_eq!(record["toolCallId"], "call-1");
            assert!(record.get("partialResult").is_none());
        }
        for wire in [
            r#"{"type":"tool_execution_update","partialResult":{"nested":[true,]}}"#,
            r#"{"partialResult": [1, 2,], "type":"tool_execution_update"}"#,
            r#"{"type":"tool_execution_update","partialResult":1} trailing"#,
            r#"{"type":"tool_execution_update","partialResult":"\ud800"}"#,
            r#"{"type":"tool_execution_update","partialResult":"\ud800","partialResult":null}"#,
        ] {
            assert!(
                decode_record(wire.as_bytes()).is_err(),
                "invalid JSON must fail validation: {wire}"
            );
        }
    }

    #[test]
    fn skipped_partial_result_keeps_metadata_and_full_json_validation() {
        let cases = [
            json!({"content":[{"type":"text","text":"escaped \\n and unicode 🪻"}],"details":{"nested":[null,true,1,-2,0.25]}}),
            json!([0, 1, 2, 3, 4]),
            json!({"image":{"data":"opaque-base64","mimeType":"image/png"}}),
        ];
        for partial in cases {
            let wire = format!("{{\"partialResult\":{partial},\"toolCallId\":\"call-1\",\"type\":\"tool_execution_update\",\"args\":{{\"path\":\"a\"}}}}");
            let mut expected: Value = serde_json::from_str(&wire).unwrap();
            expected.as_object_mut().unwrap().remove("partialResult");
            assert_eq!(decode_record(wire.as_bytes()).unwrap(), expected);
        }
        // Invalid values inside ignored content are still protocol failures.
        for value in [
            "1e9999",
            "[1,,2]",
            "{\"data\":\"\\uD800\"}",
            "{\"x\":false \"y\":1}",
        ] {
            let wire = format!("{{\"partialResult\":{value},\"type\":\"tool_execution_update\"}}");
            assert!(decode_record(wire.as_bytes()).is_err(), "{wire}");
        }
    }

    #[test]
    fn user_message_start_preserves_delivery_text_and_count_without_image_bytes() {
        for wire in [
            r#"{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"steer one"},{"data":"PRIVATE_BASE64","mimeType":"image/png","type":"image"},{"type":"image","data":"PRIVATE_2"}],"timestamp":3}}"#,
            r#"{"message":{"content":[{"data":"PRIVATE_BASE64","type":"image"},{"type":"text","text":"follow up"}],"role":"user"},"type":"message_start"}"#,
        ] {
            let actual = decode_record(wire.as_bytes()).unwrap();
            let mut expected: Value = serde_json::from_str(wire).unwrap();
            for block in expected["message"]["content"].as_array_mut().unwrap() {
                if block["type"] == "image" {
                    block.as_object_mut().unwrap().remove("data");
                }
            }
            assert_eq!(actual, expected);
            assert!(!actual.to_string().contains("PRIVATE_"));
        }
        let wire = r#"{"type":"message_start","message":{"role":"user","content":[{"type":"custom","data":{"must":"stay"}},{"type":"text","data":"still-here","text":"hello"}]}}"#;
        assert_eq!(
            decode_record(wire.as_bytes()).unwrap(),
            serde_json::from_str::<Value>(wire).unwrap()
        );
        // Every duplicate, including an overwritten image field, is validated.
        for bad in [
            r#"{"type":"message_start","message":{"role":"user","content":[{"type":"image","data":"\ud800"}]}}"#,
            r#"{"type":"message_start","message":{"role":"user","content":[{"data":"\ud800","data":"valid","type":"image"}]}}"#,
            r#"{"type":"message_start","message":{"role":"user","content":[{"data":[1,,2],"type":"image"}]}}"#,
        ] {
            assert!(
                decode_record(bad.as_bytes()).is_err(),
                "malformed image echo must fail"
            );
        }
    }

    #[test]
    fn every_other_record_retains_full_value_and_correlation() {
        for wire in [
            r#"{"id":"req-1","type":"response","success":true,"data":{"partialResult":[1,2]}}"#,
            r#"{"partialResult":[1,2],"type":"future_event","id":"req-2"}"#,
            r#"{"type":"tool_execution_end","toolCallId":"call-1","toolName":"goal","result":{"details":{"hibiscusGoal":{"completed":1}}}}"#,
            r#"{"type":"extension_ui_request","id":"approval","method":"select","title":"Confirm","options":["Deny","Allow"]}"#,
            r#"{"type":"message_start","message":{"role":"assistant","content":[{"type":"image","data":"test"}]}}"#,
            r#"{"type":"message_end","message":{"role":"user","content":[{"type":"image","data":"test"}]}}"#,
            r#"{"type":"future_event","message":{"role":"user","content":[{"type":"image","data":"test"}]}}"#,
            r#"{"type":"queue_update","steering":["first"],"followUp":[]}"#,
            r#"{"type":"agent_settled"}"#,
            "{\"type\":\"future_event\",\"message\":\"a\u{2028}b\u{2029}c\"}",
        ] {
            let expected: Value = serde_json::from_str(wire).unwrap();
            assert_eq!(decode_record(wire.as_bytes()).unwrap(), expected);
        }
    }

    #[test]
    fn malformed_image_echo_invalidates_inbox_before_next_event() {
        let wire = b"{\"type\":\"message_start\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"image\",\"data\":\"\\ud800\"}]}}\n{\"type\":\"agent_settled\"}\n";
        let inbox = Inbox::start(
            io::Cursor::new(wire.to_vec()),
            Limits {
                bytes: 4096,
                records: 8,
            },
        );
        let error = inbox
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap_err()
            .to_string();
        assert!(error.contains("Invalid Pi RPC"));
        assert!(!error.contains("\\ud800"));
        assert!(inbox.failed());
        // The inbox reports its invalidation once, then closes; the queued
        // settlement must never be delivered after a malformed image echo.
        assert!(inbox.recv_timeout(Duration::ZERO).unwrap().is_err());
        assert!(matches!(
            inbox.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn allocated_capacity_is_accounted_not_only_frame_length() {
        let shared = Shared::default();
        let mut frame = Vec::with_capacity(128);
        frame.extend_from_slice(b"{}\n");
        assert!(!shared.push(
            frame,
            Limits {
                bytes: 64,
                records: 16
            }
        ));
        assert!(shared.state.lock().unwrap().failure.is_some());
    }

    #[test]
    fn giant_and_unterminated_records_are_bounded_and_invalid_json_is_redacted() {
        let inbox = Inbox::start(
            io::Cursor::new(vec![b'x'; 8192]),
            Limits {
                bytes: 1024,
                records: 16,
            },
        );
        wait_closed(&inbox);
        assert!(inbox
            .recv_timeout(Duration::ZERO)
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("record limit"));
        let inbox = Inbox::start(
            io::Cursor::new(b"private malformed payload\n".to_vec()),
            Limits {
                bytes: 1024,
                records: 16,
            },
        );
        let error = inbox
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap_err()
            .to_string();
        assert!(error.contains("Invalid Pi RPC"));
        assert!(!error.contains("private"));
        assert!(inbox.failed());
    }

    #[test]
    fn full_stdin_has_a_deadline_and_services_cancellation() {
        for cancel in [false, true] {
            let inbox = empty();
            let (pipe, _unread_peer) = UnixStream::pair().unwrap();
            let mut writer = inbox.writer(pipe).unwrap();
            writer.timeout = Duration::from_millis(500);
            let mut calls = 0;
            let mut checked = writer.checked(|| {
                calls += 1;
                if cancel && calls > 1 {
                    Err(io::Error::other("cancelled while sending"))
                } else {
                    Ok(())
                }
            });
            let start = Instant::now();
            let error = checked.write_all(&vec![b'x'; 4 * 1024 * 1024]).unwrap_err();
            assert!(start.elapsed() < Duration::from_secs(3));
            assert!(calls >= 2);
            if cancel {
                assert!(error.to_string().contains("cancelled"));
            } else {
                assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            }
        }
    }

    #[test]
    fn duplex_stdout_flood_breaks_blocked_stdin_without_waiting_for_write_deadline() {
        let (peer_output, our_output) = UnixStream::pair().unwrap();
        let (our_input, _peer_input) = UnixStream::pair().unwrap();
        let inbox = Inbox::start(
            our_output,
            Limits {
                bytes: 4096,
                records: 8,
            },
        );
        let mut writer = inbox.writer(our_input).unwrap();
        // This models a peer writing stdout before it will read stdin.
        let producer = thread::spawn(move || {
            let mut output = peer_output;
            thread::sleep(Duration::from_millis(20));
            let _ = output.write_all(
                "{\"type\":\"tool_execution_update\"}\n"
                    .repeat(1000)
                    .as_bytes(),
            );
        });
        let start = Instant::now();
        assert!(writer
            .write_all(&vec![b'x'; 4 * 1024 * 1024])
            .unwrap_err()
            .to_string()
            .contains("receive limit"));
        assert!(start.elapsed() < Duration::from_secs(3));
        wait_closed(&inbox);
        producer.join().unwrap();
    }

    #[test]
    fn large_nonblocking_write_is_complete_when_peer_drains() {
        let inbox = empty();
        let (pipe, mut peer) = UnixStream::pair().unwrap();
        let reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            bytes
        });
        let mut writer = inbox.writer(pipe).unwrap();
        let bytes = vec![b'x'; 2 * 1024 * 1024];
        writer.write_all(&bytes).unwrap();
        writer.flush().unwrap();
        drop(writer);
        assert_eq!(reader.join().unwrap(), bytes);
    }
}
