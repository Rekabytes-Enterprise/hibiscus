//! Frames from a video for the `analyze_video` tool, cut with ffmpeg.
//!
//! The desktop app bundles ffmpeg and points `HIBISCUS_FFMPEG` at it; the
//! terminal app uses an `ffmpeg` on PATH. Without either the tool is not offered.
use crate::Result;
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

/// The most frames one call may ask for, so the images fit the model's context.
pub(crate) const MAX_FRAMES: usize = 24;
/// Frames are scaled to this width; height keeps the aspect ratio.
const WIDTH: u32 = 640;

/// The ffmpeg to run: the app's bundled copy, else one on PATH.
pub(crate) fn ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HIBISCUS_FFMPEG").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Seconds from ffmpeg's `Duration: 00:01:02.50,` line.
pub(crate) fn parse_duration(stderr: &str) -> Option<f64> {
    let rest = stderr.split("Duration: ").nth(1)?;
    let stamp = rest.split(',').next()?.trim();
    let mut parts = stamp.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    let seconds =
        h.parse::<f64>().ok()? * 3600.0 + m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?;
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

/// `count` moments spread over the video, each in the middle of its slice.
pub(crate) fn moments(duration: f64, count: usize) -> Vec<f64> {
    (0..count)
        .map(|i| (duration * (i as f64 + 0.5) / count as f64 * 100.0).round() / 100.0)
        .collect()
}

/// `75.5` as `1:15.5`.
pub(crate) fn clock(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    let rest = if rest.fract().abs() < 0.05 {
        format!("{:02}", rest.round() as u32)
    } else {
        format!("{rest:04.1}")
    };
    format!("{minutes}:{rest}")
}

fn command(ffmpeg: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(ffmpeg);
    command.arg("-hide_banner").arg("-nostdin");
    command
}

/// A frame: when it is from, and the JPEG as base64.
pub(crate) struct Frame {
    pub time: f64,
    pub jpeg: String,
}

/// The video's length and `count` frames spread across it.
pub(crate) async fn frames(
    video: &Path,
    count: usize,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(f64, Vec<Frame>)> {
    let ffmpeg = ffmpeg().ok_or("ffmpeg is not available")?;
    if !video.is_file() {
        return Err(format!("No video file at {}", video.display()).into());
    }
    // `-i` alone prints the stream info, Duration included, and exits non-zero.
    let mut probe = command(&ffmpeg);
    probe.arg("-i").arg(video);
    let info = crate::process::run(probe, None, Duration::from_secs(60), cancel).await?;
    let duration = parse_duration(&String::from_utf8_lossy(&info.stderr))
        .ok_or("ffmpeg could not read this file as a video")?;

    let dir = std::env::temp_dir().join(format!(
        "hibiscus-frames-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir)?;
    let result: Result<Vec<Frame>> = async {
        let mut out = Vec::new();
        for (i, time) in moments(duration, count).into_iter().enumerate() {
            let file = dir.join(format!("{i}.jpg"));
            let mut cut = command(&ffmpeg);
            // `-ss` before `-i` seeks fast to the nearest keyframe, then decodes to the moment.
            cut.args(["-v", "error", "-ss", &format!("{time:.2}")])
                .arg("-i")
                .arg(video)
                .args([
                    "-frames:v",
                    "1",
                    "-vf",
                    &format!("scale={WIDTH}:-2"),
                    "-q:v",
                    "4",
                    "-y",
                ])
                .arg(&file);
            let output = crate::process::run(cut, None, Duration::from_secs(60), cancel).await?;
            if !output.status.success() || !file.is_file() {
                return Err(format!(
                    "ffmpeg could not cut the frame at {}: {}",
                    clock(time),
                    String::from_utf8_lossy(&output.stderr).trim()
                )
                .into());
            }
            out.push(Frame {
                time,
                jpeg: base64::engine::general_purpose::STANDARD.encode(std::fs::read(&file)?),
            });
        }
        Ok(out)
    }
    .await;
    let _ = std::fs::remove_dir_all(&dir);
    Ok((duration, result?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_duration_line() {
        let stderr = "Input #0, mov,mp4, from 'a.mp4':\n  Duration: 00:01:02.50, start: 0.000000, bitrate: 1205 kb/s\n";
        assert_eq!(parse_duration(stderr), Some(62.5));
        assert_eq!(parse_duration("  Duration: N/A, bitrate: N/A"), None);
        assert_eq!(parse_duration("no video here"), None);
    }

    #[test]
    fn spreads_frames_over_the_video() {
        assert_eq!(moments(10.0, 4), [1.25, 3.75, 6.25, 8.75]);
        assert_eq!(moments(3.0, 1), [1.5]);
    }

    #[test]
    fn labels_frames_with_a_clock() {
        assert_eq!(clock(1.25), "0:01.2");
        assert_eq!(clock(75.0), "1:15");
        assert_eq!(clock(3.75), "0:03.8");
    }

    // Runs only where ffmpeg is installed; CI without it skips the real cut.
    #[tokio::test]
    async fn cuts_frames_from_a_real_video() {
        let Some(ffmpeg) = ffmpeg() else { return };
        let video = std::env::temp_dir().join(format!("hibiscus-test-{}.mp4", std::process::id()));
        let made = std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=3:size=320x240:rate=10",
                "-y",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(made.success());
        let (_tx, mut cancel) = watch::channel(false);
        let (duration, frames) = frames(&video, 3, &mut cancel).await.unwrap();
        let _ = std::fs::remove_file(&video);
        assert!((duration - 3.0).abs() < 0.2, "{duration}");
        assert_eq!(
            frames.iter().map(|f| f.time).collect::<Vec<_>>(),
            [0.5, 1.5, 2.5]
        );
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(&frames[0].jpeg)
            .unwrap();
        assert_eq!(&jpeg[..2], [0xff, 0xd8]);
    }
}
