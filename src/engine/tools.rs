use super::search;
use crate::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{process::Command, sync::watch};

/// The built-in tools. `cwd` is the session folder: when it is connected to
/// WSL, the bash tool says it runs there.
pub fn definitions(cwd: Option<&Path>) -> Vec<Value> {
    let shell = match cwd.and_then(crate::wsl::distro_for) {
        Some(distro) => crate::wsl::description(&distro),
        None => crate::sys::shell_description().to_owned(),
    };
    let mut definitions = vec![
        json!({"name":"read","description":"Read a UTF-8 text file, up to 2000 lines or 50 KB. Each line starts with its number and a tab, which is not part of the file. A negative offset reads from the end.","parameters":{"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer","minimum":1}},"required":["path"],"additionalProperties":false}}),
        json!({"name":"grep","description":"Regex search of file contents, skipping ignored, hidden and binary files. Returns up to 200 path:line: text matches. Use it before reading whole files.","parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"glob":{"type":"string"},"ignore_case":{"type":"boolean"},"files_only":{"type":"boolean"}},"required":["pattern"],"additionalProperties":false}}),
        json!({"name":"glob","description":"List up to 200 files matching a glob such as src/**/*.ts.","parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"],"additionalProperties":false}}),
        json!({"name":"write","description":"Write UTF-8 contents to a file. Creates parent directories. Replaces existing contents.","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}),
        json!({"name":"edit","description":"Replace exactly one occurrence of oldText with newText. Fails if missing or ambiguous.","parameters":{"type":"object","properties":{"path":{"type":"string"},"oldText":{"type":"string"},"newText":{"type":"string"}},"required":["path","oldText","newText"],"additionalProperties":false}}),
        json!({"name":"bash","description":shell,"parameters":{"type":"object","properties":{"command":{"type":"string"},"timeout":{"type":"integer","minimum":1,"maximum":600}},"required":["command"],"additionalProperties":false}}),
        json!({"name":"goal","description":"Track an explicit checklist. Set the list before work, complete a one-based step only when finished, or clear it.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["set","complete","clear"]},"steps":{"type":"array","items":{"type":"string"}},"id":{"type":"integer"}},"required":["action"],"additionalProperties":false}}),
    ];
    if agent_browser().is_some() {
        definitions.push(json!({"name":"browser","description":"Drive a headless Chrome that the user watches in the app's browser pane. Pass one agent-browser command as args:\n- [\"open\",\"http://localhost:5173\"]\n- [\"snapshot\",\"-i\"] lists interactive elements with refs like @e1. Snapshot again after the page changes.\n- [\"click\",\"@e1\"], [\"fill\",\"@e2\",\"text\"], [\"press\",\"Enter\"], [\"get\",\"text\",\"body\"]\n- [\"console\"] shows log lines and [\"errors\"] uncaught exceptions.\n- [\"network\",\"requests\",\"--status\",\"400-599\"] lists failed requests (also --filter, --method, --type). [\"network\",\"request\",\"<id>\"] shows one request's body.\n- Logs keep the latest 100 entries. Add --clear to empty them before reproducing a problem.\n- [\"screenshot\"] returns the page as an image you can see. Add \"#app\" for one element or \"--full\" for the whole page. Use it to check layout, overlap, colours and blank areas.\nStart the project's server with dev_server, not bash.","parameters":{"type":"object","properties":{"args":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64},"timeout":{"type":"integer","minimum":1,"maximum":600}},"required":["args"],"additionalProperties":false}}));
        definitions.push(json!({"name":"dev_server","description":"Run the project's dev server in the background, one per folder. It keeps running between tool calls and shows in the browser pane.\n- start runs `command`, or the project's usual one (package.json dev or start script, Django, Rails, a static index.html), and returns its local URL. If a server is already running, it returns that one.\n- status reports it. output returns its last lines, where build errors show up. stop stops it.\nOpen the URL with the browser tool.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["start","status","output","stop"]},"command":{"type":"string","maxLength":4096},"lines":{"type":"integer","minimum":1,"maximum":500}},"required":["action"],"additionalProperties":false}}));
    }
    if crate::video::ffmpeg().is_some() {
        definitions.push(json!({"name":"analyze_video","description":"See a video: returns `frames` stills spread across it (default 12, at most 24) as images with their times, plus its length. Use it before describing, analysing or remaking a video file. The frames also open on the canvas as a Frames card.","parameters":{"type":"object","properties":{"path":{"type":"string","minLength":1,"maxLength":4096},"frames":{"type":"integer","minimum":1,"maximum":24}},"required":["path"],"additionalProperties":false}}));
    }
    if canvas() {
        definitions.push(json!({"name":"ask_user","description":"Ask the user on the canvas when a canvas request needs an answer before you can go on. Give each question 2 to 5 short suggested `options`. The user can also type an answer. Then end your turn. The answers arrive as the user's next message.","parameters":{"type":"object","properties":{"questions":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"object","properties":{"question":{"type":"string","minLength":1,"maxLength":500},"options":{"type":"array","minItems":2,"maxItems":5,"items":{"type":"string","minLength":1,"maxLength":200}}},"required":["question","options"],"additionalProperties":false}}},"required":["questions"],"additionalProperties":false}}));
        definitions.push(json!({"name":"storyboard","description":"Open a storyboard on the canvas for the user to review before any video is made. Each beat has the script `line` it goes with and an `svg`: a complete <svg> element, viewBox 0 0 1920 1080, with no scripts or external links.\n- Follow the style of any reference frames.\n- With a style, set each beat's `component` to the style body it uses. No two beats share one.\n- Send long storyboards in batches of up to 4: the first call without `from`, later calls with `from` set to the batch's first beat number (5, 9, …).\nThe user keeps or removes beats and confirms. Only then build the video.","parameters":{"type":"object","properties":{"title":{"type":"string","maxLength":200},"beats":{"type":"array","minItems":1,"maxItems":40,"items":{"type":"object","properties":{"line":{"type":"string","minLength":1,"maxLength":2000},"svg":{"type":"string","minLength":10,"maxLength":200000},"component":{"type":"string","maxLength":80}},"required":["line","svg"],"additionalProperties":false}},"from":{"type":"integer","minimum":1,"maximum":40}},"required":["beats"],"additionalProperties":false}}));
        definitions.push(json!({"name":"video_preview","description":"Show a video in a preview card joined to the user's Video card.\n- project: the HyperFrames project folder. The app starts its preview server and shows it; don't start one yourself. It also checks the video's length against the voiceover.\n- video: the rendered MP4, once there is one.\n- url: only for a page you serve some other way, at http://localhost or http://127.0.0.1.","parameters":{"type":"object","properties":{"project":{"type":"string","maxLength":4096},"video":{"type":"string","maxLength":4096},"url":{"type":"string","maxLength":2048}},"additionalProperties":false}}));
        definitions.push(json!({"name":"text_to_speech","description":"Open a script on the canvas as a Text to speech card, where the user reviews it and makes the audio with ElevenLabs.\n- script: only the words to be spoken.\n- video: one line on what the video shows, if there is one.\n- references: paths of images or videos the user gave as references.","parameters":{"type":"object","properties":{"script":{"type":"string","minLength":1,"maxLength":20000},"video":{"type":"string","maxLength":2000},"references":{"type":"array","items":{"type":"string","maxLength":4096},"maxItems":32}},"required":["script"],"additionalProperties":false}}));
    }
    definitions.extend(crate::extensions::agent_definitions());
    definitions.extend(crate::checkpoints::definitions());
    definitions
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DevServer {
    action: String,
    command: Option<String>,
    lines: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    path: String,
    offset: Option<i64>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    ignore_case: Option<bool>,
    files_only: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobArgs {
    pattern: String,
    path: Option<String>,
}
/// Lines and bytes one read returns at most, and the longest line it shows whole.
const READ_LINES: usize = 2000;
const READ_BYTES: usize = 50 * 1024;
const LINE_CHARS: usize = 2000;

/// Lines `start..` of a file, numbered, within the read limits; also returns
/// the line after the last one shown and whether any line was cut.
fn numbered(lines: &[&str], start: usize, limit: usize) -> (String, usize, bool) {
    let mut out = String::new();
    let mut cut_any = false;
    let mut end = start;
    for (i, line) in lines.iter().enumerate().skip(start).take(limit) {
        let cut = line
            .char_indices()
            .nth(LINE_CHARS)
            .map_or(line.len(), |(i, _)| i);
        let entry = if cut < line.len() {
            cut_any = true;
            format!(
                "{:>6}\t{}… [line cut at {LINE_CHARS} characters]\n",
                i + 1,
                &line[..cut]
            )
        } else {
            format!("{:>6}\t{line}\n", i + 1)
        };
        if i > start && out.len() + entry.len() > READ_BYTES {
            break;
        }
        out.push_str(&entry);
        end = i + 1;
    }
    (out, end, cut_any)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    path: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Edit {
    path: String,
    old_text: String,
    new_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bash {
    command: String,
    timeout: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeVideo {
    path: String,
    frames: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    question: String,
    options: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskUser {
    questions: Vec<Question>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Beat {
    line: String,
    svg: String,
    #[allow(dead_code)] // The app reads it from the tool call event.
    component: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Storyboard {
    #[allow(dead_code)] // The app reads it from the tool call event.
    title: Option<String>,
    beats: Vec<Beat>,
    from: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VideoPreview {
    url: Option<String>,
    #[allow(dead_code)]
    video: Option<String>,
    project: Option<String>,
}
/// A HyperFrames project's length and scene count from its `index.html`: the
/// root composition's `data-duration`, and the mounted sub-compositions other
/// than captions.
fn timeline(html: &str) -> Option<(f64, usize)> {
    let attr = |tag: &str, name: &str| {
        let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
        Some(tag[start..].split('"').next()?.to_string())
    };
    let root = html.find("data-composition-id=")?;
    let open = html[..root].rfind('<')?;
    let tag = &html[open..open + html[open..].find('>')?];
    let duration = attr(tag, "data-duration")?.parse().ok()?;
    let scenes = html
        .split('<')
        .filter_map(|t| attr(t.split('>').next()?, "data-composition-src"))
        .filter(|src| !src.contains("caption"))
        .count();
    Some((duration, scenes))
}
/// Only the user's own machine: the card embeds this page.
fn local_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "http" && matches!(u.host_str(), Some("localhost" | "127.0.0.1"))
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextToSpeech {
    script: String,
    #[allow(dead_code)] // The app reads it from the tool call event.
    video: Option<String>,
    references: Option<Vec<String>>,
}
/// The desktop app shows a canvas; the text_to_speech tool exists only there.
pub(crate) fn canvas() -> bool {
    std::env::var_os("HIBISCUS_CANVAS").is_some()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Browser {
    args: Vec<String>,
    timeout: Option<u64>,
}

/// The agent-browser binary the desktop app bundles. The browser tool exists only when it is set.
fn agent_browser() -> Option<PathBuf> {
    std::env::var_os("HIBISCUS_AGENT_BROWSER").map(PathBuf::from)
}
/// One session for the app, so the browser pane knows which stream to show.
pub const BROWSER_SESSION: &str = "hibiscus";
// Commands that manage agent-browser itself or reach outside its own headless Chrome.
const BROWSER_DENIED: &[&str] = &[
    "install",
    "upgrade",
    "doctor",
    "dashboard",
    "chat",
    "connect",
    "mcp",
    "profiles",
    "skills",
    "stream",
    "session",
];
// Flags that would attach the user's own Chrome or profile, or change which browser runs.
const BROWSER_FLAGS: &[&str] = &[
    "--session",
    "--profile",
    "--state",
    "--cdp",
    "--auto-connect",
    "--executable-path",
    "--extension",
    "--provider",
    "-p",
    "--engine",
    "--namespace",
    "--config",
    "--headed",
    "--allow-file-access",
    "--restore",
];
fn browser_command(bin: &Path, args: &[String], cwd: &Path) -> Command {
    let mut command = Command::new(bin);
    command
        .args(["--session", BROWSER_SESSION])
        .args(args)
        .current_dir(cwd);
    command
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Goal {
    action: String,
    steps: Option<Vec<String>>,
    id: Option<usize>,
}
fn path(cwd: &Path, path: &str) -> Result<PathBuf> {
    if path.is_empty() {
        return Err("Empty file path".into());
    }
    // In a WSL-connected folder the model sees Linux paths, such as
    // `/home/me/app/main.rs` or `/mnt/c/...`; read them as the Windows path.
    if cfg!(windows) && path.starts_with('/') {
        if let Some(distro) = crate::wsl::distro_for(cwd) {
            return Ok(PathBuf::from(crate::wsl::to_windows(&distro, path)));
        }
    }
    let p = PathBuf::from(path);
    Ok(if p.is_absolute() { p } else { cwd.join(p) })
}
fn read_file(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > 16 * 1024 * 1024 {
        return Err("Expected a regular UTF-8 file no larger than 16 MiB".into());
    }
    Ok(std::fs::read_to_string(path)?)
}
fn result(text: String, details: Value) -> Value {
    json!({"content":[{"type":"text","text":text}],"details":details})
}

/// Keep the OS exit status typed through the error path. Human-readable errors
/// still retain all bounded output; the UI need not parse a formatted status.
#[derive(Debug)]
pub(super) struct ShellExit {
    status: std::process::ExitStatus,
    output: String,
}
impl ShellExit {
    pub(super) fn code(&self) -> Option<i32> {
        self.status.code()
    }
}
impl std::fmt::Display for ShellExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Shell exited with {}\n{}", self.status, self.output)
    }
}
impl std::error::Error for ShellExit {}
pub fn validate(name: &str, args: &Value) -> Result<()> {
    match name {
        "read" => {
            let a: Read = serde_json::from_value(args.clone())?;
            if a.path.is_empty() || a.offset == Some(0) || a.limit == Some(0) {
                return Err("Invalid read bounds/path".into());
            }
        }
        "grep" => {
            let a: GrepArgs = serde_json::from_value(args.clone())?;
            if a.pattern.is_empty()
                || a.pattern.len() > 1000
                || a.glob.as_ref().is_some_and(|g| g.len() > 500)
            {
                return Err("grep takes a pattern of 1 to 1000 characters".into());
            }
            search::check_pattern(&a.pattern)?;
        }
        "glob" => {
            let a: GlobArgs = serde_json::from_value(args.clone())?;
            if a.pattern.is_empty() || a.pattern.len() > 500 {
                return Err("glob takes a pattern of 1 to 500 characters".into());
            }
        }
        "write" => {
            let a: Write = serde_json::from_value(args.clone())?;
            if a.path.is_empty() || a.content.len() > 16 * 1024 * 1024 {
                return Err("Invalid write path/content size".into());
            }
        }
        "edit" => {
            let a: Edit = serde_json::from_value(args.clone())?;
            if a.path.is_empty() || a.old_text.is_empty() || a.new_text.len() > 16 * 1024 * 1024 {
                return Err("Invalid edit arguments".into());
            }
        }
        "bash" => {
            let a: Bash = serde_json::from_value(args.clone())?;
            if a.command.trim().is_empty()
                || a.command.len() > 65536
                || !(1..=600).contains(&a.timeout.unwrap_or(120))
            {
                return Err("Invalid command or timeout".into());
            }
        }
        "goal" => {
            let _: Goal = serde_json::from_value(args.clone())?;
        }
        "browser" => {
            let a: Browser = serde_json::from_value(args.clone())?;
            let verb = a.args.first().map(String::as_str).unwrap_or("");
            if verb.is_empty()
                || verb.starts_with('-')
                || a.args.len() > 64
                || a.args.iter().any(|arg| arg.len() > 65536)
                || !(1..=600).contains(&a.timeout.unwrap_or(60))
            {
                return Err("Invalid browser command or timeout".into());
            }
            if BROWSER_DENIED.contains(&verb) {
                return Err(format!("The browser tool does not run `{verb}`").into());
            }
            // Text after `--` is literal, such as a value typed into a field.
            let flags = a.args.iter().take_while(|arg| *arg != "--");
            if let Some(flag) = flags
                .filter_map(|arg| arg.split('=').next())
                .find(|flag| BROWSER_FLAGS.contains(flag))
            {
                return Err(format!("The browser tool does not accept {flag}").into());
            }
        }
        "dev_server" => {
            let a: DevServer = serde_json::from_value(args.clone())?;
            if !["start", "status", "output", "stop"].contains(&a.action.as_str())
                || a.command.as_ref().is_some_and(|c| c.len() > 4096)
                || !(1..=500).contains(&a.lines.unwrap_or(50))
            {
                return Err("Invalid dev_server action, command or lines".into());
            }
        }
        "analyze_video" => {
            let a: AnalyzeVideo = serde_json::from_value(args.clone())?;
            if a.path.is_empty()
                || a.path.len() > 4096
                || !(1..=crate::video::MAX_FRAMES).contains(&a.frames.unwrap_or(12))
            {
                return Err("Invalid analyze_video path or frame count".into());
            }
        }
        "ask_user" => {
            let a: AskUser = serde_json::from_value(args.clone())?;
            let short = |s: &str, max: usize| !s.trim().is_empty() && s.len() <= max;
            if !canvas()
                || !(1..=4).contains(&a.questions.len())
                || a.questions.iter().any(|q| {
                    !short(&q.question, 500)
                        || !(2..=5).contains(&q.options.len())
                        || q.options.iter().any(|o| !short(o, 200))
                })
            {
                return Err(
                    "ask_user takes 1 to 4 questions, each with 2 to 5 short options".into(),
                );
            }
        }
        "storyboard" => {
            let a: Storyboard = serde_json::from_value(args.clone())?;
            let svg = |s: &str| {
                let lower = s.to_ascii_lowercase();
                s.len() <= 200_000 && lower.contains("<svg") && !lower.contains("<script")
            };
            if !canvas()
                || a.beats.is_empty()
                || a.beats.len() > 40
                || !(1..=40).contains(&a.from.unwrap_or(1))
                || a.beats.iter().any(|b| {
                    b.line.trim().is_empty()
                        || b.line.len() > 2000
                        || !svg(&b.svg)
                        || b.component.as_ref().is_some_and(|c| c.len() > 80)
                })
            {
                return Err(
                    "Invalid storyboard: 1 to 40 beats, each a line and an <svg> without scripts"
                        .into(),
                );
            }
        }
        "video_preview" => {
            let a: VideoPreview = serde_json::from_value(args.clone())?;
            if !canvas() || a.url.as_deref().is_some_and(|u| !local_url(u)) {
                return Err(
                    "video_preview shows only http://localhost or http://127.0.0.1 pages".into(),
                );
            }
            if a.url.is_none() && a.project.is_none() {
                return Err("video_preview needs the project folder".into());
            }
        }
        "text_to_speech" => {
            let a: TextToSpeech = serde_json::from_value(args.clone())?;
            if !canvas()
                || a.script.trim().is_empty()
                || a.script.len() > 20000
                || a.references.as_ref().is_some_and(|r| r.len() > 32)
            {
                return Err("Invalid text_to_speech script or references".into());
            }
        }
        _ => return Err("Unknown tool".into()),
    }
    Ok(())
}
pub async fn execute(
    name: &str,
    args: &Value,
    context: &mut crate::checkpoints::Context<'_>,
    steps: &mut Vec<Value>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Value> {
    if let Some(tool) = crate::checkpoints::Tool::parse(name) {
        return tool.execute(args, context, cancel).await;
    }
    let cwd = context.cwd;
    validate(name, args)?;
    if *cancel.borrow() {
        return Err("Tool cancelled before execution".into());
    }
    match name {
        "read" => {
            let a: Read = serde_json::from_value(args.clone())?;
            let text = read_file(&path(cwd, &a.path)?)?;
            let lines: Vec<_> = text.lines().collect();
            let total = lines.len();
            if total == 0 {
                return Ok(result("[The file is empty.]".into(), json!({"totalLines":0})));
            }
            let start = match a.offset {
                Some(o) if o < 0 => total.saturating_sub(o.unsigned_abs() as usize),
                Some(o) => o as usize - 1,
                None => 0,
            };
            if start >= total {
                return Err(format!("Offset {} is past the end: the file has {total} lines.", start + 1).into());
            }
            let (shown, end, cut) = numbered(&lines, start, a.limit.unwrap_or(READ_LINES));
            let mut note = String::new();
            if start > 0 || end < total {
                note.push_str(&format!("[Lines {}-{end} of {total}.", start + 1));
                if end < total {
                    note.push_str(&format!(" Read more with offset {}.", end + 1));
                }
                note.push(']');
            }
            if cut {
                note.push_str(&format!("[Lines over {LINE_CHARS} characters were cut.]"));
            }
            Ok(result(format!("{shown}{note}"), json!({"totalLines":total})))
        }
        "grep" => {
            let a: GrepArgs = serde_json::from_value(args.clone())?;
            let root = match &a.path {
                Some(p) => path(cwd, p)?,
                None => cwd.to_path_buf(),
            };
            let text = search::grep(
                search::Grep {
                    pattern: a.pattern,
                    root,
                    glob: a.glob,
                    ignore_case: a.ignore_case.unwrap_or(false),
                    files_only: a.files_only.unwrap_or(false),
                },
                cwd.to_path_buf(),
                cancel.clone(),
            )
            .await?;
            Ok(result(text, json!({})))
        }
        "glob" => {
            let a: GlobArgs = serde_json::from_value(args.clone())?;
            let root = match &a.path {
                Some(p) => path(cwd, p)?,
                None => cwd.to_path_buf(),
            };
            let text = search::glob(a.pattern, root, cwd.to_path_buf(), cancel.clone()).await?;
            Ok(result(text, json!({})))
        }
        "write" => {
            let a: Write = serde_json::from_value(args.clone())?;
            let target = path(cwd, &a.path)?;
            let checkpoint =
                crate::checkpoints::write(context, &target, a.content.as_bytes(), None)?;
            Ok(result(
                format!(
                    "Wrote {} bytes to {}{}",
                    a.content.len(),
                    a.path,
                    checkpoint_notice(&checkpoint)
                ),
                json!({"checkpoint":checkpoint}),
            ))
        }
        "edit" => {
            let a: Edit = serde_json::from_value(args.clone())?;
            let target = path(cwd, &a.path)?;
            let old = read_file(&target)?;
            if old.matches(&a.old_text).count() != 1 {
                return Err(
                    "Edit requires exactly one occurrence of oldText; file unchanged".into(),
                );
            }
            let next = old.replacen(&a.old_text, &a.new_text, 1);
            if next.len() > 16 * 1024 * 1024 {
                return Err("Edited file would exceed 16 MiB; file unchanged".into());
            }
            let checkpoint =
                crate::checkpoints::write(context, &target, next.as_bytes(), Some(old.as_bytes()))?;
            let (diff, added, removed) = edit_diff(&old, &next);
            Ok(result(
                format!("Edited {}{}", a.path, checkpoint_notice(&checkpoint)),
                json!({"diff":diff,"addedLines":added,"removedLines":removed,"checkpoint":checkpoint}),
            ))
        }
        "bash" => {
            let a: Bash = serde_json::from_value(args.clone())?;
            // In a WSL-connected folder the command runs in that distro; `wsl`
            // kills its Linux process group when dropped, after the run.
            let wsl = crate::wsl::distro_for(cwd).map(crate::wsl::Run::new);
            let cmd = match &wsl {
                Some(run) => Command::from(run.command(cwd, &a.command)?),
                None => {
                    let mut cmd = Command::from(crate::sys::shell(&a.command));
                    cmd.current_dir(cwd);
                    cmd
                }
            };
            let output = crate::process::run(
                cmd,
                None,
                Duration::from_secs(a.timeout.unwrap_or(120)),
                cancel,
            )
            .await;
            drop(wsl);
            let output = output?;
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if !output.status.success() {
                return Err(ShellExit {
                    status: output.status,
                    // The agent loop caps it, keeping the end where errors are.
                    output: text,
                }
                .into());
            }
            // A script's exit code is its last command's: an earlier step can crash
            // unseen, so a crash in the output is called out.
            let text = match hidden_failure(&text) {
                Some(line) => format!("{text}\n\nWarning: the exit code is 0, but the output shows a crash ({line}). The exit code only reflects the last command, so an earlier step failed: check what it changed before going on, and join dependent steps with && so a failure stops them."),
                None => text,
            };
            Ok(result(text, json!({"exitCode":output.status.code()})))
        }
        "browser" => {
            let a: Browser = serde_json::from_value(args.clone())?;
            let bin = agent_browser().ok_or("The browser tool is not available")?;
            let timeout = Duration::from_secs(a.timeout.unwrap_or(60));
            let text = |o: &crate::process::Output| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                )
            };
            let mut output =
                crate::process::run(browser_command(&bin, &a.args, cwd), None, timeout, cancel)
                    .await?;
            // Without its own Chrome or an installed Google Chrome, agent-browser
            // fails with this message. Download Chrome for Testing once, then retry.
            if !output.status.success() && text(&output).contains("Chrome not found") {
                let mut install = Command::new(&bin);
                install.arg("install").current_dir(cwd);
                let installed =
                    crate::process::run(install, None, Duration::from_secs(600), cancel).await?;
                if !installed.status.success() {
                    return Err(format!(
                        "Could not download Chrome for the browser tool:\n{}",
                        bounded(&text(&installed), 65536).0
                    )
                    .into());
                }
                output =
                    crate::process::run(browser_command(&bin, &a.args, cwd), None, timeout, cancel)
                        .await?;
            }
            let text = text(&output);
            if !output.status.success() {
                return Err(ShellExit {
                    status: output.status,
                    // The agent loop caps it, keeping the end where errors are.
                    output: text,
                }
                .into());
            }
            let details = json!({"exitCode":output.status.code()});
            // Logs can hold a thousand entries; the latest are what matter.
            let logs = matches!(
                a.args.iter().map(String::as_str).collect::<Vec<_>>()[..],
                ["console", ..] | ["errors", ..] | ["network", "requests", ..]
            );
            let text = if logs {
                recent(&text, 100)
            } else if a.args[0] == "network" {
                bounded(&text, 16384).0
            } else {
                text
            };
            let saved = (a.args[0] == "screenshot")
                .then(|| {
                    text.lines()
                        .find_map(|l| l.split_once("Screenshot saved to "))
                })
                .flatten()
                .map(|(_, path)| path.trim());
            let Some(saved) = saved else {
                return Ok(result(text, details));
            };
            Ok(match screenshot(Path::new(saved)) {
                Ok((data, note)) => json!({"content":[
                    {"type":"text","text":format!("{text}{note}")},
                    {"type":"image","mimeType":"image/jpeg","data":data}
                ],"details":details}),
                Err(e) => result(
                    format!("{text}The screenshot could not be attached: {e}"),
                    details,
                ),
            })
        }

        "dev_server" => {
            use crate::devserver;
            let a: DevServer = serde_json::from_value(args.clone())?;
            let tail = |n| devserver::output(cwd, n).join("\n");
            let describe = |s: &devserver::Status| {
                let state = match (s.running, s.exit_code, &s.url) {
                    (true, _, Some(url)) => format!("running at {url}"),
                    (true, _, None) => "running, but it hasn't printed a local URL yet".into(),
                    (false, code, _) => format!(
                        "stopped (exit code {})",
                        code.map_or("unknown".into(), |c| c.to_string())
                    ),
                };
                format!("`{}` is {state}.", s.command)
            };
            let text = match a.action.as_str() {
                "start" => {
                    devserver::start(cwd, a.command.as_deref())?;
                    let s = tokio::select! {
                        s = devserver::ready(cwd, Duration::from_secs(60)) => s.ok_or("The dev server is gone")?,
                        _ = cancel.changed() => return Err("Tool cancelled; the dev server keeps running".into()),
                    };
                    format!("{}\nLast output:\n{}", describe(&s), tail(20))
                }
                "status" => devserver::status(cwd)
                    .map_or("No dev server is running for this folder.".into(), |s| {
                        describe(&s)
                    }),
                "output" => match devserver::status(cwd) {
                    Some(s) => format!("{}\n{}", describe(&s), tail(a.lines.unwrap_or(50))),
                    None => "No dev server is running for this folder.".into(),
                },
                _ => {
                    devserver::stop(cwd);
                    "Stopped the dev server.".into()
                }
            };
            Ok(result(text, json!({})))
        }

        "analyze_video" => {
            let a: AnalyzeVideo = serde_json::from_value(args.clone())?;
            let file = path(cwd, &a.path)?;
            let (duration, frames) =
                crate::video::frames(&file, a.frames.unwrap_or(12), cancel).await?;
            let clock = crate::video::clock;
            let mut content = vec![json!({"type":"text","text":format!(
                "{} is {} long. {} frames follow, in order, at {}.",
                file.display(),
                clock(duration),
                frames.len(),
                frames.iter().map(|f| clock(f.time)).collect::<Vec<_>>().join(", ")
            )})];
            content.extend(
                frames
                    .iter()
                    .map(|f| json!({"type":"image","mimeType":"image/jpeg","data":f.jpeg})),
            );
            Ok(json!({"content":content,"details":{
                "path":file,
                "duration":duration,
                "frames":frames.iter().map(|f| f.time).collect::<Vec<_>>(),
            }}))
        }
        "ask_user" => Ok(result(
            "The questions are on the canvas. End your turn now; the user's answers come as their next message.".into(),
            json!({}),
        )),
        "storyboard" => Ok(result(
            "Added these beats to the storyboard on the canvas. Send any remaining batch, then stop and wait for the user to confirm it before building the video.".into(),
            json!({}),
        )),
        "video_preview" => {
            let a: VideoPreview = serde_json::from_value(args.clone())?;
            let project = a.project.as_deref().map(|p| path(cwd, p)).transpose()?;
            let url = match a.url {
                Some(url) => url,
                None => {
                    let dir = project.as_deref().ok_or("video_preview needs the project folder")?;
                    tokio::select! {
                        url = crate::video_engine::preview(dir) => url?,
                        _ = cancel.changed() => return Err("Tool cancelled; the preview server keeps running".into()),
                    }
                }
            };
            // The app checks this against the voiceover and the storyboard.
            let found = project
                .as_deref()
                .and_then(|p| std::fs::read_to_string(p.join("index.html")).ok())
                .and_then(|html| timeline(&html));
            let project = project.map(|p| p.display().to_string());
            Ok(match found {
                Some((duration, scenes)) => result(
                    format!("The preview is showing in a card on the canvas at {url}. The project's timeline is {duration:.1} s long with {scenes} scenes."),
                    json!({"url":url,"project":project,"duration":duration,"scenes":scenes}),
                ),
                None => result(
                    format!("The preview is showing in a card on the canvas at {url}."),
                    json!({"url":url,"project":project}),
                ),
            })
        }
        // The app opens the cards from the tool call event; nothing runs here.
        "text_to_speech" => Ok(result(
            "Opened the script on the canvas as a Text to speech card joined to the Video card. The user reviews and edits it there before any audio is made.".into(),
            json!({}),
        )),
        "goal" => {
            let a: Goal = serde_json::from_value(args.clone())?;
            match a.action.as_str() {
                "set" => {
                    let labels = a.steps.ok_or("set needs steps")?;
                    let mut seen = std::collections::HashSet::new();
                    if labels.is_empty()
                        || labels.len() > 50
                        || labels
                            .iter()
                            .any(|s| s.trim().is_empty() || s.len() > 200 || !seen.insert(s))
                    {
                        return Err(
                            "Set needs 1–50 distinct named steps, each at most 200 bytes".into(),
                        );
                    }
                    *steps = labels
                        .into_iter()
                        .map(|label| json!({"label":label,"done":false}))
                        .collect();
                }
                "complete" => {
                    let index =
                        a.id.and_then(|n| n.checked_sub(1))
                            .filter(|i| *i < steps.len())
                            .ok_or("Invalid step number")?;
                    if steps[index]["done"] == true {
                        return Err("Step already complete".into());
                    }
                    steps[index]["done"] = json!(true);
                }
                "clear" => steps.clear(),
                _ => return Err("Unknown goal action".into()),
            }
            let completed = steps.iter().filter(|s| s["done"] == true).count();
            Ok(result(
                format!("Goal: {completed}/{} steps completed", steps.len()),
                json!({"hibiscusGoal":{"steps":steps,"completed":completed,"total":steps.len()}}),
            ))
        }
        _ => unreachable!("validated tool"),
    }
}

/// The last `keep` lines of `text`, each cut to 500 characters, with a note
/// on how many earlier lines were left out.
fn recent(text: &str, keep: usize) -> String {
    let lines: Vec<_> = text.lines().collect();
    let skipped = lines.len().saturating_sub(keep);
    let mut out = if skipped > 0 {
        format!("[{skipped} earlier lines not shown]\n")
    } else {
        String::new()
    };
    for line in &lines[skipped..] {
        let cut = line.char_indices().nth(500).map_or(line.len(), |(i, _)| i);
        out.push_str(&line[..cut]);
        out.push_str(if cut < line.len() { "…\n" } else { "\n" });
    }
    out
}

/// Largest screenshot sent to the model: long edge and total pixels, near what
/// providers accept without scaling it again themselves.
const SHOT_EDGE: f64 = 1568.0;
const SHOT_PIXELS: f64 = 1_150_000.0;

/// Reads a screenshot and scales it down to JPEG for the model. Returns the
/// base64 data and a note on its size.
fn screenshot(path: &Path) -> Result<(String, String)> {
    use base64::Engine;
    let image = image::open(path)?;
    let (w, h) = (image.width() as f64, image.height() as f64);
    let scale = (SHOT_EDGE / w.max(h))
        .min((SHOT_PIXELS / (w * h)).sqrt())
        .min(1.0);
    let (sw, sh) = (((w * scale) as u32).max(1), ((h * scale) as u32).max(1));
    let small = if scale < 1.0 {
        image.thumbnail_exact(sw, sh)
    } else {
        image
    };
    let mut jpeg = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&small.to_rgb8())?;
    let note = if scale < 1.0 {
        format!("The screenshot is attached, scaled from {w}x{h} to {sw}x{sh}.")
    } else {
        format!("The screenshot is attached ({sw}x{sh}).")
    };
    Ok((base64::engine::general_purpose::STANDARD.encode(jpeg), note))
}

fn checkpoint_notice(receipt: &Value) -> String {
    if receipt["protected"] == true {
        format!(
            "\nCheckpoint {}",
            receipt["checkpoint"].as_str().unwrap_or("")
        )
    } else {
        format!(
            "\nNot checkpointed: {}",
            receipt["reason"].as_str().unwrap_or("outside coverage")
        )
    }
}

/// One exact replacement has a contiguous changed region. Trim equal lines at
/// both ends and retain one context line, without a quadratic diff algorithm.
pub(crate) fn edit_diff(before: &str, after: &str) -> (String, usize, usize) {
    let old = before.split_inclusive('\n').collect::<Vec<_>>();
    let new = after.split_inclusive('\n').collect::<Vec<_>>();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    if prefix == old.len() && prefix == new.len() {
        return (String::new(), 0, 0);
    }
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = old.len() - prefix - suffix;
    let added = new.len() - prefix - suffix;
    let start = prefix.saturating_sub(1);
    let context = usize::from(suffix > 0);
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    let mut diff = format!(
        "@@ -{},{} +{},{} @@\n",
        start + 1,
        old_end + context - start,
        start + 1,
        new_end + context - start
    );
    let mut append = |sign: char, line: &str| {
        if diff.len() >= 65536 {
            return;
        }
        diff.push(sign);
        diff.push_str(line.strip_suffix('\n').unwrap_or(line));
        diff.push('\n');
        if !line.ends_with('\n') {
            diff.push_str("\\ No newline at end of file\n");
        }
    };
    for line in &old[start..prefix] {
        append(' ', line);
    }
    for line in &old[prefix..old_end] {
        append('-', line);
    }
    for line in &new[prefix..new_end] {
        append('+', line);
    }
    for line in &new[new_end..new_end + context] {
        append(' ', line);
    }
    let (mut text, truncated) = bounded(&diff, 65536);
    if truncated || diff.len() >= 65536 {
        text.push_str("\n[Diff truncated]\n");
    }
    (text, added, removed)
}
fn bounded(text: &str, max: usize) -> (String, bool) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].into(), end < text.len())
}

/// The error line of a Python or Node.js crash in a script's output, such as
/// `SyntaxError: invalid syntax` after a `File "<stdin>", line 20` trace.
fn hidden_failure(text: &str) -> Option<&str> {
    let python = text.contains("Traceback (most recent call last)") || text.contains("  File \"");
    let node = text.contains("\n    at ");
    if !python && !node {
        return None;
    }
    text.lines().find(|l| {
        let name = l.split(':').next().unwrap_or_default();
        l.len() > name.len()
            && (name.ends_with("Error") || name.ends_with("Exception"))
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    })
}

#[cfg(test)]
mod tests {
    use super::{
        edit_diff, hidden_failure, numbered, recent, screenshot, timeline, validate, READ_BYTES,
        READ_LINES,
    };
    #[test]
    fn browser_logs_keep_the_latest_entries() {
        let text = (1..=150)
            .map(|i| format!("[log] {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = recent(&text, 100);
        assert!(out.starts_with("[50 earlier lines not shown]\n[log] 51\n"));
        assert!(out.ends_with("[log] 150\n"));
        assert_eq!(
            recent(&"x".repeat(600), 100),
            format!("{}…\n", "x".repeat(500))
        );
    }
    use serde_json::{json, Value};
    #[test]
    fn screenshots_are_scaled_to_jpeg_within_the_model_limits() {
        use base64::Engine;
        let path = std::env::temp_dir().join(format!("hibiscus-shot-{}.png", std::process::id()));
        image::RgbaImage::new(4000, 1000).save(&path).unwrap();
        let (data, note) = screenshot(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        let small = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((small.width(), small.height()), (1568, 392));
        assert!(note.contains("scaled from 4000x1000 to 1568x392"), "{note}");
    }
    #[test]
    fn dev_server_tool_checks_its_action() {
        let ok = |args: Value| validate("dev_server", &args).is_ok();
        assert!(ok(json!({"action":"start","command":"npm run dev"})));
        assert!(ok(json!({"action":"output","lines":100})));
        assert!(!ok(json!({"action":"restart"})));
        assert!(!ok(json!({"action":"output","lines":0})));
        assert!(!ok(json!({"action":"start","cwd":"/"})));
    }
    #[test]
    fn questions_come_with_a_few_short_answers() {
        std::env::set_var("HIBISCUS_CANVAS", "1");
        let ok = |args: Value| validate("ask_user", &args).is_ok();
        assert!(ok(
            json!({"questions":[{"question":"Which language?","options":["English","Bahasa Melayu"]}]})
        ));
        assert!(!ok(json!({"questions":[]})));
        assert!(!ok(
            json!({"questions":[{"question":"Which?","options":["Only one"]}]})
        ));
        assert!(!ok(
            json!({"questions":[{"question":" ","options":["a","b"]}]})
        ));
    }
    #[test]
    fn a_project_timeline_counts_scenes_but_not_captions() {
        let html = r#"<html><body><div id="root" data-composition-id="main" data-start="0" data-duration="118.0"><div id="bg"></div><div id="hook" class="clip" data-composition-id="hook" data-composition-src="compositions/hook.html" data-duration="9.4"></div><div data-composition-src="compositions/check.html"></div><div data-composition-src="compositions/captions.html"></div></div>"#;
        assert_eq!(timeline(html), Some((118.0, 2)));
        assert_eq!(timeline("<html>no composition</html>"), None);
    }

    #[test]
    fn reads_are_numbered_paged_and_cut_on_long_lines() {
        let long = "x".repeat(2500);
        let lines = vec!["one", "two", long.as_str(), "four"];
        let (text, end, cut) = numbered(&lines, 0, 2);
        assert_eq!(text, "     1\tone\n     2\ttwo\n");
        assert_eq!((end, cut), (2, false));
        let (text, end, cut) = numbered(&lines, 2, 10);
        assert!(
            text.starts_with("     3\txxx")
                && text.contains("… [line cut at 2000 characters]\n     4\tfour")
        );
        assert_eq!((end, cut), (4, true));
        // The byte limit ends a read on a whole line.
        let many: Vec<String> = (0..3000).map(|i| format!("{i:>40}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let (text, end, _) = numbered(&many, 0, READ_LINES);
        assert!(text.len() <= READ_BYTES && text.ends_with('\n') && end < 2000);
    }

    #[test]
    fn crashes_hidden_behind_a_later_command_are_found() {
        let python = "12 scenes, total 118.0s\n  File \"<stdin>\", line 20\n    block=r'''x'''\n           ^\nSyntaxError: invalid syntax\n0 error(s)\n";
        assert_eq!(hidden_failure(python), Some("SyntaxError: invalid syntax"));
        let traceback = "Traceback (most recent call last):\n  File \"build.py\", line 3, in <module>\nKeyError: 'hook'\nlint ok\n";
        assert_eq!(hidden_failure(traceback), Some("KeyError: 'hook'"));
        let node = "file.js:1\nSyntaxError: Unexpected token\n    at wrapSafe (node:internal)\n";
        assert_eq!(hidden_failure(node), Some("SyntaxError: Unexpected token"));
        assert_eq!(hidden_failure("ValueError: just a log line\n"), None);
        assert_eq!(hidden_failure("  File \"a\", line 1\nall good\n"), None);
    }

    #[test]
    fn storyboards_and_previews_stay_local_and_script_free() {
        std::env::set_var("HIBISCUS_CANVAS", "1");
        let ok = |name: &str, args: Value| validate(name, &args).is_ok();
        let svg = r#"<svg viewBox="0 0 1920 1080"><rect width="10" height="10"/></svg>"#;
        assert!(ok(
            "storyboard",
            json!({"beats":[{"line":"Hello","svg":svg}]})
        ));
        assert!(!ok("storyboard", json!({"beats":[]})));
        assert!(ok(
            "storyboard",
            json!({"from":5,"beats":[{"line":"Scene five","svg":svg,"component":"Meter list"}]})
        ));
        assert!(!ok(
            "storyboard",
            json!({"from":0,"beats":[{"line":"Hi","svg":svg}]})
        ));
        assert!(!ok(
            "storyboard",
            json!({"beats":[{"line":"Hi","svg":"<svg><script>alert(1)</script></svg>"}]})
        ));
        assert!(!ok(
            "storyboard",
            json!({"beats":[{"line":"Hi","svg":"<div>not svg</div>"}]})
        ));
        assert!(ok(
            "video_preview",
            json!({"url":"http://localhost:3002/","video":"/tmp/out.mp4"})
        ));
        assert!(ok(
            "video_preview",
            json!({"url":"http://127.0.0.1:3002/p"})
        ));
        assert!(!ok("video_preview", json!({"url":"https://example.com/"})));
        assert!(!ok(
            "video_preview",
            json!({"url":"http://localhost.example.com/"})
        ));
        // The app serves the preview when the agent names only the project.
        assert!(ok("video_preview", json!({"project":"video"})));
        assert!(!ok("video_preview", json!({"video":"/tmp/out.mp4"})));
    }
    #[test]
    fn text_to_speech_needs_a_script_and_the_canvas() {
        std::env::set_var("HIBISCUS_CANVAS", "1");
        let ok = |args: Value| validate("text_to_speech", &args).is_ok();
        assert!(ok(
            json!({"script":"Hello there.","video":"A product tour","references":["a.png"]})
        ));
        assert!(!ok(json!({"script":"  "})));
        assert!(!ok(json!({"video":"No script"})));
        assert!(!ok(json!({"script":"Hi","voice":"x"})));
        assert!(super::definitions(None)
            .iter()
            .any(|d| d["name"] == "text_to_speech"));
    }
    #[test]
    fn browser_tool_refuses_commands_outside_its_own_headless_chrome() {
        let ok = |args: Value| validate("browser", &json!({ "args": args })).is_ok();
        assert!(ok(json!(["open", "http://localhost:5173"])));
        assert!(ok(json!([
            "fill",
            "@e2",
            "--",
            "--profile is fine as typed text"
        ])));
        assert!(ok(json!(["snapshot", "-i"])));
        assert!(!ok(json!([])));
        assert!(!ok(json!(["--headed", "open", "x"])));
        assert!(!ok(json!(["install"])));
        assert!(!ok(json!(["stream", "status"])));
        assert!(!ok(json!(["open", "x", "--profile", "Default"])));
        assert!(!ok(json!(["open", "x", "--cdp=9222"])));
        assert!(!ok(json!(["open", "x", "--session", "other"])));
        assert!(validate("browser", &json!({"args":["open"],"timeout":601})).is_err());
    }
    #[test]
    fn edit_diff_trims_unchanged_lines_and_tracks_actual_line_numbers() {
        let before = "intro\ndef greet(name):\n    return name\nend\nfooter\n";
        let after =
            "intro\ndef greet(name):\n    name = name.strip()\n    return name\nend\nfooter\n";
        let (diff, added, removed) = edit_diff(before, after);
        assert_eq!((added, removed), (1, 0));
        assert_eq!(
            diff,
            "@@ -2,2 +2,3 @@\n def greet(name):\n+    name = name.strip()\n     return name\n"
        );
        assert!(!diff.contains("intro") && !diff.contains("footer"));
        let (diff, added, removed) = edit_diff(after, before);
        assert_eq!((added, removed), (0, 1));
        assert!(diff.contains("-    name = name.strip()"));
    }
    #[test]
    fn edit_diff_handles_empty_files_final_newlines_and_bounded_previews() {
        assert_eq!(edit_diff("same", "same"), (String::new(), 0, 0));
        assert_eq!(
            edit_diff("", "hi\n"),
            ("@@ -1,0 +1,1 @@\n+hi\n".into(), 1, 0)
        );
        assert!(edit_diff("hi", "hi\n")
            .0
            .contains("\\ No newline at end of file"));
        assert_eq!((edit_diff("hi\n", "").1, edit_diff("hi\n", "").2), (0, 1));
        let (diff, added, removed) = edit_diff(&"old\n".repeat(30000), &"new\n".repeat(30000));
        assert_eq!((added, removed), (30000, 30000));
        assert!(diff.len() < 65600 && diff.ends_with("[Diff truncated]\n"));
    }
}
