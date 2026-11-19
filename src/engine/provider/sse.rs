//! Server-sent-event framing, independent of network chunk boundaries.
use crate::Result;

#[derive(Debug, PartialEq)]
pub(super) enum Frame {
    Data(String),
    Comment(String),
}

#[derive(Default)]
pub(super) struct Decoder {
    pub(super) buffer: Vec<u8>,
}
impl Decoder {
    #[cfg(any(test, feature = "fuzzing"))]
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        Ok(self
            .push_frames(bytes)?
            .into_iter()
            .filter_map(|frame| match frame {
                Frame::Data(data) => Some(data),
                Frame::Comment(_) => None,
            })
            .collect())
    }
    pub(super) fn push_frames(&mut self, bytes: &[u8]) -> Result<Vec<Frame>> {
        if bytes.len() > (2 * 1024 * 1024_usize).saturating_sub(self.buffer.len()) {
            return Err("Provider streaming frame exceeds 2 MiB".into());
        }
        self.buffer.extend_from_slice(bytes);
        let mut frames = vec![];
        let mut consumed = 0;
        loop {
            let remaining = &self.buffer[consumed..];
            // Pick the first delimiter on the wire, regardless of line endings.
            let marker = remaining.windows(2).enumerate().find_map(|(i, pair)| {
                if pair == b"\n\n" {
                    Some((i, 2))
                } else if pair == b"\r\n" && remaining.get(i..i + 4) == Some(b"\r\n\r\n") {
                    Some((i, 4))
                } else {
                    None
                }
            });
            let Some((end, skip)) = marker else { break };
            let frame = std::str::from_utf8(&remaining[..end])?;
            for comment in frame.lines().filter_map(|line| line.strip_prefix(':')) {
                frames.push(Frame::Comment(comment.trim_start().to_owned()));
            }
            let data = frame
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|s| s.strip_prefix(' ').unwrap_or(s))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() {
                frames.push(Frame::Data(data));
            }
            consumed += end + skip;
        }
        // Drain once rather than repeatedly shifting every remaining frame.
        self.buffer.drain(..consumed);
        Ok(frames)
    }
}
